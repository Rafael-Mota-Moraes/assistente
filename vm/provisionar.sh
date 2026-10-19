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

echo "==> ai-jail"
if ! command -v ai-jail >/dev/null; then
  tmp="$(mktemp -d)"
  curl -fsSL "$AI_JAIL_URL" | tar -xz -C "$tmp"
  install -m755 "$tmp/ai-jail" /usr/local/bin/ai-jail
  rm -rf "$tmp"
fi

echo "==> agentes (instalados no usuário)"
as_user 'command -v claude >/dev/null || curl -fsSL https://claude.ai/install.sh | bash'
as_user 'command -v opencode >/dev/null || curl -fsSL https://opencode.ai/install | bash'

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
