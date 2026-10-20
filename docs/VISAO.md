# Visão e requisitos

O **Assistente** é um hub pessoal para trabalhar com agentes de IA: um app desktop bonito
que abre sessões do Claude Code e do OpenCode em projetos pessoais e de trabalho, cada uma
isolada numa VM e num sandbox, com memória compartilhada entre sessões e máquinas, e que à
noite entrega o trabalho em andamento ao notebook para seguir desenvolvendo sozinho.

Hoje o uso é espalhado: Claude, Cursor e OpenCode, cada um com sua config, sem memória
comum, rodando direto no sistema. O 9router (gateway de modelos) mora no notebook.

**Regra: tudo roda no PC principal** (host + VM). O notebook só cuida de duas coisas: o
**9router** e o **modo hora de dormir** (R6), incluindo a réplica do wiki que o dormir usa. Com
o notebook desligado, toda funcionalidade do dia a dia segue funcionando, só sem o 9router.
Funcionalidade nova não pode depender do notebook fora desses dois casos.

## Requisitos

Cada requisito aponta para a funcionalidade (docs/FUNCIONALIDADES.md) e a spec que o cobrem.

| ID | Requisito (das anotações) | Critério de aceite | Funcionalidades | Spec |
|---|---|---|---|---|
| R1 | Interface maneira | App desktop com tema escuro, perfis por cor, projetos na lateral, sessões em abas com terminal real, barra de status com VM/notebook/9router/memória sempre visíveis | F0.4, F2.4, F2.5 | [03](specs/03-interface.md) |
| R2 | Conexão com o notebook para usar o 9router de lá | Dentro da VM, `http://127.0.0.1:20128/v1` responde pelo 9router do notebook; cai e volta sozinho | F4.1–F4.3 | [07](specs/07-notebook-e-9router.md) |
| R3 | Rodar em container ou VM (melhor VM) | Agentes rodam numa VM Incus; o host só vê as raízes dos perfis e a config | F1.1–F1.5 | [01](specs/01-vm-e-instalacao.md) |
| R4 | AI Memory (ai-memory 2.0) | Servidor principal na VM/PC (`mirror_dir` no host); sessões com briefing e captura sem notebook; réplica no notebook para dormir | F5.0–F5.6 | [06](specs/06-memoria.md) |
| R5 | Jail (ai-jail) | Toda sessão de agente roda dentro do ai-jail; uma sessão não vê credenciais de outro perfil | F3.1–F3.3 | [05](specs/05-sandbox.md) |
| R6 | Modo "hora de dormir" | Um clique entrega o projeto atual ao notebook, que trabalha no backlog com opencode; de manhã, um clique traz o resultado para revisar | F6.1–F6.6 | [08](specs/08-modo-dormir.md) |
| R7 | Suportar Claude e OpenCode | Os dois agentes abrem em qualquer projeto, com config/login por perfil | F2.2, F3.2 | [02](specs/02-hubd-e-sessoes.md) |
| R8 | Separação pessoal × trabalho | Raiz, git, SSH, login do Claude, config do opencode, workspace de memória e combo do 9router separados por perfil | F2.1, F3.2, F5.3 | [04](specs/04-perfis-e-projetos.md) |
| R9 | Fácil de reinstalar ao formatar | Do PC zerado ao app funcionando com `git clone` + `./instalar.sh backup.tar.gz` + logins | F1.1, F7.1–F7.3 | [01](specs/01-vm-e-instalacao.md) |
| R10 | O Assistente se atualiza sem fechar | Numa aba fixa, um agente edita o próprio repo; um clique aplica o commit (UI e hubd) sem fechar o app nem derrubar sessões, com volta para a versão anterior | F8.1–F8.7 | [09](specs/09-auto-atualizacao.md) |

## Fora do escopo (por enquanto)

- Cursor dentro do hub: ele continua no host, e o hub só abre o projeto nele (F2.6).
- Chat próprio em cima de SDKs: as sessões são os TUIs reais dos agentes num terminal.
- Acesso pelo celular, notificações e mais agentes (codex etc.): ficam em "Depois".
