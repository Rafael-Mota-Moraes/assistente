#!/usr/bin/env bash
# Atualizador do Assistente (F8.5). Roda na VM, fora do ai-jail, desanexado do hubd.
# Uso: atualizar.sh <commit>
# Variáveis (sobrescrevem os padrões, úteis nos testes):
#   ASSISTENTE_REPO            raiz do repositório (padrão: pai desta pasta)
#   ASSISTENTE_VERSOES        pasta de versões (padrão: ~/.assistente/versoes)
#   ASSISTENTE_ATUALIZACOES   pasta de estado/log (padrão: ~/.assistente/atualizacoes)
#   ASSISTENTE_CHECK_CMD      comando de verificação (padrão: ./dev just check)
#   ASSISTENTE_BUILD_UI_CMD   comando de build da UI (padrão: ./dev just build-ui)
#   ASSISTENTE_BUILD_HUBD_CMD comando de build do hubd (padrão: ./dev just build-hubd)
#   ASSISTENTE_HUBD_ARTIFACT  binário gerado (padrão: target/release/hubd)
#   ASSISTENTE_HUBD_SERVICE   unidade systemd (padrão: hubd)
#   ASSISTENTE_RESTART_CMD    reinício do serviço (padrão: systemctl restart <serviço>; nos
#                             testes troca o serviço por um falso). Sem systemd e sem esta
#                             variável, o reinício é pulado.
#   ASSISTENTE_HEALTH_URL     URL de saúde (padrão: http://127.0.0.1:7878/api/health)
#   ASSISTENTE_HEALTH_TRIES   tentativas de saúde após o reinício (padrão: 30)
#   ASSISTENTE_HEALTH_WAIT    segundos entre tentativas (padrão: 1)
#   ASSISTENTE_KEEP_VERSIONS  quantas versões manter (padrão: 3)
set -euo pipefail

COMMIT="${1:?uso: atualizar.sh <commit>}"
REPO="${ASSISTENTE_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
HOME_DIR="${HOME:?HOME não definido}"
VERSOES="${ASSISTENTE_VERSOES:-$HOME_DIR/.assistente/versoes}"
ATUALIZACOES="${ASSISTENTE_ATUALIZACOES:-$HOME_DIR/.assistente/atualizacoes}"
CHECK_CMD="${ASSISTENTE_CHECK_CMD:-./dev just check}"
BUILD_UI_CMD="${ASSISTENTE_BUILD_UI_CMD:-./dev just build-ui}"
BUILD_HUBD_CMD="${ASSISTENTE_BUILD_HUBD_CMD:-./dev just build-hubd}"
HUBD_ARTIFACT="${ASSISTENTE_HUBD_ARTIFACT:-target/release/hubd}"
SERVICE="${ASSISTENTE_HUBD_SERVICE:-hubd}"
RESTART_CMD="${ASSISTENTE_RESTART_CMD:-}"
HEALTH_URL="${ASSISTENTE_HEALTH_URL:-http://127.0.0.1:7878/api/health}"
HEALTH_TRIES="${ASSISTENTE_HEALTH_TRIES:-30}"
HEALTH_WAIT="${ASSISTENTE_HEALTH_WAIT:-1}"
KEEP="${ASSISTENTE_KEEP_VERSIONS:-3}"

STATE_DIR="$ATUALIZACOES/$COMMIT"
SRC="$STATE_DIR/src"
LOG="$STATE_DIR/log"
ESTADO="$STATE_DIR/estado.json"
mkdir -p "$STATE_DIR" "$VERSOES"

write_state() {
  # $1 = status, $2 = erro (opcional)
  local status="$1" erro="${2:-}"
  python3 - "$ESTADO" "$COMMIT" "$status" "$erro" <<'PY'
import json, sys, time
path, commit, status, erro = sys.argv[1:5]
state = {
    "commit": commit,
    "status": status,
    "error": erro or None,
    "updated_at": int(time.time()),
}
try:
    old = json.load(open(path))
    state = {**old, **state}
except Exception:
    state["started_at"] = int(time.time())
if status in ("success", "failed"):
    state["finished_at"] = int(time.time())
json.dump(state, open(path, "w"))
PY
}

echo "[$(date -Is)] iniciando atualização para $COMMIT" | tee "$LOG"
write_state running

# Reinício do hubd: comando configurado (testes) ou systemd. Sem os dois, não reinicia.
restart_hubd() {
  if [ -n "$RESTART_CMD" ]; then
    bash -c "$RESTART_CMD"
  else
    systemctl restart "$SERVICE"
  fi
}

can_restart() {
  [ -n "$RESTART_CMD" ] || [ "$(ps -p 1 -o comm= 2>/dev/null)" = "systemd" ]
}

cleanup_worktree() {
  if [ -d "$SRC" ]; then
    git -C "$REPO" worktree remove --force "$SRC" >/dev/null 2>&1 || rm -rf "$SRC"
  fi
}
trap 'cleanup_worktree' EXIT

# 1. worktree limpo do commit pedido
if git -C "$REPO" worktree list | grep -q " $SRC "; then
  git -C "$REPO" worktree remove --force "$SRC" >/dev/null 2>&1 || true
fi
if ! git -C "$REPO" worktree add --detach "$SRC" "$COMMIT" >>"$LOG" 2>&1; then
  echo "falha ao criar worktree" | tee -a "$LOG"
  write_state failed "falha ao criar worktree"
  exit 1
fi

# 2. verificação
if ! (cd "$SRC" && bash -c "$CHECK_CMD" >>"$LOG" 2>&1); then
  echo "verificação falhou; nada muda" | tee -a "$LOG"
  write_state failed "verificação falhou"
  exit 1
fi

# 3. build das camadas
if ! (cd "$SRC" && bash -c "$BUILD_UI_CMD" >>"$LOG" 2>&1); then
  echo "build da UI falhou" | tee -a "$LOG"
  write_state failed "build da UI falhou"
  exit 1
fi
if ! (cd "$SRC" && bash -c "$BUILD_HUBD_CMD" >>"$LOG" 2>&1); then
  echo "build do hubd falhou" | tee -a "$LOG"
  write_state failed "build do hubd falhou"
  exit 1
fi

# 4. instala em versoes/<commit>
DEST="$VERSOES/$COMMIT"
rm -rf "$DEST"
mkdir -p "$DEST/ui"
if [ -f "$SRC/$HUBD_ARTIFACT" ]; then
  install -m755 "$SRC/$HUBD_ARTIFACT" "$DEST/hubd"
else
  echo "binário do hubd não encontrado: $SRC/$HUBD_ARTIFACT" | tee -a "$LOG"
  write_state failed "binário do hubd não encontrado"
  exit 1
fi
if [ -d "$SRC/app/dist" ]; then
  cp -a "$SRC/app/dist/." "$DEST/ui/"
else
  echo "build da UI ausente: $SRC/app/dist" | tee -a "$LOG"
  write_state failed "build da UI ausente"
  exit 1
fi

# 5. troca os links atual/anterior
if [ -e "$VERSOES/atual" ] && [ "$(readlink -f "$VERSOES/atual" 2>/dev/null)" != "$(readlink -f "$DEST" 2>/dev/null)" ]; then
  rm -rf "$VERSOES/anterior"
  if [ -L "$VERSOES/atual" ]; then
    cp -a "$VERSOES/atual" "$VERSOES/anterior" 2>/dev/null || true
  else
    mv "$VERSOES/atual" "$VERSOES/anterior"
  fi
fi
rm -rf "$VERSOES/atual"
ln -s "$COMMIT" "$VERSOES/atual"

# 6. reinicia o hubd se ele mudou e confere saúde
hubd_changed=0
if [ -f "$VERSOES/anterior/hubd" ]; then
  if ! cmp -s "$VERSOES/anterior/hubd" "$DEST/hubd"; then
    hubd_changed=1
  fi
else
  hubd_changed=1
fi

if [ "$hubd_changed" -eq 1 ] && can_restart; then
  restart_hubd || true
  # espera até HEALTH_TRIES tentativas
  ok=0
  for _ in $(seq 1 "$HEALTH_TRIES"); do
    if curl -fsS "$HEALTH_URL" 2>/dev/null | grep -q "$COMMIT"; then
      ok=1
      break
    fi
    sleep "$HEALTH_WAIT"
  done
  if [ "$ok" -ne 1 ]; then
    echo "hubd não respondeu com o commit novo; voltando" | tee -a "$LOG"
    # rollback
    rm -rf "$VERSOES/atual"
    if [ -e "$VERSOES/anterior" ]; then
      mv "$VERSOES/anterior" "$VERSOES/atual"
    fi
    restart_hubd || true
    write_state failed "hubd não respondeu com o commit novo"
    exit 1
  fi
fi

# 7. limpa worktree e mantém as últimas versões
cleanup_worktree
trap - EXIT
# mantém atual, anterior (e os alvos dos dois) e as $KEEP versões mais recentes
protected="atual anterior"
for link in atual anterior; do
  if target="$(readlink -f "$VERSOES/$link" 2>/dev/null)" && [ -n "$target" ]; then
    protected="$protected $(basename "$target")"
  fi
done
pattern="^($(printf '%s' "$protected" | tr ' ' '|'))$"
find "$VERSOES" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %f\n' \
  | sort -rn \
  | sed 's/^[^ ]* //' \
  | { grep -v -E "$pattern" || true; } \
  | tail -n +$((KEEP + 1)) \
  | while read -r old; do
      rm -rf "${VERSOES:?}/$old"
    done

write_state success
echo "[$(date -Is)] atualização concluída: $COMMIT" | tee -a "$LOG"