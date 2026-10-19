#!/usr/bin/env bash
# Bootstrap do PC (Pop!_OS / Ubuntu 24.04) depois de formatar:
#   git clone <este repo> && cd assistente && ./instalar.sh [backup.tar.gz]
# NÃO TESTADO (F1.1/F7.2). Spec: docs/specs/01-vm-e-instalacao.md
#
# 1. Instala Incus + QEMU (VM) — Docker já é pré-requisito (ambiente ./dev).
# 2. Restaura a config do backup, ou cria uma a partir de config/assistente.exemplo.toml.
# 3. Gera o token do hub, cria a VM e provisiona.
# 4. Compila e instala o app desktop (.deb).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONF_DIR="$HOME/.config/assistente"
BACKUP="${1:-}"

command -v docker >/dev/null || { echo "instale o Docker antes (é o ambiente de build ./dev)"; exit 1; }

echo "==> Incus + QEMU (pede sudo)"
if ! command -v incus >/dev/null; then
  sudo apt-get update
  sudo apt-get install -y incus qemu-system-x86 ovmf
  sudo usermod -aG incus-admin "$USER"
  sudo incus admin init --minimal
  echo "Entrou no grupo incus-admin: faça logout/login e rode ./instalar.sh de novo."
  exit 0
fi

echo "==> configuração"
if [ -n "$BACKUP" ]; then
  "$ROOT/bin/assistente" restaurar "$BACKUP"
elif [ ! -f "$CONF_DIR/assistente.toml" ]; then
  mkdir -p "$CONF_DIR"
  cp "$ROOT/config/assistente.exemplo.toml" "$CONF_DIR/assistente.toml"
  echo "Criei $CONF_DIR/assistente.toml a partir do exemplo. Ajuste perfis/notebook e rode de novo."
  exit 0
fi
"$ROOT/bin/assistente" token

echo "==> VM"
incus info "$(python3 -c "import tomllib;print(tomllib.load(open('$CONF_DIR/assistente.toml','rb'))['vm']['name'])")" >/dev/null 2>&1 \
  || "$ROOT/bin/assistente" vm criar

echo "==> app desktop"
"$ROOT/dev" just build
sudo apt-get install -y "$ROOT"/target/release/bundle/deb/*.deb

cat <<'FIM'

Pronto. Próximos passos:
  bin/assistente login claude pessoal      (e trabalho, se usar)
  bin/assistente notebook instalar         (uma vez por notebook)
  Abra "Assistente" no menu de aplicativos.
FIM
