#!/usr/bin/env bash
# teste-backup — exercita `assistente backup`/`restaurar`/`vm logins` (F7.1, F9.4) com um
# incus falso, sem VM: config + espelho + logins + configurações dos projetos, --sem-logins,
# VM parada e o envio dos logins.
# Roda no devenv:  ./dev just teste-backup
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ASSISTENTE="$ROOT/bin/assistente"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASSOS=0
FALHAS=0
ok() { PASSOS=$((PASSOS + 1)); echo "  ok: $1"; }
falha() { FALHAS=$((FALHAS + 1)); echo "  FALHA: $1"; }

USER_NAME="$(id -un)"
HOME1="$WORK/home"
HOME2="$WORK/home2"
VM_HOME="$WORK/vm"
mkdir -p "$WORK/bin" "$HOME1/.config/assistente" "$HOME1/.local/share/assistente/memoria-copia/wiki"
echo "# wiki" >"$HOME1/.local/share/assistente/memoria-copia/wiki/inicio.md"
cat >"$HOME1/.config/assistente/assistente.toml" <<TOML
[vm]
name = "assistente-teste"

[memory]
mirror_dir = "$HOME1/.local/share/assistente/memoria-copia"
TOML

# VM falsa: só o ~/.assistente/perfis interessa ao backup.
mkdir -p "$VM_HOME/.assistente/perfis/pessoal/claude" "$VM_HOME/.assistente/perfis/pessoal/ssh"
echo '{"token":"secreto"}' >"$VM_HOME/.assistente/perfis/pessoal/claude/.credentials.json"
chmod 600 "$VM_HOME/.assistente/perfis/pessoal/claude/.credentials.json"
echo "chave" >"$VM_HOME/.assistente/perfis/pessoal/ssh/id_ed25519"
printf '[pessoal.game]\ndocker = true\n' >"$VM_HOME/.assistente/projetos.toml"

# incus falso: `info`/`list` respondem pelo estado em FAKE_VM_STATE e `exec` roda o comando
# localmente, trocando /home/<usuário> pelo home falso da VM.
cat >"$WORK/bin/incus" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  info)
    [ "${FAKE_VM_STATE:-running}" = "absent" ] && exit 1
    exit 0
    ;;
  list)
    if [ "${FAKE_VM_STATE:-running}" = "stopped" ]; then echo STOPPED; else echo RUNNING; fi
    ;;
  exec)
    shift        # exec
    shift        # nome da instância
    [ "${1:-}" = "--" ] && shift
    if [ "${1:-}" = "sudo" ]; then
      shift
      while [ $# -gt 0 ]; do
        case "$1" in
          -u) shift 2 ;;
          -i|-H|-n) shift ;;
          *) break ;;
        esac
      done
    fi
    args=()
    for a in "$@"; do args+=("$(sed "s|/home/$FAKE_USER|$FAKE_VM_HOME|g" <<<"$a")"); done
    "${args[@]}"
    ;;
  *)
    echo "incus falso: comando não suportado: $*" >&2
    exit 1
    ;;
esac
FAKE
chmod +x "$WORK/bin/incus"

ESTADO="running"
roda_em() { # <home> <comando…>
  local home="$1"
  shift
  (
    HOME="$home" PATH="$WORK/bin:$PATH" \
      FAKE_USER="$USER_NAME" FAKE_VM_HOME="$VM_HOME" FAKE_VM_STATE="$ESTADO" \
      bash "$ASSISTENTE" "$@"
  )
}
roda() { roda_em "$HOME1" "$@"; }

echo "== backup completo: config, espelho e logins da VM"
if saida="$(roda backup "$WORK/completo.tar.gz" 2>&1)"; then
  ok "backup termina com a VM ligada"
else
  falha "backup termina com a VM ligada — $saida"
fi
lista="$(tar -tzf "$WORK/completo.tar.gz" 2>/dev/null)"
for item in \
  ".config/assistente/assistente.toml" \
  ".local/share/assistente/memoria-copia/wiki/inicio.md" \
  "vm-perfis/pessoal/claude/.credentials.json" \
  "vm-perfis/pessoal/ssh/id_ed25519" \
  "vm-projetos.toml"; do
  if grep -qxF "$item" <<<"$lista"; then ok "o tar tem $item"; else falha "o tar tem $item — ausente"; fi
done

extraido="$WORK/extraido"
mkdir -p "$extraido"
tar -xzf "$WORK/completo.tar.gz" -C "$extraido" vm-perfis
if cmp -s "$extraido/vm-perfis/pessoal/claude/.credentials.json" \
  "$VM_HOME/.assistente/perfis/pessoal/claude/.credentials.json"; then
  ok "credenciais copiadas sem alteração"
else
  falha "credenciais copiadas sem alteração — conteúdo diferente"
fi
if [ "$(stat -c %a "$extraido/vm-perfis/pessoal/claude/.credentials.json")" = "600" ]; then
  ok "modo 600 preservado"
else
  falha "modo 600 preservado — veio $(stat -c %a "$extraido/vm-perfis/pessoal/claude/.credentials.json")"
fi

echo "== --sem-logins não exige a VM"
ESTADO="absent"
if saida="$(roda backup --sem-logins "$WORK/sem-logins.tar.gz" 2>&1)"; then
  ok "--sem-logins funciona sem VM"
else
  falha "--sem-logins funciona sem VM — $saida"
fi
if tar -tzf "$WORK/sem-logins.tar.gz" 2>/dev/null | grep -q "^vm-perfis/"; then
  falha "--sem-logins não inclui logins — veio vm-perfis/"
else
  ok "--sem-logins não inclui logins"
fi

echo "== VM parada: falha alto e não gera tar"
rm -f "$WORK/parada.tar.gz"
ESTADO="stopped"
if saida="$(roda backup "$WORK/parada.tar.gz" 2>&1)"; then
  falha "backup com VM parada falha — terminou ok"
else
  ok "backup com VM parada falha"
fi
if grep -q "VM assistente-teste parada" <<<"$saida"; then
  ok "o erro diz que a VM está parada"
else
  falha "o erro diz que a VM está parada — $saida"
fi
if [ -e "$WORK/parada.tar.gz" ]; then
  falha "backup com VM parada não gera tar — arquivo criado"
else
  ok "backup com VM parada não gera tar"
fi

echo "== restaurar num home zerado"
ESTADO="running"
mkdir -p "$HOME2"
if saida="$(roda_em "$HOME2" restaurar "$WORK/completo.tar.gz" 2>&1)"; then
  ok "restaurar termina"
else
  falha "restaurar termina — $saida"
fi
if [ -f "$HOME2/.config/assistente/assistente.toml" ]; then
  ok "config restaurada"
else
  falha "config restaurada — ausente"
fi
if [ -f "$HOME2/.local/share/assistente/memoria-copia/wiki/inicio.md" ]; then
  ok "espelho restaurado"
else
  falha "espelho restaurado — ausente"
fi
if [ -e "$HOME2/vm-perfis" ]; then
  falha "logins não vão para o home do host — vm-perfis/ criado"
else
  ok "logins não vão para o home do host"
fi
if [ -f "$HOME2/.local/share/assistente/restauracao/perfis/pessoal/claude/.credentials.json" ]; then
  ok "logins ficam à espera da VM"
else
  falha "logins ficam à espera da VM — ausente"
fi
if [ -e "$HOME2/vm-projetos.toml" ]; then
  falha "configurações dos projetos não vão para o home — vm-projetos.toml criado"
elif grep -q "docker = true" "$HOME2/.local/share/assistente/restauracao/projetos.toml" 2>/dev/null; then
  ok "configurações dos projetos ficam à espera da VM"
else
  falha "configurações dos projetos ficam à espera da VM — ausente"
fi

echo "== restaurar backup antigo (sem logins) limpa pendências"
HOME3="$WORK/home3"
mkdir -p "$HOME3/.local/share/assistente/restauracao/perfis/pessoal"
tar -czf "$WORK/antigo.tar.gz" -C "$HOME1" ".config/assistente" ".local/share/assistente/memoria-copia"
if saida="$(roda_em "$HOME3" restaurar "$WORK/antigo.tar.gz" 2>&1)"; then
  ok "restaurar backup antigo termina"
else
  falha "restaurar backup antigo termina — $saida"
fi
if [ -f "$HOME3/.config/assistente/assistente.toml" ]; then
  ok "backup antigo restaura a config"
else
  falha "backup antigo restaura a config — ausente"
fi
if [ -e "$HOME3/.local/share/assistente/restauracao/perfis" ]; then
  falha "backup antigo limpa pendências — pasta ficou"
else
  ok "backup antigo limpa pendências"
fi

echo "== vm logins copia os logins restaurados"
rm -rf "$VM_HOME/.assistente"
if saida="$(roda_em "$HOME2" vm logins 2>&1)"; then
  ok "vm logins termina"
else
  falha "vm logins termina — $saida"
fi
if [ -f "$VM_HOME/.assistente/perfis/pessoal/claude/.credentials.json" ]; then
  ok "credenciais chegam à VM"
else
  falha "credenciais chegam à VM — ausente"
fi
if grep -q "docker = true" "$VM_HOME/.assistente/projetos.toml" 2>/dev/null \
  && [ "$(stat -c %a "$VM_HOME/.assistente/projetos.toml")" = "600" ]; then
  ok "configurações dos projetos chegam à VM (modo 600)"
else
  falha "configurações dos projetos chegam à VM (modo 600) — ausente ou modo errado"
fi
if [ -e "$HOME2/.local/share/assistente/restauracao/projetos.toml" ]; then
  falha "configurações pendentes são consumidas — arquivo ficou"
else
  ok "configurações pendentes são consumidas"
fi
if [ -d "$HOME2/.local/share/assistente/restauracao/perfis" ]; then
  falha "logins pendentes são consumidos — pasta ficou"
else
  ok "logins pendentes são consumidos"
fi

echo "== vm logins sem restauração pendente"
if saida="$(roda_em "$HOME2" vm logins 2>&1)"; then
  falha "vm logins sem pendência falha — terminou ok"
else
  ok "vm logins sem pendência falha"
fi
if grep -q "nada para copiar" <<<"$saida"; then
  ok "o erro explica que falta restaurar"
else
  falha "o erro explica que falta restaurar — $saida"
fi

echo
echo "$PASSOS verificações ok, $FALHAS falhas."
[[ "$FALHAS" -eq 0 ]]
