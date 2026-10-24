#!/usr/bin/env bash
# Configura os agentes da VM: providers do opencode por perfil (F3.4), ai-memory local (F5.0),
# instruções do `fora-do-jail` (F9.6).
# Claude Code e opencode por perfil. Roda como o
# SEU usuário dentro da VM; reexecutável. Chamado por `bin/assistente agentes configurar`, que
# manda os providers + as policies do PC (JSON, com as chaves) pelo stdin.
# Specs: docs/specs/04-perfis-e-projetos.md, docs/specs/06-memoria.md (F5.2, F5.3)
set -euo pipefail

payload="{}"
[ -t 0 ] || payload="$(cat)"
[ -n "$payload" ] || payload="{}"
host_providers="$(python3 - "$payload" <<'PY'
import json, sys
print(json.dumps(json.loads(sys.argv[1]).get("provider") or {}))
PY
)"
host_policies="$(python3 - "$payload" <<'PY'
import json, sys
print(json.dumps(json.loads(sys.argv[1]).get("policies") or []))
PY
)"

URL="http://127.0.0.1:49374"
CONF="$HOME/.config/assistente/assistente.toml"
TOKEN_FILE="$HOME/.assistente/ai-memory.token"
export PATH="$HOME/.local/bin:$PATH"

umask 077
host="$(python3 -c "import tomllib;print(tomllib.load(open('$CONF','rb'))['notebook']['ssh_host'])")"
router_port="$(python3 -c "import tomllib;print(tomllib.load(open('$CONF','rb'))['notebook']['tunnel']['9router'])")"
OC="$HOME/.config/opencode/opencode.json"

echo "==> opencode: config global (sem providers — as chaves vão por perfil, F3.4)"
# Só toca no notebook se algum perfil for usar o 9router.
needs_router="$(python3 - "$CONF" <<'PY'
import sys, tomllib
profiles = tomllib.load(open(sys.argv[1], "rb"))["profiles"]
print(any("9router" in p.get("opencode_providers", ["9router"]) for p in profiles.values()))
PY
)"
router_key=""
router_models=""
if [ "$needs_router" = "True" ]; then
  echo "==> 9router: chave e modelos (notebook)"
  # A chave fica no server.env do notebook (o opencode de lá usa {env:ROUTER_API_KEY}).
  router_key="$(ssh -n -o BatchMode=yes "$host" "sed -n 's/^ROUTER_API_KEY=//p' ~/.config/opencode/server.env")"
  [ -n "$router_key" ] || { echo "sem ROUTER_API_KEY no notebook" >&2; exit 1; }
  router_models="$(curl -s -m10 -H "Authorization: Bearer $router_key" "http://127.0.0.1:$router_port/v1/models" \
    | python3 -c 'import json, sys; print("\n".join(m["id"] for m in json.load(sys.stdin).get("data", [])))')"
  [ -n "$router_models" ] || { echo "o 9router não listou modelos" >&2; exit 1; }
  echo "    $(printf '%s\n' "$router_models" | wc -l) modelos"
fi
mkdir -p "$HOME/.config/opencode"
# Arquivo gerado do zero: sem providers nem chaves (o opencode.json de cada perfil traz os
# seus; o MCP do ai-memory entra mais abaixo).
# shellcheck disable=SC2016 # o JSON é literal
printf '{\n  "$schema": "https://opencode.ai/config.json"\n}\n' > "$OC.novo"
mv "$OC.novo" "$OC"
rm -f "$OC".bak-*

echo "==> token do ai-memory (servidor local na VM)"
DATA_DIR="$(python3 - "$CONF" "$HOME" <<'PY'
import os, sys, tomllib
cfg = tomllib.load(open(sys.argv[1], "rb"))
home = sys.argv[2]
mem = cfg["memory"]
data = mem.get("data_dir", "~/.local/share/assistente/ai-memory")
if data.startswith("~/"):
    data = os.path.join(home, data[2:])
elif data == "~":
    data = home
print(data)
PY
)"
TOKEN_SRC="$DATA_DIR/auth-tokens/default"
[ -f "$TOKEN_SRC" ] || {
  echo "sem token em $TOKEN_SRC — rode bin/assistente vm provisionar (ai-memory init)" >&2
  exit 1
}
cp "$TOKEN_SRC" "$TOKEN_FILE.tmp"
mv "$TOKEN_FILE.tmp" "$TOKEN_FILE"
token="$(cat "$TOKEN_FILE")"
code="$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $token" "$URL/api/v1/projects")"
[ "$code" = 200 ] || {
  echo "servidor local recusou o token (HTTP $code). systemctl --user status ai-memory?" >&2
  exit 1
}

# Como pedir um comando fora do jail (F9.6). Vai para o CLAUDE.md do perfil (memória de usuário
# do Claude com CLAUDE_CONFIG_DIR) e para as `instructions` do opencode do perfil.
FORA_MD="$(cat <<'MD'
# Fora do jail (Assistente)

Você roda dentro do ai-jail, numa VM: Docker desligado, rede liberada só para alguns hosts e
home privada. Quando um comando falhar por causa disso (docker ou `./dev`, instalar pacote ou
acessar host bloqueado, `git push`/ssh, sudo), **não tente contornar o sandbox**. Peça para
rodá-lo fora do jail:

    "$ASSISTENTE_FORA_DO_JAIL" '<comando completo>'

- O usuário aprova ou nega no app do Assistente. A saída e o código de saída voltam como se o
  comando tivesse rodado aqui: na pasta atual (dentro do projeto), na VM, com a identidade git
  do perfil.
- Antes, diga em uma frase o que vai pedir e por quê. Um comando por pedido; nada destrutivo
  sem combinar.
- O pedido espera a resposta de uma pessoa: chame com timeout longo na ferramenta de shell
  (600000 ms). Se ela cortar antes, retome com `"$ASSISTENTE_FORA_DO_JAIL" --resultado <id>`
  (o id aparece na primeira linha do stderr).
- Código 126 = negado: não insista, pergunte o que fazer. 124 = ninguém respondeu a tempo ou o
  comando estourou o timeout (`--timeout SEG` muda o limite, padrão 600). 125 = pedido perdido:
  peça de novo.
- Sem `ASSISTENTE_FORA_DO_JAIL` no ambiente, a sessão é de antes desse recurso: peça ao
  usuário para rodar o comando na aba Terminal do app.

## Verificação do projeto (Assistente, F9.13)

O `./dev` (e o check do projeto) **roda no PC**, não na VM. Para rodar as verificações:

    "$ASSISTENTE_FORA_DO_JAIL" --pc --timeout 1500 './dev just check'

- Use `--pc` para executar no computador principal (host). Não peça `sudo` nem `systemctl start docker`.
- **Sem check verde, não diga que está pronto** e não valide a tarefa. O QA só aprova com o check
  passando.

## Quadro do projeto (Assistente, F11)

O quadro é `.assistente/kanban.json` no repo. Cada card tem um tipo: `manual` só roda quando o
usuário manda (botão "Mandar"); `agente` foi marcado para o agente implementar.

- `"$ASSISTENTE_FORA_DO_JAIL" kanban pull`: pega a próxima tarefa `agente` do backlog e a deixa
  presa a esta sessão. Sem tarefa, responde `{"card": null}`: pare, não invente trabalho.
  Termine a tarefa atual antes de puxar outra.
- `"$ASSISTENTE_FORA_DO_JAIL" kanban done <id> --commit <sha>`: conclui uma tarefa que você
  segura, com o commit que a fechou. `kanban release <id>` devolve ao backlog.
- Orquestrador (só sessão do Claude): para uma tarefa que você puxou, abra um trabalhador com
  `"$ASSISTENTE_FORA_DO_JAIL" kanban trabalhador <id>`. Sem `--modelos`, o hubd escolhe entre os
  modelos grátis do opencode, pela taxa de sucesso do perfil; com `--modelos a,b` você fixa a
  ordem. O trabalhador roda o opencode no worktree da tarefa (`.assistente/worktrees/<id>`, branch
  `agente/<id>`), troca de modelo na hora quando o provedor recusa e devolve a tarefa se todos
  falharem. Você acompanha pelas abas e não precisa reiniciar nada à mão.
- Trabalhador: não marque a tarefa nem edite o quadro; o `trabalhador` faz isso pelo resultado
  do git.

## Diagnóstico da VM (Assistente, F14)

Para ler o estado da VM **sem pedir aprovação** (a leitura não escreve nada):

    "$ASSISTENTE_FORA_DO_JAIL" diagnostico <comando>

- `journal-hubd [linhas]`: as últimas linhas do journal do hubd (padrão 200).
- `tmux-ls`: as sessões abertas.
- `tmux-pane <sessão>`: a saída de uma sessão deste projeto (as últimas 200 linhas).
- `explorar`: os prompts das sessões que montaram o quadro.
- `claude-login`: onde o `claude` está no PATH de login do perfil.

Qualquer outro comando é recusado. Prefira isto a pedir um comando fora do jail quando o que
você quer é só ler (o journal do hubd, a saída de uma sessão que morreu).
MD
)"
BEGIN_MARK="<!-- assistente:fora-do-jail (gerado por vm/configurar-agentes.sh) -->"
END_MARK="<!-- /assistente:fora-do-jail -->"

# perfil<TAB>raiz<TAB>workspace
profiles="$(python3 - "$CONF" <<'PY'
import sys, tomllib, os
for pid, p in tomllib.load(open(sys.argv[1], "rb"))["profiles"].items():
    print("\t".join([pid, os.path.expanduser(p["root"]), p["memory_workspace"]]))
PY
)"

while IFS=$'\t' read -r pid root workspace; do
  data="$HOME/.assistente/perfis/$pid"
  echo "==> perfil $pid"
  mkdir -p "$data/claude" "$data/opencode" "$data/ssh"
  ai-memory install-hooks --agent claude-code --server-url "$URL" --auth-token "$token" \
    --config-file "$data/claude/settings.json" --apply >/dev/null
  ai-memory install-mcp --client claude-code --server-url "$URL" --auth-token "$token" \
    --config-file "$data/claude/.claude.json" --apply >/dev/null
  chmod 600 "$data/claude/settings.json" "$data/claude/.claude.json"
  # Permissões padrão por perfil (F3.3): allow/deny/ask do Claude.
  ai-jail --claude-dir "$data/claude" --bootstrap >/dev/null
  # Bloco gerenciado no CLAUDE.md do perfil: o resto do arquivo é seu e fica intacto.
  FORA_MD="$FORA_MD" BEGIN_MARK="$BEGIN_MARK" END_MARK="$END_MARK" \
    python3 - "$data/claude/CLAUDE.md" <<'PY'
import os, pathlib, re, sys
path = pathlib.Path(sys.argv[1])
begin, end = os.environ["BEGIN_MARK"], os.environ["END_MARK"]
block = f"{begin}\n{os.environ['FORA_MD'].strip()}\n{end}\n"
text = path.read_text() if path.exists() else ""
pattern = re.compile(re.escape(begin) + r".*?" + re.escape(end) + r"\n?", re.S)
if pattern.search(text):
    text = pattern.sub(lambda _: block, text)
else:
    text = (text.rstrip() + "\n\n" if text.strip() else "") + block
path.write_text(text)
PY
  printf '%s\n' "$FORA_MD" > "$data/opencode/fora-do-jail.md"
  # O opencode mescla a config global com esta (gerada aqui; sempre regravada). Os providers
  # vêm do opencode_providers do perfil (F3.4): o 9router é montado com a chave/modelos do
  # notebook e os do PC copiados como estão. As policies do PC vão junto: o tab de modelos
  # fica igual ao do PC. O arquivo carrega chaves de API: fica 0600.
  ROUTER_KEY="$router_key" ROUTER_PORT="$router_port" ROUTER_MODELS="$router_models" \
    HOST_PROVIDERS="$host_providers" HOST_POLICIES="$host_policies" \
    python3 - "$CONF" "$pid" "$data/opencode/opencode.json" "$data/opencode/fora-do-jail.md" <<'PY'
import json, os, sys, tomllib
pid = sys.argv[2]
profile = tomllib.load(open(sys.argv[1], "rb"))["profiles"][pid]
# Como pedir comandos fora do jail (F9.6); a pasta do perfil entra no jail (`--rw-map`).
config = {"$schema": "https://opencode.ai/config.json", "instructions": [sys.argv[4]],
          "permission": {"bash": "allow", "edit": "allow", "write": "allow"}}
for key, field in (("model", "opencode_model"), ("small_model", "opencode_small_model")):
    if profile.get(field):
        config[key] = profile[field]
# Um "9router" que exista no PC não substitui o do notebook.
host = {k: v for k, v in json.loads(os.environ.get("HOST_PROVIDERS") or "{}").items() if k != "9router"}
router_models = [m for m in os.environ.get("ROUTER_MODELS", "").splitlines() if m]
providers = {}
for name in profile.get("opencode_providers", ["9router"]):
    if name == "9router":
        if not router_models:
            sys.exit(f"perfil {pid}: 9router em opencode_providers, mas o notebook não listou modelos")
        providers[name] = {
            "npm": "@ai-sdk/openai-compatible",
            "name": "9router (notebook)",
            "options": {"baseURL": "http://127.0.0.1:%s/v1" % os.environ["ROUTER_PORT"],
                        "apiKey": os.environ["ROUTER_KEY"]},
            "models": {m: {"name": m} for m in router_models},
        }
    elif name in host:
        providers[name] = host[name]
    else:
        known = ", ".join(sorted(["9router", *host]))
        sys.exit(f'perfil {pid}: opencode_providers tem "{name}" (disponíveis: {known})')
policies = json.loads(os.environ.get("HOST_POLICIES") or "[]")
if providers:
    config["provider"] = providers
if policies:
    config["policies"] = policies
with open(sys.argv[3], "w") as f:
    json.dump(config, f, indent=2)
    f.write("\n")
models = " · ".join(config[k] for k in ("model", "small_model") if k in config)
print(
    "    opencode: %s%s%s"
    % (
        ", ".join(providers) or "sem providers",
        f" (modelo {models})" if models else "",
        f" · policies: {len(policies)}" if policies else "",
    )
)
PY
  chmod 600 "$data/opencode/opencode.json"
  # Marcador do workspace na raiz do perfil (F5.3): pessoal e trabalho não se misturam.
  if [ -d "$root" ] && [ ! -f "$root/.ai-memory.toml" ]; then
    printf '# Criado pelo Assistente (vm/configurar-agentes.sh): todo repo abaixo daqui é deste workspace.\nworkspace = "%s"\n' "$workspace" > "$root/.ai-memory.toml"
    echo "    marcador: $root/.ai-memory.toml (workspace = $workspace)"
  fi
done <<< "$profiles"

echo "==> opencode (global: plugin de hooks + MCP)"
ai-memory install-hooks --agent open-code --server-url "$URL" --auth-token "$token" --apply >/dev/null
ai-memory install-mcp --client opencode --server-url "$URL" --auth-token "$token" --apply >/dev/null
# As policies do PC também entram no global da VM: o tab fica igual ao do PC até fora do
# perfil (o global continua sem chaves de API). Depois do ai-memory, que regrava o arquivo.
HOST_POLICIES="$host_policies" python3 - "$OC" <<'PY'
import json, os, sys
policies = json.loads(os.environ.get("HOST_POLICIES") or "[]")
if not policies:
    sys.exit(0)
config = json.load(open(sys.argv[1]))
config["policies"] = policies
with open(sys.argv[1], "w") as f:
    json.dump(config, f, indent=2)
    f.write("\n")
PY
# Permissões padrão no opencode global (F3.3): bash/edit/write.
ai-jail --bootstrap >/dev/null
chmod 600 "$OC"
rm -f "$OC".bak-*

echo "Agentes configurados."
