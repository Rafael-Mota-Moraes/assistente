# 01 — VM e instalação

Cobre R3 (VM) e R9 (reinstalar fácil). Funcionalidades F1.x e F7.x.

## Peças

| Arquivo | Papel |
|---|---|
| `instalar.sh` | Bootstrap do PC: Incus/QEMU, config, token, VM, app `.deb` |
| `bin/assistente` | CLI do host: `vm criar|iniciar|parar|shell|recriar|status`, `login`, `token`, `backup`, `restaurar`, `notebook instalar` |
| `vm/cloud-init.yaml` | Primeira inicialização: usuário com UID do host, pacotes (git, tmux, bubblewrap, docker, autossh) |
| `vm/provisionar.sh` | Reexecutável, como root na VM: ai-jail, claude, opencode, ai-memory (cliente), pastas de perfil, hubd |
| `vm/hubd.service` | systemd do hubd, como o usuário |

## A VM

- **Incus**, VM KVM (`--vm`), imagem `images:ubuntu/24.04/cloud`, CPUs/memória/disco de `[vm]`.
- **Rede própria** `assistente0` (10.77.0.1/24, NAT, sem IPv6) e **IP fixo** `vm.ip` (10.77.0.10).
  O app acha o hubd em `http://<vm.ip>:7878`, e ninguém na LAN alcança a VM.
- **Usuário igual ao do host** (nome e UID): arquivos criados na VM ficam com o seu dono, e
  os caminhos são os mesmos dos dois lados.
- **Montagens virtiofs** (`incus config device add … disk`), todas no mesmo caminho do host:

  | Origem (host) | Modo | Para quê |
  |---|---|---|
  | raiz de cada perfil | rw | projetos |
  | `~/.config/assistente` | ro | config + token, lidos pelo hubd |
  | `memory.mirror_dir` | rw | wiki git principal do ai-memory (spec 06) |

- **Não montado**: o resto do home do host (chaves SSH, navegador, etc.). Dentro da VM, as
  credenciais ficam em `~/.assistente/perfis/<perfil>/` (ver spec 05).
- **Docker dentro da VM**, para os projetos que usam `./dev` (como o game).

## Ciclo de vida

```
assistente vm criar     rede → init com cloud-init → devices → start → espera cloud-init
                        → ./dev just build-hubd → push hubd + provisionar.sh → provisiona
assistente vm recriar   confirma, apaga e cria (F7.3: restaura logins do backup)
assistente vm shell     shell do seu usuário na VM
assistente login claude <perfil>   claude /login com CLAUDE_CONFIG_DIR do perfil
```

Atualizar o hubd sem recriar a VM: `./dev just build-hubd`, `incus file push` e
`systemctl restart hubd` (vira `assistente vm atualizar` na F1.4).

## Reinstalar depois de formatar (R9)

O que sobrevive à formatação e onde fica:

| Coisa | Onde mora | Como volta |
|---|---|---|
| Código do Assistente | GitHub | `git clone` |
| Config + token | `~/.config/assistente` | `backup.tar.gz` → `instalar.sh backup.tar.gz` |
| Memória | wiki em `memory.mirror_dir` (host) + servidor na VM (F5.0) | backup inclui espelho; réplica no notebook (F5.4) |
| Logins dos agentes na VM | `~/.assistente/perfis` (VM) | backup (F7.1) ou `assistente login` de novo |
| Projetos | suas pastas / GitHub | seu backup normal |

Roteiro:

1. Instale o Docker.
2. `git clone … && cd assistente`
3. `./instalar.sh ~/assistente-backup-AAAA-MM-DD.tar.gz` (na primeira vez ele pede logout para
   o grupo `incus-admin`; rode de novo depois).
4. `bin/assistente login claude pessoal` (e os outros perfis/agentes).
5. Abra o app.

## Validado em 2026-10-19 (Pop!_OS 24.04, Incus 6.0.0)

- `qemu-system-x86` e `ovmf` já vinham instalados; só faltou o pacote `incus`.
- virtiofs com UID/GID iguais funciona **sem** `shift`: arquivo criado na VM aparece `1000:1000` no host.
- A rede `assistente0` + `ipv4.address` fixo funciona ao lado do `incusbr0` do `init --minimal`.
- O Docker do host não bloqueou o encaminhamento: a VM tem internet pelo NAT.
- Sem logout: `sg incus-admin -c "bin/assistente …"` serve até o próximo login.

Pegadinhas encontradas (já tratadas nos scripts):

- `incus exec` logo após `start` falha ("VM agent isn't currently running"): `vm_esperar_agente`.
- `cloud-init status --wait` sai com 2 para avisos de schema; tratado como sucesso com aviso.
- `ssh_authorized_keys: []` é inválido no schema do cloud-init.
- Sem `bootcmd` com o GID do host, o grupo `docker` pega o GID 1000.
- Dentro de `sg incus-admin`, `id -g` devolve o GID do incus-admin: usar `id -g "$(id -un)"`.
- O Incus cria `/home/<usuario>` como root (por causa dos pontos de montagem): o
  `provisionar.sh` corrige o dono e copia o `/etc/skel`.
- O opencode fica em `~/.opencode/bin`, com link em `~/.local/bin` e `PATH` explícito no
  `hubd.service`. O instalador do opencode.ai só instala o 1.x; o 2.x sai no npm como
  `@opencode/cli`, e o `provisionar.sh` baixa o binário do pacote `@opencode/cli-linux-x64`
  (versão fixa, sha512 conferido), sem precisar de Node na VM. Trocou a versão? Rode
  `assistente agentes configurar`: o `ai-memory install-hooks` gera o plugin de hooks para a
  API da versão instalada (o do 1.x não carrega no 2.x: "1 plugin failed").
- Versões instaladas (constantes no topo de `vm/provisionar.sh`, F1.3): Claude Code 2.1.303,
  opencode 2.0.23, ai-jail 2.10.0 (Landlock "fully enforced" na VM), ai-memory **3.1.1**
  (as specs foram escritas olhando a 2.0). O script é reexecutável: se a versão já está no PATH,
  pula o download; senão instala (ai-jail com sha256 do release, opencode com sha512 do npm,
  ai-memory via `install.sh` da release, Claude via `install.sh` com número de versão).
- Ubuntu 24.04 na VM restringe user namespace não privilegiada via AppArmor
  (`kernel.apparmor_restrict_unprivileged_userns=1`): o bwrap cai no perfil
  `unprivileged_userns` ("deny capability") e as sessões morrem com "bwrap: loopback:
  Failed RTM_NEWADDR: Operation not permitted". A política só carrega no boot — funciona
  até o primeiro reinício. O `provisionar.sh` instala `/etc/apparmor.d/bwrap` (perfil
  `flags=(unconfined)` com `userns,`, correção documentada pelo upstream do ai-jail,
  issue #34); o resto da política continua valendo.
- O bind da wiki do ai-memory (`provisionar.sh`) precisa de `After=/Requires=incus-agent.service`:
  o agent monta o virtiofs do espelho ~0,4s depois do systemd tentar o bind no boot. Sem a
  ordem, o bind captura a pasta vazia por baixo do ponto de montagem e o ai-memory crasha
  com "Permission denied" em `wiki/.git` (a pasta raiz do espelho fica com dono root).
