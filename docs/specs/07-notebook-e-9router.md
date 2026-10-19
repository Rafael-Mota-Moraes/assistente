# 07 — Notebook e 9router

Cobre R2. Funcionalidades F4.x.

## Papel do notebook (Linux Mint, sempre ligado)

| Serviço | Endereço no notebook | Como o PC usa |
|---|---|---|
| 9router | `127.0.0.1:20128` (já existe, dashboard em `/dashboard`) | túnel → `127.0.0.1:20128` na VM |
| ai-memory | `127.0.0.1:49374` | túnel → `127.0.0.1:49374` na VM |
| sshd | `:22` (LAN ou Tailscale) | túnel, git e controle do modo dormir |
| opencode + ai-jail + `dorme` | — | modo dormir (spec 08) |

O 9router é um gateway OpenAI-compatível com combos (ex.: `dev-maneirao`, `testes`) e
fallback entre provedores. O `opencode.json` de hoje usa
`baseURL: http://127.0.0.1:20128/v1`. Com o túnel, **a mesma config funciona dentro da VM**.

## Setup (F4.1)

`bin/assistente notebook instalar` copia `notebook/` e roda `notebook/instalar.sh` por SSH:
pacotes (openssh-server, bubblewrap, tmux, git), ai-jail, ai-memory (servidor + opencode),
pastas `~/assistente/{projetos,logs,briefings}`, `dorme` em `~/.local/bin`; no fim confere o
9router e o ai-memory e mostra a chave pública da VM para autorizar.

Pré-requisito: um alias no `~/.ssh/config` **da VM** (o hubd roda lá):

```
Host notebook
  HostName 192.168.0.50        # ou o nome no Tailscale (recomendado fora de casa)
  User <seu-usuario-no-notebook>
  IdentityFile ~/.ssh/id_ed25519
  ServerAliveInterval 15
  ServerAliveCountMax 3
```

## Túnel gerenciado pelo hubd (F4.2)

- Processo filho: `ssh -N -o ExitOnForwardFailure=yes -L 20128:127.0.0.1:20128 -L
  49374:127.0.0.1:49374 notebook` (portas vindas de `[notebook.tunnel]`).
- Reinício com backoff exponencial (1 s → 30 s); estado `connecting|up|down` + último erro.
- `/api/notebook` passa a ter `tunnel_managed: true` e o estado do SSH; os serviços continuam
  sendo testados com connect TCP em 127.0.0.1 (já implementado).
- **Porta ocupada**: no PC de hoje o próprio `opencode` escuta em `127.0.0.1:49374` (a porta
  padrão do ai-memory), visto em 2026-10-19 com `ss -ltnp`. Se isso acontecer na VM, o `-L` falha
  (`ExitOnForwardFailure`) e o hubd precisa dizer "porta 49374 ocupada por <processo>", não
  só "caído". Se for frequente, troque a porta local em `[notebook.tunnel]` (a remota continua igual).
- **Saúde de verdade (F4.3)**: o connect TCP de hoje dá falso positivo quando outro processo
  ocupa a porta. Trocar por HTTP: `GET /v1/models` no 9router e `GET /` no ai-memory.
- Alternativa avaliada: `autossh` (instalado na VM) como unidade systemd separada. Mais simples,
  mas o hubd perde o estado fino; fica como plano B.

## Estado real do notebook (2026-10-19)

- Linux Mint 22.3, usuário `rafa`, IP 192.168.0.200 (`[notebook] address/user` na config).
- **9router roda em Docker** (`decolua/9router:latest`) com a porta publicada em
  `0.0.0.0:20128`, ou seja, **exposto na LAN**. O túnel funciona igual; para fechar, publique
  como `127.0.0.1:20128:20128` no container (decisão do usuário, pendente).
- Já existe um `opencode web --hostname 0.0.0.0 --port 4096` (serviço `opencode.service`,
  com senha em `~/.config/opencode/server.env`). Não é usado pelo Assistente.
- O `opencode` fica em `~/.opencode/bin` (fora do PATH de shells não interativos);
  `notebook/instalar.sh` e o `dorme` precisam desse PATH.
- `loginctl enable-linger rafa` ligado para o serviço `--user` do ai-memory.

## Tailscale (opcional)

Para usar o 9router e o modo dormir fora de casa, instale o Tailscale no PC e no notebook e
aponte o `HostName` para o nome MagicDNS. O tráfego da VM sai pelo NAT do host e alcança a
tailnet sem configuração extra (validar).
