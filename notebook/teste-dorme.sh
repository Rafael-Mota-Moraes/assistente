#!/usr/bin/env bash
# teste-dorme — exercita o loop do `notebook/dorme` (F6.1) com agentes falsos, sem IA:
# cada condição de parada (STOP, RELATORIO.md, árvore suja, fora da base, sem progresso).
# Roda no devenv:  ./dev just teste-dorme
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DORME="$ROOT/notebook/dorme"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASSOS=0
FALHAS=0

ok() { PASSOS=$((PASSOS + 1)); echo "  ok: $1"; }
falha() { FALHAS=$((FALHAS + 1)); echo "  FALHA: $1"; }

espera_log() { # <log> <texto> <descrição>
  if grep -qF "$2" "$1"; then ok "$3"; else falha "$3 — log sem '$2'"; fi
}

novo_repo() { # <nome> → repo com commit inicial na branch dorme/<data>; ecoa o caminho
  local repo="$WORK/$1"
  mkdir -p "$repo"
  git -C "$repo" init -q -b main
  git -C "$repo" config user.email "teste@assistente.local"
  git -C "$repo" config user.name "Teste"
  git -C "$repo" config commit.gpgsign false
  echo "# projeto" >"$repo/README.md"
  git -C "$repo" add README.md
  git -C "$repo" commit -q -m "início"
  git -C "$repo" switch -q -c "dorme/2026-01-01"
  echo "$repo"
}

agente() { # <nome> <corpo bash> → script executável no lugar do opencode; ecoa o caminho
  local caminho="$WORK/agentes/$1"
  mkdir -p "$WORK/agentes"
  printf '#!/usr/bin/env bash\nset -eu\n%s\n' "$2" >"$caminho"
  chmod +x "$caminho"
  echo "$caminho"
}

roda_dorme() { # <repo> <agente> <log> [tentativas]
  local repo="$1" agente_path="$2" log="$3" tentativas="${4:-2}"
  (
    cd "$repo" &&
      AGENTE_PROMPT_FILE="$WORK/prompt.md" \
      AGENTE_STOP="$WORK/STOP" \
      AGENTE_LOG="$log" \
      AGENTE_MAX_MIN=1 \
      AGENTE_TENTATIVAS="$tentativas" \
      AGENTE_ESPERA=0 \
      AGENTE_JAIL="" \
      AGENTE_OPENCODE="$agente_path" \
      bash "$DORME"
  )
}

echo "prompt falso" >"$WORK/prompt.md"

echo "== RELATORIO.md escrito pelo agente"
repo="$(novo_repo relatorio)"
log="$WORK/relatorio.log"
roda_dorme "$repo" "$(agente relatorio 'echo "preciso de decisão humana" > RELATORIO.md')" "$log"
espera_log "$log" "agente sinalizou parada via RELATORIO.md" "para quando o agente escreve RELATORIO.md"

echo "== árvore suja antes de começar"
repo="$(novo_repo sujo-antes)"
log="$WORK/sujo-antes.log"
echo "solto" >"$repo/sujo.txt"
roda_dorme "$repo" "$(agente nada 'exit 0')" "$log"
espera_log "$log" "árvore suja — parando p/ revisão humana" "nem roda o agente com a árvore suja"

echo "== árvore suja depois da tarefa"
repo="$(novo_repo sujo-depois)"
log="$WORK/sujo-depois.log"
roda_dorme "$repo" "$(agente sujo 'echo "mudança solta" > sujo.txt')" "$log"
espera_log "$log" "árvore suja após tarefa" "para quando o agente deixa a árvore suja"

echo "== RELATORIO.md já existe antes de começar"
repo="$(novo_repo relatorio-antes)"
log="$WORK/relatorio-antes.log"
echo "revisão pendente" >"$repo/RELATORIO.md"
roda_dorme "$repo" "$(agente nada2 'exit 0')" "$log"
espera_log "$log" "RELATORIO.md presente" "para antes de rodar se RELATORIO.md já existe"

echo "== agente termina fora da base"
repo="$(novo_repo fora-da-base)"
log="$WORK/fora-da-base.log"
roda_dorme "$repo" "$(agente fora 'git switch -q -c agente/fugiu')" "$log"
espera_log "$log" "terminou fora de dorme/2026-01-01" "para quando o agente termina fora da base"

echo "== sem progresso: desiste depois de N tentativas"
repo="$(novo_repo sem-progresso)"
log="$WORK/sem-progresso.log"
roda_dorme "$repo" "$(agente nada3 'exit 0')" "$log" 2
espera_log "$log" "2 tentativas sem progresso" "desiste depois de N tentativas sem progresso"
if [[ -f "$repo/RELATORIO.md" ]]; then
  ok "escreve RELATORIO.md ao desistir"
else
  falha "escreve RELATORIO.md ao desistir — arquivo ausente"
fi

echo "== progresso + STOP: commita, loop continua, STOP encerra"
repo="$(novo_repo stop)"
log="$WORK/stop.log"
antes="$(git -C "$repo" rev-parse HEAD)"
# Aspas simples de propósito: $AGENTE_STOP expande no agente falso, em tempo de execução.
# shellcheck disable=SC2016
roda_dorme "$repo" "$(agente commit-stop 'echo trabalho >> feito.txt
git add feito.txt
git commit -q -m "tarefa feita"
touch "$AGENTE_STOP"')" "$log"
depois="$(git -C "$repo" rev-parse HEAD)"
espera_log "$log" "progresso registrado" "registra progresso quando o agente commita"
espera_log "$log" "modo-dormir encerrado" "encerra quando o arquivo STOP aparece"
if [[ "$antes" != "$depois" ]]; then
  ok "o commit do agente ficou na base"
else
  falha "o commit do agente ficou na base — HEAD não mudou"
fi
if [[ "$(git -C "$repo" branch --show-current)" == "dorme/2026-01-01" ]]; then
  ok "continua na branch base"
else
  falha "continua na branch base — está em $(git -C "$repo" branch --show-current)"
fi

echo
echo "$PASSOS verificações ok, $FALHAS falhas."
[[ "$FALHAS" -eq 0 ]]
