Você vai montar o quadro de tarefas deste projeto a partir do backlog escrito em markdown.
Ninguém está acompanhando esta sessão: faça tudo e reporte no fim.

1. Veja o que já está no quadro, para não duplicar:
   "$ASSISTENTE_FORA_DO_JAIL" kanban listar
2. Procure o backlog: arquivos .md do projeto com itens "- [ ]" (ex.: docs/FUNCIONALIDADES.md,
   TODO.md, ROADMAP.md). Leia também CLAUDE.md e AGENTS.md para entender o projeto. Não leia o
   código inteiro: só o necessário para entender um item.
3. Para cada item "- [ ]" que ainda não esteja no quadro, crie um card:
   "$ASSISTENTE_FORA_DO_JAIL" kanban criar --titulo "<ID e título>" --descricao "<o que fazer, com o
   'Pronto quando' e o 'Como testar' se houver>" --prioridade <alta|normal|baixa>
   - Crie como manual (o padrão). Não use --tipo agente: quem decide quais cards vão para o agente
     é o usuário.
   - Pule itens marcados *(humano)* ou que dependem de validação humana, e diga quais.
   - Pule itens já marcados [x].
4. Não implemente nada, não mexa em arquivos do projeto e não execute o trabalho dos itens.
5. Termine com um resumo curto: quantos cards criou, quais itens pulou e por quê.
