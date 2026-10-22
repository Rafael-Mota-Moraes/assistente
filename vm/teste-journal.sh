#!/usr/bin/env bash
# Testes do bloco "journal do hubd sem sudo" do vm/provisionar.sh (F14.2).
# Comandos falsos (getent, usermod, id): confere que o usuário entra no grupo
# systemd-journal, que rodar duas vezes não falha e que a falta do grupo avisa e segue.
# Roda no `just check`. Spec: docs/specs/01-vm-e-instalacao.md
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROV="$ROOT/vm/provisionar.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
falhas=0

ok() { echo "ok: $*"; }
falha() { echo "FALHOU: $*" >&2; falhas=$((falhas + 1)); }

# O bloco do provisionar.sh: do echo do journal até o do hubd.
bloco="$(awk '/^echo "==> journal do hubd sem sudo/{f=1} /^echo "==> hubd"/{f=0} f' "$PROV")"
case "$bloco" in
  *usermod*) ;;
  *)
    echo "FALHOU: o provisionar.sh não tem o bloco do journal (F14.2)" >&2
    exit 1
    ;;
esac

# Comandos falsos: a "conta" é um arquivo com um grupo por linha.
mkdir -p "$work/bin"
cat >"$work/bin/getent" <<'FAKE'
#!/usr/bin/env bash
# O grupo systemd-journal existe nesta VM? Controlado por FAKE_GRUPO_EXISTE.
[ "${FAKE_GRUPO_EXISTE:-1}" = "1" ] || exit 2
echo "systemd-journal:x:999:"
FAKE
cat >"$work/bin/usermod" <<'FAKE'
#!/usr/bin/env bash
echo "$*" >>"$FAKE_CHAMADAS"
# FAKE_ADICIONA=0 simula um usermod que não muda nada na conta.
[ "${FAKE_ADICIONA:-1}" = "1" ] || exit 0
# `usermod -aG` não duplica o grupo.
grep -qx systemd-journal "$FAKE_GRUPOS" || echo systemd-journal >>"$FAKE_GRUPOS"
FAKE
cat >"$work/bin/id" <<'FAKE'
#!/usr/bin/env bash
[ "$1" = "-nG" ] || exit 1
tr '\n' ' ' <"$FAKE_GRUPOS"
echo
FAKE
chmod +x "$work/bin"/*

rodar() { # <saída do bloco> (stdout e stderr junto)
  printf 'pessoal\ndocker\n' >"$work/grupos"
  : >"$work/chamadas"
  PATH="$work/bin:$PATH" \
    FAKE_GRUPOS="$work/grupos" \
    FAKE_CHAMADAS="$work/chamadas" \
    USER_NAME=pessoal \
    bash -c "$bloco" 2>&1
}

grupos() { tr '\n' ' ' <"$work/grupos"; }
chamadas() { tr '\n' ' ' <"$work/chamadas"; }

# 1) A VM tem o grupo: o usuário entra nele e nada é avisado.
saida="$(rodar)"
if [ "$(chamadas)" = "-aG systemd-journal pessoal " ] \
  && grep -qx systemd-journal "$work/grupos" \
  && ! grep -q 'aviso:' <<<"$saida"; then
  ok "usuário entra no grupo systemd-journal"
else
  falha "entrar: chamadas=[$(chamadas)] grupos=[$(grupos)] saida=[$saida]"
fi

# 2) Rodar de novo não falha nem duplica o grupo (o provisionar.sh é reexecutável).
saida="$(rodar)"
saida="$(rodar)"
if grep -qx systemd-journal "$work/grupos" \
  && [ "$(grep -c '^systemd-journal$' "$work/grupos")" = 1 ] \
  && ! grep -q 'aviso:' <<<"$saida"; then
  ok "segunda passada não duplica o grupo"
else
  falha "idempotente: grupos=[$(grupos)] saida=[$saida]"
fi

# 3) A VM não tem o grupo: avisa, não chama o usermod e segue (o resto da VM funciona).
saida="$(FAKE_GRUPO_EXISTE=0 rodar || true)"
if [ -z "$(chamadas)" ] && grep -q 'systemd-journal' <<<"$saida"; then
  ok "VM sem o grupo avisa e segue"
else
  falha "sem grupo: chamadas=[$(chamadas)] saida=[$saida]"
fi

# 4) O usermod roda mas o grupo não entra: o aviso aponta o problema na hora.
saida="$(FAKE_ADICIONA=0 rodar || true)"
if grep -q 'aviso:' <<<"$saida" && ! grep -qx systemd-journal "$work/grupos"; then
  ok "grupo que não entra avisa"
else
  falha "não entrou: grupos=[$(grupos)] saida=[$saida]"
fi

if [ "$falhas" -gt 0 ]; then
  echo "$falhas falha(s) em teste-journal.sh" >&2
  exit 1
fi
echo "teste-journal.sh: tudo ok"
