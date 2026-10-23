Você vai montar o quadro de tarefas deste projeto a partir do backlog escrito em markdown.
Ninguém está acompanhando esta sessão: faça tudo e reporte no fim.

1. Veja o que já está no quadro, para não duplicar:
   "$ASSISTENTE_FORA_DO_JAIL" kanban listar
2. Procure o backlog: arquivos .md do projeto com itens "- [ ]" (ex.: docs/FUNCIONALIDADES.md,
   TODO.md, ROADMAP.md). Leia também CLAUDE.md e AGENTS.md para entender o projeto. Não leia o
   código inteiro: só o necessário para entender um item.
3. Para cada item "- [ ]" que ainda não esteja no quadro, crie um card:
   "$ASSISTENTE_FORA_DO_JAIL" kanban criar --titulo "<ID e título>" --descricao "<o que fazer, com o
   'Pronto quando' e o 'Como testar' se houver>" --prioridade <alta|normal|baixa> [--depende-de "T1,T3"]
   - Crie como manual (o padrão). Não use --tipo agente: quem decide quais cards vão para o agente
     é o usuário.
   - Pule itens marcados *(humano)* ou que dependem de validação humana, e diga quais.
   - Pule itens já marcados [x].
   - Dependências: o card não abre trabalhador antes das dependências estarem na main. Leia o
     "Ordem:" do texto de cada seção (ex.: "F11.1 antes dos demais", "F9.2–F9.4 antes de F9.5")
     e o "Depende de" de cada item, e passe em `--depende-de` os ids de card (T1, T2…, não os do
     backlog) dos itens dos quais este depende. Crie os cards na ordem do backlog, para a
     dependência já existir; item já feito ([x]) não vira dependência. Sem "Ordem:" nem
     "Depende de", não invente dependência.
4. Não implemente nada, não mexa em arquivos do projeto e não execute o trabalho dos itens.
5. Termine com um resumo curto: quantos cards criou, quais itens pulou e por quê.
