#!/usr/bin/env bash
# Configura os agentes da VM: 9router (túnel), providers do PC, ai-memory (hoje via túnel;
# F5.0: servidor local). Claude Code e opencode por perfil. Roda como o
# SEU usuário dentro da VM; reexecutável. Chamado por `bin/assistente agentes configurar`, que
# manda os providers do PC (JSON, com as chaves) pelo stdin.
# Specs: docs/specs/04-perfis-e-projetos.md, docs/specs/06-memoria.md (F5.2, F5.3)
set -euo pipefail

host_providers="{}"
[ -t 0 ] || host_providers="$(cat)"
[ -n "$host_providers" ] || host_providers="{}"

URL="http://127.0.0.1:49374"
CONF="$HOME/.config/assistente/assistente.toml"
TOKEN_FILE="$HOME/.assistente/ai-memory.token"
export PATH="$HOME/.local/bin:$PATH"

umask 077
host="$(python3 -c "import tomllib;print(tomllib.load(open('$CONF','rb'))['notebook']['ssh_host'])")"
router_port="$(python3 -c "import tomllib;print(tomllib.load(open('$CONF','rb'))['notebook']['tunnel']['9router'])")"
OC="$HOME/.config/opencode/opencode.json"

echo "==> opencode: provider 9router (modelos lidos do próprio 9router) e providers do PC"
# A chave fica no server.env do notebook (o opencode de lá usa {env:ROUTER_API_KEY}).
router_key="$(ssh -n -o BatchMode=yes "$host" "sed -n 's/^ROUTER_API_KEY=//p' ~/.config/opencode/server.env")"
[ -n "$router_key" ] || { echo "sem ROUTER_API_KEY no notebook" >&2; exit 1; }
mkdir -p "$HOME/.config/opencode"
# shellcheck disable=SC2016 # o python abaixo é literal
curl -s -m10 -H "Authorization: Bearer $router_key" "http://127.0.0.1:$router_port/v1/models" \
  | ROUTER_KEY="$router_key" ROUTER_PORT="$router_port" HOST_PROVIDERS="$host_providers" python3 -c '
import json, os, sys
ids = [m["id"] for m in json.load(sys.stdin).get("data", [])]
if not ids:
    sys.exit("o 9router não listou modelos")
config = {
    "$schema": "https://opencode.ai/config.json",
    "model": "9router/" + ("free" if "free" in ids else ids[0]),
    "provider": {"9router": {
        "npm": "@ai-sdk/openai-compatible",
        "name": "9router (notebook)",
        "options": {"baseURL": "http://127.0.0.1:%s/v1" % os.environ["ROUTER_PORT"],
                    "apiKey": os.environ["ROUTER_KEY"]},
        "models": {i: {"name": i} for i in ids},
    }},
}
# Providers do PC entram como estão (baseURL, chave, modelos); o 9router daqui prevalece.
host = {k: v for k, v in json.loads(os.environ["HOST_PROVIDERS"]).items() if k != "9router"}
config["provider"].update(host)
print(json.dumps(config, indent=2))
print("    9router: %d modelos" % len(ids), file=sys.stderr)
print("    do PC: %s" % (", ".join(host) or "nenhum"), file=sys.stderr)
' > "$OC.novo"
# Arquivo gerado do zero: só o 9router e os providers do PC (o MCP entra logo abaixo).
mv "$OC.novo" "$OC"
rm -f "$OC".bak-*

echo "==> token do ai-memory (lido do notebook)"
ssh -n -o BatchMode=yes "$host" 'cat ~/.local/share/ai-memory/auth-tokens/default' > "$TOKEN_FILE.tmp"
mv "$TOKEN_FILE.tmp" "$TOKEN_FILE"
token="$(cat "$TOKEN_FILE")"
code="$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $token" "$URL/api/v1/projects")"
[ "$code" = 200 ] || { echo "servidor recusou o token (HTTP $code). O túnel está de pé?" >&2; exit 1; }

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
  mkdir -p "$data/claude" "$data/opencode"
  ai-memory install-hooks --agent claude-code --server-url "$URL" --auth-token "$token" \
    --config-file "$data/claude/settings.json" --apply >/dev/null
  ai-memory install-mcp --client claude-code --server-url "$URL" --auth-token "$token" \
    --config-file "$data/claude/.claude.json" --apply >/dev/null
  chmod 600 "$data/claude/settings.json" "$data/claude/.claude.json"
  # O opencode mescla a config global com esta (gerada aqui; sempre regravada).
  python3 - "$CONF" "$pid" "$data/opencode/opencode.json" <<'PY'
import json, sys, tomllib
profile = tomllib.load(open(sys.argv[1], "rb"))["profiles"][sys.argv[2]]
config = {"$schema": "https://opencode.ai/config.json"}
for key, field in (("model", "opencode_model"), ("small_model", "opencode_small_model")):
    if profile.get(field):
        config[key] = profile[field]
if len(config) > 1:
    with open(sys.argv[3], "w") as f:
        json.dump(config, f, indent=2)
        f.write("\n")
    print("    opencode: %s" % " · ".join(config[k] for k in ("model", "small_model") if k in config))
PY
  # Marcador do workspace na raiz do perfil (F5.3): pessoal e trabalho não se misturam.
  if [ -d "$root" ] && [ ! -f "$root/.ai-memory.toml" ]; then
    printf '# Criado pelo Assistente (vm/configurar-agentes.sh): todo repo abaixo daqui é deste workspace.\nworkspace = "%s"\n' "$workspace" > "$root/.ai-memory.toml"
    echo "    marcador: $root/.ai-memory.toml (workspace = $workspace)"
  fi
done <<< "$profiles"

echo "==> opencode (global: plugin de hooks + MCP)"
ai-memory install-hooks --agent open-code --server-url "$URL" --auth-token "$token" --apply >/dev/null
ai-memory install-mcp --client opencode --server-url "$URL" --auth-token "$token" --apply >/dev/null
chmod 600 "$OC"
rm -f "$OC".bak-*

echo "Agentes configurados."
