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

## Como a sessão é lançada

`ai-jail --rw-map ~/.assistente/perfis/<perfil> -- <agente>`, com cwd no projeto (montado rw
pelo ai-jail). Montado em `hub_core::session::LaunchPlan`.

- `~/.assistente/` **não** está entre os dotdirs que o ai-jail monta por padrão (`.claude`,
  `.config`, `.cache`, `.cargo`, `.docker`…), por isso as credenciais dos perfis ficam lá:
  só a do perfil ativo entra.
- `.ai-jail` no projeto (versionado) pode acrescentar `rw_maps`/`ro_maps`, por exemplo uma lib
  compartilhada.
- Rede: herdada (o agente precisa do 9router e do ai-memory em 127.0.0.1). `--lockdown` fica
  disponível como ação "sessão trancada" para revisar código suspeito (sem rede, projeto só leitura).

## Docker dentro da sessão

O ai-jail detecta e monta o `/var/run/docker.sock`, o que permite `./dev just check` nos
projetos. **Isso equivale a root dentro da VM.** Aceito porque a VM é a fronteira principal
e não tem nada além dos projetos e das credenciais de agente. Se um projeto não precisa de
Docker, o `.ai-jail` dele pode desligar.

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
