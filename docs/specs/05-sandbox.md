# 05 — Sandbox (ai-jail)

Cobre R5. Funcionalidades F3.x. Ferramenta: <https://github.com/akitaonrails/ai-jail>.

## Camadas

```
host (Pop!_OS)
 └─ VM Incus ............ fronteira principal: kernel próprio, só vê as raízes montadas
     └─ ai-jail ......... bubblewrap + Landlock: só o projeto (rw) e dotdirs de ferramentas
         └─ claude | opencode
```

Por que as duas: a VM protege o host (e o resto do seu home) mesmo que o agente ou uma
dependência escape do ai-jail; o ai-jail protege o resto da VM, inclusive as credenciais dos
outros perfis, e mantém o comportamento igual ao do notebook.

## Como a sessão é lançada (ai-jail 2.10, validado em 2026-10-19)

O ai-jail 2.x é bem mais restrito que o do artigo (v0.4): **home privada** (tmpfs; só os
dados do próprio agente entram), **rede filtrada** (proxy com lista de hosts: api.anthropic.com
no preset do claude, registries de pacotes, github), **ambiente limpo** e **Docker desligado**.
O `LaunchPlan` (`hub_core::session::jail_argv`) monta:

| Flag | Por quê |
|---|---|
| `--rw-map ~/.assistente/perfis/<perfil>` | dados do perfil (opencode, chave SSH); só o perfil ativo |
| `--claude-dir …/<perfil>/claude` (só claude) | `CLAUDE_CONFIG_DIR` do perfil: login, settings, hooks e MCP próprios |
| `--forward-port 20128` | o túnel do notebook (9router) em 127.0.0.1; sem isso o proxy responde 405 |
| `--forward-port 49374` (`memory.port`) | o sandbox tem loopback próprio: sem o relay, hooks/MCP não alcançam o ai-memory local da VM (F5.0) e a captura cai no spool para sempre |
| `--allow-host <host>` (só opencode) | `opencode_allow_hosts` do perfil: API de provedores fora do 9router. Soma à lista do preset (validado: o `registry.npmjs.org` continua liberado) |
| `--map ~/.local/lib/ai-memory` | binário que os hooks chamam |
| `--rw-map ~/.local/share/ai-memory` | spool local dos hooks (notebook fora do ar) |
| `--map <raiz do perfil>/.ai-memory.toml` | marcador do workspace; sem ele a memória cai em `default` |
| `--terminal-passthrough` | o proxy de terminal do ai-jail descarta o OSC 52: sem isso copiar com o mouse no claude/opencode não chega ao app. O agente passa a escrever direto no painel do tmux, que só repassa ao app o OSC 52 de escrita |
| `--env NOME` (uma por variável do perfil) | o valor vem do env do tmux, não aparece na linha de comando |
| `--docker` | engrenagem do projeto (F9.2) ou `[jail] docker` do `.assistente.toml` |
| `--allow-host <host>` (os dois agentes) | `allow_hosts` da engrenagem do projeto (F9.2) |
| `--forward-port 7879` (`hub.agent_listen`) | API dos agentes (F9.5): o agente pede comandos fora do jail. A 7878 do app continua fora |
| `--map ~/.local/lib/assistente` | cliente `fora-do-jail` (F9.6), só leitura |
| `--network` | `full_network` da engrenagem: rede sem filtro. O ai-jail não aceita junto com `--forward-port`, então os relays e os `--allow-host` saem |
| `-- claude` / `-- opencode --standalone` | o preset vale com ou sem `--`. O opencode 2 é cliente de um servidor em segundo plano compartilhado, que não sobrevive ao jail: `--standalone` sobe um por sessão |

O preset do opencode monta `~/.config/opencode` e `~/.local/share/opencode`, que são
**compartilhados entre perfis**. Desde F3.4 o config global não carrega chaves de API: os
providers (com as chaves) ficam no `OPENCODE_CONFIG` de cada perfil, e o global traz só MCP,
plugins e node_modules. Isolar o histórico por perfil (`XDG_DATA_HOME`) fica para depois.

## Docker dentro da sessão

No 2.x o Docker vem **desligado** (`--docker` liga; equivale a root na VM). Projetos com
`./dev` (como o game) precisam dele para o agente rodar `just check`: ligue por projeto na
engrenagem (F9.2) ou, legado, com `[jail] docker = true` no `.assistente.toml` (F3.1), nunca
globalmente.

## `.ai-jail` no projeto (F0.5)

O ai-jail grava um `.ai-jail` na raiz do projeto na primeira execução e, dentro do sandbox,
o **mascara**: o agente vê um arquivo vazio. Versionado, ele aparece sempre modificado para o
agente, que acaba commitando a versão vazia (foi o churn de `8e886f3`). Solto, suja a árvore:
o `POST /api/sleep` responde 409 (e o WIP o commitaria) e o `dorme` para na primeira tarefa.

Por isso o arquivo nunca entra no git:

- este repo: `/.ai-jail` no `.gitignore`;
- todos os projetos: o hubd põe `/.ai-jail` no `info/exclude` do repo
  (`sleep::exclude_jail_config`) ao abrir sessão e antes de dormir, e o sync põe o mesmo
  padrão no clone do notebook. O exclude vale por clone, não é commitado e o jail o enxerga,
  porque o `.git` fica dentro do projeto. Arquivo já rastreado não é afetado: num repo que
  versiona o `.ai-jail`, falta um `git rm --cached .ai-jail`.

O conteúdo fora do jail varia com o último agente (`command = ["claude"]` ou `["opencode"]`)
e não importa: o `LaunchPlan` passa tudo por flag. Segundo o README do ai-jail, o `.ai-jail`
do projeto não é confiável e só pode restringir o sandbox, nunca afrouxar.

## Fora do jail (F9)

O que o jail bloqueia (Docker, hosts fora da lista, `git push` por ssh, `sudo`) tem três saídas,
nenhuma no PC: a VM continua sendo a fronteira.

- **Aba Terminal (F9.1).** `bash -l` no tmux, sem ai-jail, na pasta do projeto e com o env do
  perfil (identidade git, `GIT_SSH_COMMAND`). Equivale a você digitando na VM.
- **Configurações do projeto (F9.2–F9.3).** Engrenagem na lista de projetos: Docker, hosts
  liberados, rede completa e comandos sempre permitidos. Ficam em `~/.assistente/projetos.toml`
  na VM, fora do projeto e de todo mount do jail (teste `settings_file_is_outside_every_jail_mount`):
  o agente não se dá permissão. **Fraqueza conhecida:** o `[jail] docker` do `.assistente.toml`
  continua valendo por compatibilidade, e esse arquivo o agente pode editar (vale na sessão
  seguinte). Projetos novos: use a engrenagem.
- **Pedido do agente com aprovação (F9.5–F9.7).** O agente roda
  `"$ASSISTENTE_FORA_DO_JAIL" '<comando>'` (instruções no `CLAUDE.md` do perfil e nas
  `instructions` do opencode, gravadas pelo `configurar-agentes.sh`). O pedido aparece no app,
  você aprova e o hubd roda o comando na VM, fora do jail, na pasta atual do agente e com o env
  do perfil. A saída e o código voltam ao agente (126 = negado). API e limites: spec 02.
- **Diagnóstico sem aprovação (F14.1).** Para **ler** o estado da VM, o agente usa
  `"$ASSISTENTE_FORA_DO_JAIL" diagnostico <comando>`: o hubd roda uma leitura de uma lista
  fechada (journal do hubd, sessões, painel de uma sessão, prompts do quadro, `claude` no PATH)
  e devolve a saída. Como nada é escrito, isso não passa pelo app — o caminho é `POST
  /v1/diagnostico` na API dos agentes (spec 02). Comando fora da lista é recusado com 400.

### Segurança do "fora do jail"

- **O que o jail alcança:** só a API dos agentes (`agent_listen`), num roteador separado: pedir
  e ler o próprio pedido. Aprovar exige o token do app, que fica em `~/.config/assistente`
  (fora do jail).
- **Quem pode pedir:** qualquer processo do jail com o env (um `postinstall` do npm, por
  exemplo). A defesa é a aprovação humana com o comando inteiro na tela; limites de pendentes
  e expiração evitam enxurrada.
- **"Sempre permitir" é casamento exato**, e é uma fuga quando o comando roda arquivos do
  projeto (`./dev`, `make`, `npm run`, scripts): o agente edita o arquivo e roda o que quiser.
  A UI avisa antes de gravar.
- **O comando aprovado roda como o seu usuário na VM:** enxerga os dados de todos os perfis
  (como a aba Terminal). Nunca roda no PC.
- **Rede completa** tira os `--forward-port`; se o `--network` compartilhar a rede da VM, o
  agente alcança a 7878 (exige token), a 7879 e a LAN. A validar (F9.8).

## Permissões dos agentes (F3.3)

`ai-jail --bootstrap` gera, por perfil (com `CLAUDE_CONFIG_DIR`/`OPENCODE_CONFIG` do perfil):

- Claude: permite git status/diff/log, cargo, npm, python; nega `rm -rf`, `sudo`,
  `chmod 777`, `git push --force`; pergunta em `git push`, `rm`, `docker run`.
- OpenCode: permissões de bash/edit/write.

No notebook (modo dormir), o `opencode run --auto` roda dentro do ai-jail do mesmo jeito
(`AGENTE_JAIL`, padrão `ai-jail`).

## Pontos a validar

- Hooks do ai-memory dentro do ai-jail: o binário (`~/.local/bin`?) e o token precisam estar
  visíveis; talvez um `ro_map` do diretório de dados do ai-memory.
- `CLAUDE_CONFIG_DIR` fora de `~/.claude` funciona com o instalador nativo e com os hooks.
- Landlock no kernel da imagem cloud do Ubuntu 24.04 (ABI V3+).
- F9.8: de dentro do jail, `127.0.0.1:7879` responde e `127.0.0.1:7878`/`10.77.0.10:7878` não;
  `~/.assistente/projetos.toml`, `hub.token` e o socket do tmux (`/tmp/tmux-<uid>`) não
  aparecem (senão `tmux send-keys` seria uma fuga); o Claude lê o `CLAUDE.md` do
  `CLAUDE_CONFIG_DIR` e respeita o `BASH_MAX_TIMEOUT_MS`.
