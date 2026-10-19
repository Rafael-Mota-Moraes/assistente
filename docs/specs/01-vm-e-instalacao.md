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
  | `memory.mirror_dir` | rw | espelho do wiki do ai-memory |

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
| Memória | notebook (principal) + espelho | já está no notebook; o espelho vem no backup |
| Logins dos agentes na VM | `~/.assistente/perfis` (VM) | backup (F7.1) ou `assistente login` de novo |
| Projetos | suas pastas / GitHub | seu backup normal |

Roteiro:

1. Instale o Docker.
2. `git clone … && cd assistente`
3. `./instalar.sh ~/assistente-backup-AAAA-MM-DD.tar.gz` (na primeira vez ele pede logout para
   o grupo `incus-admin`; rode de novo depois).
4. `bin/assistente login claude pessoal` (e os outros perfis/agentes).
5. Abra o app.

## Pontos a validar

- Pacotes do Incus para VM no Pop!_OS 24.04 (`incus`, `qemu-system-x86`, `ovmf`) e se o
  `incus admin init --minimal` conflita com a rede própria.
- virtiofs com UID igual sem `shift=true`.
- `incus config device override eth0 ipv4.address=…` com a NIC vinda da rede `assistente0`.
