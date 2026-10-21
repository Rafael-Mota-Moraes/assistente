#!/usr/bin/env bash
# teste-atualizar — exercita `vm/atualizar.sh` (F8.5) com um repositório de teste e um serviço
# falso: aplicação, troca de versão, verificação que falha, saúde quebrada (volta sozinho) e poda.
# Roda no devenv:  ./dev just teste-atualizar
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ATUALIZAR="$ROOT/vm/atualizar.sh"
WORK="$(mktemp -d)"
export HOME="$WORK/home"
mkdir -p "$HOME"
VERSOES="$HOME/.assistente/versoes"
ATUALIZACOES="$HOME/.assistente/atualizacoes"
REPO="$WORK/repo"
SITE="$WORK/site"
PIDFILE="$WORK/servico.pid"

PASSOS=0
FALHAS=0
ok() { PASSOS=$((PASSOS + 1)); echo "  ok: $1"; }
falha() { FALHAS=$((FALHAS + 1)); echo "  FALHA: $1"; }

limpar() {
  if [ -f "$PIDFILE" ]; then
    kill "$(cat "$PIDFILE")" 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap 'limpar' EXIT

# Porta livre para o serviço falso.
PORTA="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
HEALTH_URL="http://127.0.0.1:$PORTA/api/health"

# Serviço falso: responde em /api/health o conteúdo do binário instalado (o "commit" que roda).
cat >"$WORK/servico.sh" <<'SH'
#!/usr/bin/env bash
# Reinício do "hubd" falso: derruba o servidor antigo e sobe um novo servindo o binário atual.
set -uo pipefail
if [ -f "$PID_TESTE" ]; then
  antigo="$(cat "$PID_TESTE")"
  kill "$antigo" 2>/dev/null || true
  for _ in $(seq 1 50); do
    kill -0 "$antigo" 2>/dev/null || break
    sleep 0.02
  done
  rm -f "$PID_TESTE"
fi
mkdir -p "$SITE_TESTE/api"
cp "$VERSOES_TESTE/atual/hubd" "$SITE_TESTE/api/health" 2>/dev/null || echo '{"commit":"?"}' >"$SITE_TESTE/api/health"
setsid python3 -m http.server "$PORTA_TESTE" --bind 127.0.0.1 --directory "$SITE_TESTE" >/dev/null 2>&1 &
echo $! >"$PID_TESTE"
SH

BUILD_UI_CMD='mkdir -p app/dist && git rev-parse HEAD > app/dist/commit.txt'
BUILD_HUBD_CMD='mkdir -p target/release && git rev-parse HEAD > target/release/hubd'
CHECK_CMD=true
KEEP_VERSIONS=3

roda() { # $1 = commit; usa CHECK_CMD/BUILD_HUBD_CMD/KEEP_VERSIONS do momento
  ASSISTENTE_REPO="$REPO" \
    ASSISTENTE_VERSOES="$VERSOES" \
    ASSISTENTE_ATUALIZACOES="$ATUALIZACOES" \
    ASSISTENTE_CHECK_CMD="$CHECK_CMD" \
    ASSISTENTE_BUILD_UI_CMD="$BUILD_UI_CMD" \
    ASSISTENTE_BUILD_HUBD_CMD="$BUILD_HUBD_CMD" \
    ASSISTENTE_HUBD_ARTIFACT="target/release/hubd" \
    ASSISTENTE_RESTART_CMD="bash '$WORK/servico.sh'" \
    ASSISTENTE_HEALTH_URL="$HEALTH_URL" \
    ASSISTENTE_HEALTH_TRIES=20 \
    ASSISTENTE_HEALTH_WAIT=0.1 \
    ASSISTENTE_KEEP_VERSIONS="$KEEP_VERSIONS" \
    VERSOES_TESTE="$VERSOES" SITE_TESTE="$SITE" PID_TESTE="$PIDFILE" PORTA_TESTE="$PORTA" \
    bash "$ATUALIZAR" "$1"
}

campo_estado() { # $1 = commit, $2 = campo
  python3 - "$ATUALIZACOES/$1/estado.json" "$2" <<'PY'
import json, sys
try:
    estado = json.load(open(sys.argv[1]))
except Exception:
    estado = {}
print(estado.get(sys.argv[2]) or "")
PY
}

N=0
novo_commit() { # $1 = mensagem; imprime o hash
  N=$((N + 1))
  echo "$1" >"$REPO/arquivo-$N.txt"
  git -C "$REPO" add .
  git -C "$REPO" commit -qm "$1"
  git -C "$REPO" rev-parse HEAD
}

# Repositório de teste com dois commits bons.
mkdir -p "$REPO"
git -C "$REPO" init -q
git -C "$REPO" config user.name teste
git -C "$REPO" config user.email teste@teste
COMMIT_A="$(novo_commit "feat: primeiro")"
COMMIT_B="$(novo_commit "feat: segundo")"

echo "== aplicação inicial (sem versão anterior)"
if roda "$COMMIT_A"; then
  ok "aplicar o primeiro commit termina bem"
else
  falha "aplicar o primeiro commit falhou"
  tail -5 "$ATUALIZACOES/$COMMIT_A/log" 2>/dev/null
fi
if [ "$(readlink "$VERSOES/atual")" = "$COMMIT_A" ]; then
  ok "atual aponta para o commit aplicado"
else
  falha "atual não aponta para $COMMIT_A"
fi
if [ -f "$VERSOES/$COMMIT_A/hubd" ] && [ -f "$VERSOES/$COMMIT_A/ui/commit.txt" ]; then
  ok "binário e UI instalados na versão"
else
  falha "binário/UI ausentes em $VERSOES/$COMMIT_A"
fi
if [ "$(campo_estado "$COMMIT_A" status)" = "success" ]; then
  ok "estado termina em success"
else
  falha "estado não é success: $(campo_estado "$COMMIT_A" status)"
fi
if [ ! -e "$VERSOES/anterior" ]; then
  ok "primeira aplicação não cria anterior"
else
  falha "anterior existe sem versão anterior"
fi
if [ ! -d "$ATUALIZACOES/$COMMIT_A/src" ]; then
  ok "worktree é removido no fim"
else
  falha "worktree ficou para trás"
fi
if curl -fsS "$HEALTH_URL" 2>/dev/null | grep -q "$COMMIT_A"; then
  ok "o serviço falso responde com o commit novo"
else
  falha "o serviço falso não responde com $COMMIT_A"
fi

echo "== atualização com versão anterior"
if roda "$COMMIT_B"; then
  ok "aplicar o segundo commit termina bem"
else
  falha "aplicar o segundo commit falhou"
  tail -5 "$ATUALIZACOES/$COMMIT_B/log" 2>/dev/null
fi
if [ "$(readlink -f "$VERSOES/anterior")" = "$VERSOES/$COMMIT_A" ]; then
  ok "anterior guarda a versão que estava rodando"
else
  falha "anterior não aponta para $COMMIT_A"
fi

echo "== saúde quebrada volta sozinho"
COMMIT_C="$(novo_commit "feat: quebra a saúde")"
COMMIT_ANTES="$COMMIT_B"
BUILD_HUBD_CMD='mkdir -p target/release && printf "quebrado\n" > target/release/hubd'
if roda "$COMMIT_C"; then
  falha "commit sem saúde deveria falhar"
else
  ok "commit sem saúde termina em erro"
fi
if [ "$(readlink "$VERSOES/atual")" = "$COMMIT_ANTES" ]; then
  ok "volta para a versão anterior"
else
  falha "não voltou: atual aponta para $(readlink "$VERSOES/atual")"
fi
if [ "$(campo_estado "$COMMIT_C" status)" = "failed" ] && campo_estado "$COMMIT_C" error | grep -q "não respondeu"; then
  ok "estado explica que a saúde não respondeu"
else
  falha "estado sem o motivo da falha"
fi
if grep -q "voltando" "$ATUALIZACOES/$COMMIT_C/log"; then
  ok "log registra a volta"
else
  falha "log não registra a volta"
fi
BUILD_HUBD_CMD='mkdir -p target/release && git rev-parse HEAD > target/release/hubd'

echo "== verificação que falha não muda nada"
COMMIT_D="$(novo_commit "feat: quebra o check")"
CHECK_CMD=false
if roda "$COMMIT_D"; then
  falha "verificação que falha deveria abortar"
else
  ok "verificação que falha aborta"
fi
CHECK_CMD=true
if [ "$(readlink "$VERSOES/atual")" = "$COMMIT_ANTES" ]; then
  ok "atual não muda quando a verificação falha"
else
  falha "atual mudou mesmo com verificação falhando"
fi
if [ "$(campo_estado "$COMMIT_D" status)" = "failed" ] && [ ! -d "$VERSOES/$COMMIT_D" ]; then
  ok "nada é instalado quando a verificação falha"
else
  falha "versão instalada (ou estado errado) após verificação falhando"
fi

echo "== poda mantém atual, anterior e as mais recentes"
COMMIT_E="$(novo_commit "feat: depois da poda")"
KEEP_VERSIONS=1
if roda "$COMMIT_E"; then
  ok "aplicar com poder de poda termina bem"
else
  falha "aplicar com poda falhou"
fi
if [ "$(readlink -f "$VERSOES/anterior")" = "$VERSOES/$COMMIT_B" ]; then
  ok "anterior aponta para a versão em uso antes da troca"
else
  falha "anterior não aponta para $COMMIT_B"
fi
if [ -d "$VERSOES/$COMMIT_B" ]; then
  ok "o alvo do link anterior não é podado"
else
  falha "a poda apagou o alvo do link anterior"
fi
if [ ! -d "$VERSOES/$COMMIT_A" ]; then
  ok "versão antiga fora dos links é podada"
else
  falha "versão antiga $COMMIT_A ficou no disco com KEEP=1"
fi

echo
echo "$PASSOS verificações ok, $FALHAS falhas."
[[ "$FALHAS" -eq 0 ]]
