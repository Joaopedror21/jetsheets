# Arquitetura do JetSheets

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
- **storage**: importação CSV/XLSX, exportação das linhas visíveis para CSV/XLSX e arquivo de projeto `.jetsheet`, versionado. A gravação sincroniza um arquivo temporário no mesmo diretório e só então substitui o destino; se a gravação falhar antes da substituição, o projeto anterior é preservado.
- **charts**: especificação persistível de barras ou linhas e preparação dos pontos a partir das linhas visíveis. A interface desenha os gráficos com egui.
- **ui**: janela nativa, grade editável com seleção retangular e transferência TSV, prévia de comandos, resultados, visualização e operações de arquivo. O processamento de arquivos é enviado a uma thread de trabalho. O módulo `theme.rs` concentra as paletas Jet Dark/Jet Light, espaçamento e componentes de cartões, títulos e métricas; a escolha de tema é estado da interface durante a sessão.

## Estado e fluxo

O projeto contém uma tabela, o filtro confirmado, a ordenação, o limite opcional de linhas visíveis, o agrupamento, a configuração do gráfico e o dashboard. A interface mantém separadamente o comando em digitação, a prévia e o histórico de visualização para desfazer/refazer filtro, ordenação, ranking ou agrupamento. Editar uma célula preserva o índice da linha na tabela de origem; resultados, gráficos e indicadores são recalculados a partir do estado atual. Salvar grava somente dados e visualizações confirmados.

## Evolução da Fase 2

A ordenação por coluna é aplicada sobre os índices das linhas visíveis, sem reorganizar os dados de origem. Números brasileiros e datas são comparados pelos seus valores, texto ignora diferenças de maiúsculas e acentos e células vazias ficam no fim em ambas as direções. O comando em português e o seletor de resultados usam a mesma operação. O campo opcional `sort` no projeto mantém compatibilidade de leitura com arquivos `.jetsheet` anteriores.

O ranking em português aplica a ordenação e depois reduz os índices visíveis a até `visible_limit` registros. Resumos, gráficos, dashboard e exportações usam esses mesmos índices. O limite é opcional no projeto para manter a leitura de arquivos anteriores; ao mudar a ordenação manualmente, a interface o remove. Um comando de gráfico gera uma `ChartSpec` validada e usa a mesma preparação de dados do painel visual; o gráfico resultante pode ser adicionado ao dashboard.

Agrupamentos de contagem, soma e média operam sobre os mesmos índices visíveis, preservando a primeira ordem encontrada e considerando apenas células numéricas preenchidas. A tabela agrupada é recalculada após alterações nos dados ou filtros, permite exportação CSV e guarda a especificação no campo opcional `aggregate` do projeto. Arquivos de projeto antigos continuam legíveis.

O módulo XLSX usa `calamine` para ler as abas e seus valores e `rust_xlsxwriter` para exportar a visualização atual. O usuário escolhe uma única aba para importar. Fórmulas são lidas pelos valores em cache e não entram no modelo de dados; a interface informa a perda de abas restantes, fórmulas, formatação, gráficos e macros. O formato `.jetsheet` continua sendo o arquivo de projeto completo do aplicativo.

O dashboard persiste a coluna selecionada para métricas e até seis especificações de gráfico no campo opcional `dashboard` do projeto. Os valores dos cartões não são gravados: são derivados dos índices visíveis após cada alteração de dados, filtro ou ordenação. A leitura de arquivos anteriores cria um dashboard vazio por padrão. Referências a colunas inexistentes são recusadas na validação do projeto.

O instalador/atualizador é um crate Rust separado para Windows. Ele não altera o formato `.jetsheet`. O fluxo de download, verificação e recuperação é descrito em [INSTALLER.md](INSTALLER.md).

## Limite da Fase 1

O interpretador executa filtros, ordenação, rankings simples, agrupamentos de contagem, soma ou média e comandos explícitos para gráficos de barras ou linhas em português. Fórmulas, comandos compostos avançados, layouts livres de dashboard e outros tipos de gráficos pertencem a fases posteriores. O arquivo `.jetsheet` usa JSON estruturado e versionado. Compactação e recursos avançados de recuperação ficam para uma evolução do formato.
