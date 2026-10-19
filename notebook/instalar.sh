#!/usr/bin/env bash
# Prepara o notebook (Linux Mint) para o Assistente. Rode NO NOTEBOOK, como seu usuário:
#   bash instalar.sh            (o `bin/assistente notebook instalar` copia e roda por SSH)
# NÃO TESTADO (F4.1). Spec: docs/specs/07-notebook-e-9router.md
#
# O que faz: sshd, bubblewrap, tmux, git; ai-jail; servidor ai-memory (systemd do usuário);
# pastas ~/assistente/{projetos,logs,briefings}; instala `dorme` em ~/.local/bin.
# O que NÃO faz: instalar/configurar o 9router e o opencode (já existem aí) nem abrir portas
# na LAN — o PC acessa tudo pelo túnel SSH.
set -euo pipefail

AI_JAIL_URL="https://github.com/akitaonrails/ai-jail/releases/latest/download/ai-jail-linux-x86_64.tar.gz"
AI_MEMORY_INSTALL="https://github.com/akitaonrails/ai-memory/releases/latest/download/install.sh"
# TODO(F4.1): fixar versões e verificar checksums em vez de "latest".

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# O instalador do opencode só põe ~/.opencode/bin no .bashrc (interativo).
export PATH="$HOME/.opencode/bin:$HOME/.local/bin:$PATH"

echo "==> pacotes do sistema (pede sudo)"
sudo apt-get update
sudo apt-get install -y openssh-server bubblewrap tmux git curl
sudo systemctl enable --now ssh
# Serviços systemd --user (ai-memory) continuam rodando sem sessão aberta.
sudo loginctl enable-linger "$USER"

echo "==> ai-jail"
if ! command -v ai-jail >/dev/null; then
  tmp="$(mktemp -d)"
  curl -fsSL "$AI_JAIL_URL" | tar -xz -C "$tmp"
  install -Dm755 "$tmp/ai-jail" "$HOME/.local/bin/ai-jail"
  rm -rf "$tmp"
fi

echo "==> ai-memory (servidor principal da memória)"
if ! command -v ai-memory >/dev/null; then
  curl -fsSL "$AI_MEMORY_INSTALL" | sh
fi
# setup (v3.x): init + serviço systemd --user + plugin/MCP do opencode ("open-code").
# TODO(F5.1): confirmar que o serviço escuta só em 127.0.0.1:49374 e onde fica o token.
ai-memory setup --yes --agent open-code

echo "==> pastas e dorme"
mkdir -p "$HOME/assistente/projetos" "$HOME/assistente/logs" "$HOME/assistente/briefings"
install -Dm755 "$here/dorme" "$HOME/.local/bin/dorme"

echo "==> conferindo serviços"
# Qualquer resposta HTTP conta (o 9router responde 401 sem a chave).
check() {
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 "$2" || true)"
  if [ "$code" != "000" ]; then echo "  ok  $1 (HTTP $code)"; else echo "  FALHOU  $1 ($2)"; fi
}
check 9router "http://127.0.0.1:20128/v1/models"
check ai-memory "http://127.0.0.1:49374/"

echo
echo "Notebook pronto."
