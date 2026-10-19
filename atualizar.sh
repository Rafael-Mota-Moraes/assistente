#!/usr/bin/env bash
# Atualiza tudo de uma vez: puxa o código, põe o hubd novo na VM, reinstala o app e o reabre.
#   ./atualizar.sh
# As sessões tmux sobrevivem; só o app desktop é fechado e aberto de novo.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="/usr/bin/assistente"

echo "==> código"
if [ -n "$(git -C "$ROOT" status --porcelain)" ]; then
  echo "Há mudanças locais não commitadas; atualizando com o código como está."
else
  git -C "$ROOT" pull --ff-only
fi

"$ROOT/bin/assistente" atualizar

echo "==> reabrindo o app"
pkill -x -f "$APP" || true
for _ in $(seq 20); do
  pgrep -x -f "$APP" >/dev/null || break
  sleep 0.5
done
setsid -f "$APP" >/dev/null 2>&1
echo "Pronto."
