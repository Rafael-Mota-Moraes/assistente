#!/usr/bin/env bash
# Testes do vm/trabalhador (F11.6): opencode e fora-do-jail falsos, repo git temporário.
# Roda no `just check`. Spec: docs/specs/10-kanban-e-trabalhadores.md
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TRAB="$ROOT/vm/trabalhador"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
falhas=0

ok() { echo "ok: $*"; }
falha() { echo "FALHOU: $*" >&2; falhas=$((falhas + 1)); }

# opencode falso: o modelo "bom" faz um commit; "sujo" commita e deixa lixo; os outros falham.
cat >"$work/opencode" <<'FAKE'
#!/usr/bin/env bash
modelo=""
standalone=0
while [ $# -gt 0 ]; do
  if [ "$1" = -m ]; then modelo="$2"; fi
  if [ "$1" = --standalone ]; then standalone=1; fi
  shift
done
# Como no opencode real: sem --standalone a porta 49374 (do ai-memory) está ocupada e ele aborta.
if [ "$standalone" = 0 ]; then
  echo "Error: Managed service port 49374 on 127.0.0.1 is already in use by another process."
  exit 1
fi
echo "$modelo" >>"$CHAMADAS"
echo "opencode falso trabalhando com $modelo"
case "$modelo" in
  bom) echo x >>arquivo.txt; git add arquivo.txt; git commit -qm "feito por bom"; exit 0 ;;
  sujo) echo x >>arquivo.txt; git add arquivo.txt; git commit -qm "parcial"; echo y >lixo.txt; exit 0 ;;
  # recusado pelo provedor e preso: o trabalhador tem que matar e trocar sem esperar.
  limite) echo "Error: rate limit exceeded (429)"; exec sleep 300 ;;
  # indisponível: morre logo, antes de qualquer vigia (caso real do opencode, F11.10).
  indisponivel) echo "Error: Model unavailable: opencode/$modelo"; exit 1 ;;
  descontinuado) echo "Error: Model $modelo has been deprecated."; exit 1 ;;
  # erro que nenhum padrão conhece, logo de cara: só a regra do "morreu rápido" pega.
  mudo) echo "Error: algo que ninguém previu"; exit 1 ;;
  *) exit 1 ;;
esac
FAKE
# fora-do-jail falso: só grava a chamada.
cat >"$work/fora" <<'FAKE'
#!/usr/bin/env bash
echo "$*" >>"$CHAMADAS"
FAKE
chmod +x "$work/opencode" "$work/fora"

repo_novo() { # <nome> → repo com um commit inicial
  local dir="$work/$1"
  mkdir -p "$dir"
  git -C "$dir" init -q -b agente/T3
  git -C "$dir" config user.email t@t
  git -C "$dir" config user.name t
  git -C "$dir" commit -q --allow-empty -m inicial
  echo "$dir"
}

rodar() { # <repo> <modelos> <tentativas> → código de saída do trabalhador
  local repo="$1"
  echo "prompt da tarefa" >"$work/prompt.md"
  rm -f "$work/chamadas" "$work/log" "$work/resultados" "$work/tela"
  (
    cd "$repo"
    AGENTE_TAREFA=T3 \
    AGENTE_MODELOS="$2" \
    AGENTE_TENTATIVAS="$3" \
    AGENTE_ESPERA=0 \
    AGENTE_MAX_MIN=1 \
    AGENTE_VIGIA=1 \
    AGENTE_RAPIDO="${RAPIDO:-0}" \
    AGENTE_PROMPT_FILE="$work/prompt.md" \
    AGENTE_LOG="$work/log" \
    AGENTE_RESULTADOS="$work/resultados" \
    AGENTE_OPENCODE="$work/opencode" \
    ASSISTENTE_FORA_DO_JAIL="$work/fora" \
    CHAMADAS="$work/chamadas" \
      bash "$TRAB" >"$work/tela" 2>&1
  ) && echo 0 || echo $?
}

# 1) O primeiro modelo esgota as tentativas; o reserva conclui: done com o commit dele.
repo="$(repo_novo concluir)"
rc="$(rodar "$repo" "ruim bom" 2)"
sha="$(git -C "$repo" rev-parse HEAD)"
if [ "$rc" = 0 ] && grep -qx "kanban done T3 --commit $sha" "$work/chamadas" \
  && [ "$(grep -c '^ruim$' "$work/chamadas")" = 2 ] && grep -q 'ruim esgotou' "$work/log"; then
  ok "modelo de reserva conclui e marca o commit"
else
  falha "reserva: rc=$rc chamadas=$(tr '\n' ' ' <"$work/chamadas" 2>/dev/null)"
fi
# A aba do trabalhador mostra a saída do opencode enquanto ele roda, não só o log do loop.
if grep -qx "opencode falso trabalhando com bom" "$work/tela"; then
  ok "saída do opencode aparece na tela do trabalhador"
else
  falha "tela: $(tr '\n' ' ' <"$work/tela" 2>/dev/null)"
fi

# 2) Tudo falha: a tarefa volta ao backlog e o trabalhador sai com erro.
repo="$(repo_novo esgotar)"
rc="$(rodar "$repo" "ruim pior" 1)"
if [ "$rc" = 1 ] && grep -qx "kanban release T3" "$work/chamadas" \
  && ! grep -q 'kanban done' "$work/chamadas"; then
  ok "sem sucesso devolve a tarefa ao backlog"
else
  falha "esgotar: rc=$rc chamadas=$(tr '\n' ' ' <"$work/chamadas" 2>/dev/null)"
fi

# 3) Commit com lixo na árvore não conta como progresso.
repo="$(repo_novo sujo)"
rc="$(rodar "$repo" "sujo" 1)"
if [ "$rc" = 1 ] && grep -qx "kanban release T3" "$work/chamadas"; then
  ok "árvore suja não é progresso"
else
  falha "sujo: rc=$rc chamadas=$(tr '\n' ' ' <"$work/chamadas" 2>/dev/null)"
fi

# 4) Erro de provedor no meio da tentativa: o modelo é morto na hora (não espera o sleep de 300s)
#    e a reserva assume. O histórico registra cada resultado para a escolha automática (F11.10).
repo="$(repo_novo vigia)"
inicio="$(date +%s)"
rc="$(rodar "$repo" "limite bom" 2)"
duracao=$(($(date +%s) - inicio))
sha="$(git -C "$repo" rev-parse HEAD)"
if [ "$rc" = 0 ] && [ "$duracao" -lt 60 ] && [ "$(grep -c '^limite$' "$work/chamadas")" = 1 ] \
  && grep -qx "kanban done T3 --commit $sha" "$work/chamadas" \
  && grep -qx "limite falhou" "$work/resultados" && grep -qx "bom ok" "$work/resultados"; then
  ok "erro de provedor troca o modelo na hora e grava o histórico"
else
  falha "vigia: rc=$rc duracao=${duracao}s chamadas=$(tr '\n' ' ' <"$work/chamadas" 2>/dev/null) resultados=$(tr '\n' ' ' <"$work/resultados" 2>/dev/null)"
fi

# 5) Modelo indisponível sai na hora com erro: não repete o mesmo modelo, troca para a reserva.
repo="$(repo_novo indisponivel)"
rc="$(rodar "$repo" "indisponivel descontinuado bom" 3)"
sha="$(git -C "$repo" rev-parse HEAD)"
if [ "$rc" = 0 ] && [ "$(grep -c '^indisponivel$' "$work/chamadas")" = 1 ] \
  && [ "$(grep -c '^descontinuado$' "$work/chamadas")" = 1 ] \
  && grep -qx "kanban done T3 --commit $sha" "$work/chamadas"; then
  ok "modelo indisponível troca para a reserva sem repetir"
else
  falha "indisponivel: rc=$rc chamadas=$(tr '\n' ' ' <"$work/chamadas" 2>/dev/null)"
fi

# 6) Erro desconhecido em poucos segundos: o opencode nem trabalhou, então troca de modelo na hora.
repo="$(repo_novo rapido)"
rc="$(RAPIDO=30 rodar "$repo" "mudo bom" 3)"
sha="$(git -C "$repo" rev-parse HEAD)"
if [ "$rc" = 0 ] && [ "$(grep -c '^mudo$' "$work/chamadas")" = 1 ] \
  && grep -qx "kanban done T3 --commit $sha" "$work/chamadas" && grep -q 'antes de trabalhar' "$work/log"; then
  ok "erro rápido desconhecido troca para a reserva sem repetir"
else
  falha "rapido: rc=$rc chamadas=$(tr '\n' ' ' <"$work/chamadas" 2>/dev/null)"
fi

if [ "$falhas" -gt 0 ]; then
  echo "$falhas falha(s) em teste-trabalhador.sh" >&2
  exit 1
fi
echo "teste-trabalhador.sh: tudo ok"
