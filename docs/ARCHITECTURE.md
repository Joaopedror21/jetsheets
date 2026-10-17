# Arquitetura da Fase 1

JetSheets é um aplicativo desktop offline, escrito inteiramente em Rust.

```text
jetsheets-ui (eframe/egui)
  ├── jetsheets-language ──→ jetsheets-core
  ├── jetsheets-storage ───→ jetsheets-core, jetsheets-charts
  ├── jetsheets-charts ────→ jetsheets-core
  └── jetsheets-core
```

- **core**: tabela, inferência de tipos, AST de filtros, execução determinística e estatísticas. As linhas originais são preservadas; um filtro produz índices de linhas visíveis.
- **language**: normalização e interpretação local de comandos de filtro em português. Erros de sintaxe, coluna desconhecida e ambiguidade são explícitos. Nenhum texto digitado é executado como código.
- **storage**: importação CSV, exportação das linhas visíveis e arquivo de projeto `.jetsheet`, versionado. A gravação sincroniza um arquivo temporário no mesmo diretório e só então substitui o destino; se a gravação falhar antes da substituição, o projeto anterior é preservado.
- **charts**: especificação persistível de barras ou linhas e preparação dos pontos a partir das linhas visíveis. A interface desenha os gráficos com egui.
- **ui**: janela nativa, grade editável com seleção retangular e transferência TSV, prévia de comandos, resultados, visualização e operações de arquivo. O processamento de arquivos é enviado a uma thread de trabalho. O módulo `theme.rs` concentra as paletas Jet Dark/Jet Light, espaçamento e componentes de cartões, títulos e métricas; a escolha de tema é estado da interface durante a sessão.

## Estado e fluxo

O projeto contém uma tabela, o filtro confirmado e a configuração do gráfico. A interface mantém separadamente o comando em digitação, a prévia e o histórico de filtros para desfazer/refazer. Editar uma célula preserva o índice da linha na tabela de origem; resultados e gráfico são recalculados a partir do estado atual. Salvar grava somente dados e visualizações confirmados.

## Limite da Fase 1

O interpretador executa filtros e comparações de valores; agrupamentos, agregações em linguagem natural, XLSX, fórmulas, dashboards e outros tipos de gráficos pertencem a fases posteriores. O arquivo `.jetsheet` usa JSON estruturado e versionado nesta primeira fase. Compactação e recursos avançados de recuperação ficam para uma evolução do formato.
