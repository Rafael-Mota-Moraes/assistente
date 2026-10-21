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
      AGENTE_QUADRO="${AGENTE_QUADRO:-}" \
      CONTADOR="${CONTADOR:-}" \
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

com_quadro() { # <repo> <json> → grava o quadro e commita (o dorme exige árvore limpa)
  mkdir -p "$1/.assistente"
  printf '%s\n' "$2" >"$1/.assistente/kanban.json"
  git -C "$1" add .assistente/kanban.json
  git -C "$1" commit -q -m "quadro de teste"
}

coluna() { # <repo> <id> → coluna da tarefa no quadro commitado
  python3 -c 'import json, sys
cards = {c["id"]: c for c in json.load(open(sys.argv[1]))["cards"]}
print(cards[sys.argv[2]]["column"])' "$1/.assistente/kanban.json" "$2"
}

echo "== quadro: a tarefa de maior prioridade vai primeiro e vira feito com o commit do agente"
repo="$(novo_repo quadro-feito)"
log="$WORK/quadro-feito.log"
com_quadro "$repo" '{"next":4,"cards":[
 {"id":"T1","title":"primeira","description":"faça A","priority":"normal","kind":"agente","column":"backlog","created_at":1},
 {"id":"T2","title":"segunda","description":"faça B","priority":"alta","kind":"agente","column":"backlog","created_at":2},
 {"id":"T3","title":"manual","description":"","priority":"alta","kind":"manual","column":"backlog","created_at":3}]}'
# Agente falso: cada chamada faz um commit; a segunda pede parada.
# Aspas simples de propósito, como no teste de progresso: $CONTADOR expande no agente falso.
# shellcheck disable=SC2016
AGENTE_QUADRO=.assistente/kanban.json CONTADOR="$WORK/contador-feito" roda_dorme "$repo" "$(agente quadro-feito 'n=$(cat "$CONTADOR" 2>/dev/null || echo 0)
n=$((n+1)); echo "$n" >"$CONTADOR"
echo trabalho >>feito.txt; git add feito.txt; git commit -q -m "tarefa $n"
if [ "$n" -ge 2 ]; then touch "$AGENTE_STOP"; fi')" "$log" >/dev/null
espera_log "$log" "tarefa do quadro: T2" "pega primeiro a tarefa de prioridade alta"
espera_log "$log" "tarefa do quadro: T1" "depois pega a tarefa normal"
if [[ "$(coluna "$repo" T1)" == feito && "$(coluna "$repo" T2)" == feito && "$(coluna "$repo" T3)" == backlog ]]; then
  ok "T1 e T2 ficam em feito; a manual não é tocada"
else
  falha "colunas: T1=$(coluna "$repo" T1) T2=$(coluna "$repo" T2) T3=$(coluna "$repo" T3)"
fi
commit_t2="$(python3 -c 'import json, sys
print([c for c in json.load(open(sys.argv[1]))["cards"] if c["id"] == "T2"][0].get("commit", ""))' "$repo/.assistente/kanban.json")"
if [[ -n "$commit_t2" && "$commit_t2" == "$(git -C "$repo" log --format=%H --grep='^tarefa 1$')" ]]; then
  ok "o card guarda o commit que o agente fez"
else
  falha "o card guarda o commit do agente — commit=$commit_t2"
fi

echo "== quadro: tarefa sem commit volta ao backlog e para depois das tentativas"
repo="$(novo_repo quadro-falha)"
log="$WORK/quadro-falha.log"
com_quadro "$repo" '{"next":2,"cards":[{"id":"T1","title":"difícil","description":"","priority":"normal","kind":"agente","column":"backlog","created_at":1}]}'
AGENTE_QUADRO=.assistente/kanban.json roda_dorme "$repo" "$(agente quadro-falha 'exit 0')" "$log" 2 >/dev/null
espera_log "$log" "tarefa T1 sem commit: voltou ao backlog" "explica no log que a tarefa voltou ao backlog"
espera_log "$log" "2 tentativas sem progresso" "para depois das tentativas"
if [[ "$(coluna "$repo" T1)" == backlog && -f "$repo/RELATORIO.md" ]]; then
  ok "T1 volta ao backlog e o RELATORIO.md explica"
else
  falha "T1 volta ao backlog — coluna=$(coluna "$repo" T1)"
fi

echo "== quadro sem tarefa para agente: não itera"
repo="$(novo_repo quadro-vazio)"
log="$WORK/quadro-vazio.log"
marca="$WORK/rodou-quadro-vazio"
com_quadro "$repo" '{"next":2,"cards":[{"id":"T1","title":"só manual","description":"","priority":"normal","kind":"manual","column":"backlog","created_at":1}]}'
AGENTE_QUADRO=.assistente/kanban.json roda_dorme "$repo" "$(agente quadro-vazio "touch '$marca'")" "$log" >/dev/null
espera_log "$log" "quadro sem tarefas para agente" "para sem iterar quando o quadro não tem tarefa para agente"
if [[ ! -f "$marca" ]]; then ok "o agente nem é chamado"; else falha "o agente foi chamado sem tarefa"; fi

echo
echo "$PASSOS verificações ok, $FALHAS falhas."
[[ "$FALHAS" -eq 0 ]]
