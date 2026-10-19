#!/usr/bin/env bash
# Provisiona a VM do Assistente. Roda como root DENTRO da VM; reexecutável.
#   bin/assistente vm criar   (copia este script, o hubd e a unidade e chama aqui)
# Spec: docs/specs/01-vm-e-instalacao.md (F1.3)
set -euo pipefail

USER_NAME="${1:?uso: provisionar.sh <usuario>}"
HOME_DIR="/home/$USER_NAME"
USER_UID="$(id -u "$USER_NAME")"
as_user() { sudo -u "$USER_NAME" -H bash -lc "$1"; }
# systemctl --user a partir de root (incus exec) não tem bus de sessão; linger + user@ + env.
user_systemctl() {
  loginctl enable-linger "$USER_NAME" 2>/dev/null || true
  local runtime="/run/user/$USER_UID"
  if [ ! -S "$runtime/bus" ]; then
    systemctl start "user@${USER_UID}.service"
  fi
  sudo -u "$USER_NAME" -H \
    XDG_RUNTIME_DIR="$runtime" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime/bus" \
    systemctl --user "$@"
}

AI_JAIL_VERSION="2.10.0"
AI_JAIL_URL="https://github.com/akitaonrails/ai-jail/releases/download/v${AI_JAIL_VERSION}/ai-jail-linux-x86_64.tar.gz"
AI_JAIL_SHA256="cd4b4b5dad69d6fb11a0cd9a17e6c2e1848cac4ba3d8da0a507a6eb535dc0714"

CLAUDE_VERSION="2.1.303"
CLAUDE_INSTALL="https://claude.ai/install.sh"

AI_MEMORY_VERSION="3.1.1"
AI_MEMORY_INSTALL="https://github.com/akitaonrails/ai-memory/releases/download/v${AI_MEMORY_VERSION}/install.sh"

# O opencode 2 sai no npm como @opencode/cli (o instalador do opencode.ai só instala o 1.x). O
# binário vem do pacote da plataforma, numa versão fixa publicada há 14+ dias e conferida pelo
# sha512 (o `dist.integrity` do registry).
OPENCODE_VERSION="2.0.23"
OPENCODE_URL="https://registry.npmjs.org/@opencode/cli-linux-x64/-/cli-linux-x64-$OPENCODE_VERSION.tgz"
OPENCODE_SHA512="5CQisdKXLDXVjUZYF1ROqIH/HS5YUwo4SjEBJ3DndATJL2vFgbvN0pqZQkKQqBjUrZ19jNXDHvp+LP/uGIECAQ=="

sha256_file() { sha256sum "$1" | awk '{print $1}'; }

echo "==> home do usuário"
# O Incus cria /home/<usuario> (e as pastas até os pontos de montagem) como root antes do
# cloud-init criar o usuário. Ajusta só o que é root e está no disco da VM (-xdev não entra
# nas montagens virtiofs) e completa os arquivos do /etc/skel que o useradd pulou.
find "$HOME_DIR" -xdev -user root -exec chown "$USER_NAME:" {} +
cp -rn /etc/skel/. "$HOME_DIR/"
chown "$USER_NAME:" "$HOME_DIR"/.bashrc "$HOME_DIR"/.profile "$HOME_DIR"/.bash_logout 2>/dev/null || true

echo "==> ai-jail"
if command -v ai-jail >/dev/null && ai-jail --version 2>/dev/null | grep -q "$AI_JAIL_VERSION"; then
  :
else
  echo "    ai-jail $AI_JAIL_VERSION"
  tmp="$(mktemp -d)"
  curl -fsSL "$AI_JAIL_URL" -o "$tmp/ai-jail.tar.gz"
  [ "$(sha256_file "$tmp/ai-jail.tar.gz")" = "$AI_JAIL_SHA256" ] || {
    echo "sha256 do ai-jail não confere" >&2
    exit 1
  }
  tar -xzf "$tmp/ai-jail.tar.gz" -C "$tmp"
  install -m755 "$tmp/ai-jail" /usr/local/bin/ai-jail
  rm -rf "$tmp"
fi

echo "==> agentes (instalados no usuário)"
if as_user "command -v claude >/dev/null && claude --version 2>/dev/null | grep -q \"$CLAUDE_VERSION\""; then
  :
else
  echo "    claude $CLAUDE_VERSION"
  as_user "curl -fsSL '$CLAUDE_INSTALL' | bash -s -- $CLAUDE_VERSION"
fi
if ! as_user "$HOME_DIR/.opencode/bin/opencode --version 2>/dev/null" | grep -q "$OPENCODE_VERSION\$"; then
  echo "    opencode $OPENCODE_VERSION"
  tmp="$(mktemp -d)"
  curl -fsSL "$OPENCODE_URL" -o "$tmp/opencode.tgz"
  got="$(python3 -c 'import base64, hashlib, sys; print(base64.b64encode(hashlib.sha512(open(sys.argv[1], "rb").read()).digest()).decode())' "$tmp/opencode.tgz")"
  [ "$got" = "$OPENCODE_SHA512" ] || { echo "sha512 do opencode não confere" >&2; exit 1; }
  tar -xzf "$tmp/opencode.tgz" -C "$tmp" package/bin/opencode
  install -d -o "$USER_NAME" -g "$USER_NAME" "$HOME_DIR/.opencode" "$HOME_DIR/.opencode/bin"
  install -m755 -o "$USER_NAME" -g "$USER_NAME" "$tmp/package/bin/opencode" "$HOME_DIR/.opencode/bin/opencode"
  rm -rf "$tmp"
fi
# hubd e tmux não leem o .bashrc: o opencode precisa estar no PATH.
as_user 'mkdir -p ~/.local/bin && ln -sf ~/.opencode/bin/opencode ~/.local/bin/opencode'

if as_user "command -v ai-memory >/dev/null && ai-memory --version 2>/dev/null | grep -q \"$AI_MEMORY_VERSION\""; then
  :
else
  echo "    ai-memory $AI_MEMORY_VERSION"
  as_user "curl -fsSL '$AI_MEMORY_INSTALL' | AI_MEMORY_VERSION='$AI_MEMORY_VERSION' sh"
fi

echo "==> ai-memory (servidor local, F5.0)"
read -r MIRROR DATA PORT <<< "$(python3 - "$HOME_DIR" <<'PY'
import os, sys, tomllib
home = sys.argv[1]
cfg = tomllib.load(open(os.path.join(home, ".config/assistente/assistente.toml"), "rb"))
mem = cfg["memory"]

def expand(path: str) -> str:
    if path.startswith("~/"):
        return os.path.join(home, path[2:])
    if path == "~":
        return home
    return path

mirror = expand(mem["mirror_dir"])
data = expand(mem.get("data_dir", "~/.local/share/assistente/ai-memory"))
print(mirror, data, mem.get("port", 49374))
PY
)"
install -d -o "$USER_NAME" -g "$USER_NAME" "$MIRROR/wiki" "$DATA"
# Wiki no espelho virtiofs; SQLite em disco local. Symlink é recusado pelo ai-memory.
if mountpoint -q "$DATA/wiki"; then
  umount "$DATA/wiki"
fi
as_user "rm -rf '$DATA/wiki'"
install -d -o "$USER_NAME" -g "$USER_NAME" "$DATA/wiki"
WIKI_UNIT="$(systemd-escape -p --suffix=mount "$DATA/wiki")"
rm -f /etc/systemd/system/assistente-ai-memory-wiki.mount
cat > "/etc/systemd/system/$WIKI_UNIT" <<EOF
[Unit]
Description=ai-memory wiki (espelho do host via bind)

[Mount]
What=$MIRROR/wiki
Where=$DATA/wiki
Type=none
Options=bind

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable --now "$WIKI_UNIT"
as_user "test -f '$DATA/config.toml' || AI_MEMORY_DATA_DIR='$DATA' ai-memory init --data-dir '$DATA'"
install -d -o "$USER_NAME" -g "$USER_NAME" "$HOME_DIR/.config/systemd/user"
sed -e "s|@HOME@|$HOME_DIR|g" -e "s|@DATA_DIR@|$DATA|g" -e "s/@PORT@/$PORT/g" \
  /tmp/assistente/ai-memory.service > "$HOME_DIR/.config/systemd/user/ai-memory.service"
chown "$USER_NAME:" "$HOME_DIR/.config/systemd/user/ai-memory.service"
user_systemctl daemon-reload
user_systemctl enable --now ai-memory.service
# Hooks/MCP por perfil: bin/assistente agentes configurar

echo "==> pastas por perfil"
# Lidas pelo hubd: ~/.assistente/perfis/<perfil>/{claude,opencode,ssh}
python3 - "$HOME_DIR" <<'PY'
import sys, tomllib, pathlib
home = pathlib.Path(sys.argv[1])
cfg = tomllib.loads((home / ".config/assistente/assistente.toml").read_text())
for pid in cfg["profiles"]:
    for sub in ("claude", "opencode", "ssh"):
        (home / ".assistente/perfis" / pid / sub).mkdir(parents=True, exist_ok=True)
PY
chown -R "$USER_NAME:" "$HOME_DIR/.assistente"

echo "==> hubd"
install -m755 /tmp/assistente/hubd /usr/local/bin/hubd
sed "s/@USER@/$USER_NAME/g" /tmp/assistente/hubd.service > /etc/systemd/system/hubd.service
systemctl daemon-reload
systemctl enable --now hubd
systemctl restart hubd

echo "Provisionamento concluído."
