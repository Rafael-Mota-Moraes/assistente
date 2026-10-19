#!/usr/bin/env bash
# Provisiona a VM do Assistente. Roda como root DENTRO da VM; reexecutável.
#   bin/assistente vm criar   (copia este script, o hubd e a unidade e chama aqui)
# NÃO TESTADO (F1.3). Spec: docs/specs/01-vm-e-instalacao.md
set -euo pipefail

USER_NAME="${1:?uso: provisionar.sh <usuario>}"
HOME_DIR="/home/$USER_NAME"
as_user() { sudo -u "$USER_NAME" -H bash -lc "$1"; }

AI_JAIL_URL="https://github.com/akitaonrails/ai-jail/releases/latest/download/ai-jail-linux-x86_64.tar.gz"
AI_MEMORY_INSTALL="https://github.com/akitaonrails/ai-memory/releases/latest/download/install.sh"
# TODO(F1.3): fixar versões e verificar checksums em vez de "latest".
# O opencode 2 sai no npm como @opencode/cli (o instalador do opencode.ai só instala o 1.x). O
# binário vem do pacote da plataforma, numa versão fixa publicada há 14+ dias e conferida pelo
# sha512 (o `dist.integrity` do registry).
OPENCODE_VERSION="2.0.23"
OPENCODE_URL="https://registry.npmjs.org/@opencode/cli-linux-x64/-/cli-linux-x64-$OPENCODE_VERSION.tgz"
OPENCODE_SHA512="5CQisdKXLDXVjUZYF1ROqIH/HS5YUwo4SjEBJ3DndATJL2vFgbvN0pqZQkKQqBjUrZ19jNXDHvp+LP/uGIECAQ=="

echo "==> home do usuário"
# O Incus cria /home/<usuario> (e as pastas até os pontos de montagem) como root antes do
# cloud-init criar o usuário. Ajusta só o que é root e está no disco da VM (-xdev não entra
# nas montagens virtiofs) e completa os arquivos do /etc/skel que o useradd pulou.
find "$HOME_DIR" -xdev -user root -exec chown "$USER_NAME:" {} +
cp -rn /etc/skel/. "$HOME_DIR/"
chown "$USER_NAME:" "$HOME_DIR"/.bashrc "$HOME_DIR"/.profile "$HOME_DIR"/.bash_logout 2>/dev/null || true

echo "==> ai-jail"
if ! command -v ai-jail >/dev/null; then
  tmp="$(mktemp -d)"
  curl -fsSL "$AI_JAIL_URL" | tar -xz -C "$tmp"
  install -m755 "$tmp/ai-jail" /usr/local/bin/ai-jail
  rm -rf "$tmp"
fi

echo "==> agentes (instalados no usuário)"
as_user 'command -v claude >/dev/null || curl -fsSL https://claude.ai/install.sh | bash'
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

echo "==> ai-memory (cliente; o servidor fica no notebook, via túnel em 127.0.0.1:49374)"
as_user "command -v ai-memory >/dev/null || curl -fsSL $AI_MEMORY_INSTALL | sh"
# TODO(F5.2): install-mcp/install-hooks por perfil (CLAUDE_CONFIG_DIR / OPENCODE_CONFIG)
# com --server-url http://127.0.0.1:49374 e o token do servidor.

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
