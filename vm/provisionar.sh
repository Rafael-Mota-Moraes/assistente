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
as_user '[ -x ~/.opencode/bin/opencode ] || curl -fsSL https://opencode.ai/install | bash'
# O instalador do opencode só mexe no .bashrc (interativo); hubd e tmux precisam dele no PATH.
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
