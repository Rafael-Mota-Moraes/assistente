#!/usr/bin/env bash
# Testes do vm/limpar-disco (F1.7) com df e docker falsos: só limpa abaixo do mínimo e,
# quando limpa, apaga volumes sem uso e o cache de build.
# Roda no `just check`. Spec: docs/specs/01-vm-e-instalacao.md
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="$ROOT/vm/limpar-disco"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
falhas=0

ok() { echo "ok: $*"; }
falha() { echo "FALHOU: $*" >&2; falhas=$((falhas + 1)); }

mkdir -p "$work/bin"
cat >"$work/bin/df" <<'FAKE'
#!/usr/bin/env bash
echo " Avail"
echo " $FAKE_LIVRE_GB"
FAKE
cat >"$work/bin/docker" <<'FAKE'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_CHAMADAS"
FAKE
chmod +x "$work/bin"/*

rodar() { # <GB livres> [mínimo]
  : >"$work/chamadas"
  PATH="$work/bin:$PATH" \
    FAKE_LIVRE_GB="$1" \
    FAKE_CHAMADAS="$work/chamadas" \
    LIMPAR_DISCO_MIN_GB="${2:-10}" \
    bash "$SCRIPT" 2>&1
}

chamadas() { tr '\n' ';' <"$work/chamadas"; }

# 1) Disco com folga: não chama o docker.
saida="$(rodar 30)"
if [ -z "$(chamadas)" ] && grep -q 'disco ok' <<<"$saida"; then
  ok "disco com folga não limpa nada"
else
  falha "folga: chamadas=[$(chamadas)] saida=[$saida]"
fi

# 2) Disco quase cheio: apaga volumes sem uso e o cache de build.
saida="$(rodar 3)"
if [ "$(chamadas)" = "volume prune -a -f;builder prune -a -f;" ]; then
  ok "disco quase cheio apaga volumes sem uso e cache de build"
else
  falha "cheio: chamadas=[$(chamadas)] saida=[$saida]"
fi

# 3) Exatamente no mínimo conta como ok; o mínimo vem de LIMPAR_DISCO_MIN_GB.
saida="$(rodar 20 20)"
if [ -z "$(chamadas)" ]; then
  ok "no mínimo não limpa"
else
  falha "no mínimo: chamadas=[$(chamadas)] saida=[$saida]"
fi
saida="$(rodar 19 20)"
if [ -n "$(chamadas)" ]; then
  ok "mínimo configurável"
else
  falha "mínimo configurável: chamadas=[$(chamadas)] saida=[$saida]"
fi

if [ "$falhas" -gt 0 ]; then
  echo "$falhas falha(s) em teste-limpar-disco.sh" >&2
  exit 1
fi
echo "teste-limpar-disco.sh: tudo ok"
