# Assistente

Hub pessoal para trabalhar com agentes de IA (Claude Code e OpenCode): um app desktop que abre
sessões em projetos **pessoais** e de **trabalho**, cada uma isolada numa **VM Incus** e no
**ai-jail**, com **memória compartilhada** (ai-memory) e o **9router** do notebook, e que à noite
entrega o trabalho ao notebook no **modo hora de dormir**.

> Estado: **esqueleto**. Funciona: config, hubd com saúde/perfis/projetos, UI com status.
> O resto está especificado em [`docs/`](docs/) e listado em [`docs/FUNCIONALIDADES.md`](docs/FUNCIONALIDADES.md).

## Documentação

- [Visão e requisitos](docs/VISAO.md) — suas anotações viraram R1–R9
- [Arquitetura](docs/ARQUITETURA.md) — componentes, fluxos, decisões e riscos
- [Funcionalidades](docs/FUNCIONALIDADES.md) — backlog com IDs (também é a fila do modo dormir)
- Specs: [VM e instalação](docs/specs/01-vm-e-instalacao.md) ·
  [hubd e sessões](docs/specs/02-hubd-e-sessoes.md) · [interface](docs/specs/03-interface.md) ·
  [perfis](docs/specs/04-perfis-e-projetos.md) · [sandbox](docs/specs/05-sandbox.md) ·
  [memória](docs/specs/06-memoria.md) · [notebook e 9router](docs/specs/07-notebook-e-9router.md) ·
  [modo dormir](docs/specs/08-modo-dormir.md)

## Desenvolver

Tudo roda no container (`./dev`); no host só precisa de Docker e git.

```bash
./dev just check      # todas as verificações
./dev just hubd       # hubd em http://127.0.0.1:7878 com a config de exemplo
./dev just app-dev    # app Tauri falando com esse hubd (janela via X11)
```

## Reinstalar do zero (depois de formatar)

1. Instale o Docker.
2. `git clone <este repo> && cd assistente`
3. `./instalar.sh ~/assistente-backup-AAAA-MM-DD.tar.gz` (sem backup: cria a config de exemplo)
4. `bin/assistente login claude pessoal` (e os outros perfis/agentes)
5. Abra **Assistente** no menu de aplicativos.

Backup: `bin/assistente backup`. Detalhes: [spec 01](docs/specs/01-vm-e-instalacao.md).
