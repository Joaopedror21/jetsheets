# JetSheets

Aplicativo desktop offline para editar tabelas, importar CSV e explorar dados com filtros escritos em português. Todo o código de execução é Rust; a interface usa `eframe`/`egui`.

## Executar no Windows

Requer Rust 1.95 ou mais recente e as ferramentas de compilação MSVC para Windows.

```powershell
cargo run -p jetsheets-ui
```

Para verificar o workspace:

```powershell
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

No ambiente de desenvolvimento deste repositório, os comandos também funcionam com `--offline` antes de `--`.

## Instalador e atualizações para Windows

O instalador é um executável Rust separado. `dist/JetSheets-Setup.exe` abre diretamente, sem `.bat` ou `.cmd`, e já inclui a versão atual do aplicativo para instalação sem download. As versões publicadas estão em [GitHub Releases](https://github.com/Joaopedror21/jetsheets/releases). O instalador consulta a última versão estável, mostra as notas da publicação, permite escolher a pasta e instala `JetSheets.exe` nela.

Durante o download, **Pausar** fecha a conexão e **Continuar** retoma pelo trecho restante; **Cancelar** remove o arquivo parcial. Antes de substituir o aplicativo, o instalador confere o tamanho e o SHA-256 informados pelo GitHub. Se a atualização falhar ou for interrompida, tenta recuperar a versão anterior. O instalador também mostra quando o JetSheets já está instalado e permite desinstalá-lo após confirmação, removendo somente o executável e o registro. Feche o JetSheets antes de atualizar ou desinstalar. Projetos `.jetsheet` não são removidos.

Para compilar um instalador que inclua o aplicativo localmente:

```powershell
cargo build --release --locked -p jetsheets-ui
$env:JETSHEETS_BUNDLE_PATH = (Resolve-Path target/release/jetsheets-ui.exe).Path
cargo build --release --locked -p jetsheets-installer
Remove-Item Env:JETSHEETS_BUNDLE_PATH
Copy-Item target/release/jetsheets-installer.exe dist/JetSheets-Setup.exe
```

O arquivo fica em `target/release/jetsheets-installer.exe`. No modo release, tanto ele quanto o JetSheets usam o subsistema gráfico do Windows e não abrem um console; builds debug mantêm o terminal. Erros de inicialização aparecem em diálogo e ficam em `%LOCALAPPDATA%\JetSheets\logs`. O instalador grava o executável na pasta escolhida; não cria atalho nem entrada no painel de aplicativos do Windows. O procedimento de publicação e as decisões de segurança estão em [docs/INSTALLER.md](docs/INSTALLER.md).

## Fluxo rápido

1. Use **+ Linha** para começar uma tabela vazia ou **Importar CSV** para abrir um arquivo UTF-8 com cabeçalhos. O arquivo [estoque_exemplo.csv](examples/estoque_exemplo.csv) contém dados sintéticos para experimentar.
2. Edite os nomes das colunas no cabeçalho e os valores diretamente na grade. Clique em uma célula e use Shift+clique em outra, ou arraste o mouse entre elas, para selecionar um intervalo. Ctrl+C e Ctrl+X copiam ou recortam a seleção, Ctrl+V cola blocos TSV que caibam nas linhas e colunas visíveis, e Delete apaga o intervalo selecionado. A inferência de número, moeda, porcentagem, data e texto é atualizada após a edição.
3. Digite `Filtre os produtos com valor acima de 5000 e quantidade maior que 4000`, clique em **Interpretar comando**, confira a interpretação e clique em **Aplicar filtro**. Para ordenar, use `Ordene do maior para o menor valor` ou `Ordene por valor crescente`.
4. Examine a contagem, o percentual e as estatísticas das colunas numéricas. Clique numa célula para ver os detalhes do registro. Escolha também uma coluna e direção no seletor **Ordenação**. Use `Agrupe por categoria`, `Conte por categoria` ou `Some valor por categoria` para ver uma tabela agrupada; ela usa as linhas visíveis e pode ser exportada em CSV. **Desfazer**, **Refazer** e **Limpar filtro** controlam a visualização sem apagar as linhas de origem. **Exportar CSV** grava as linhas visíveis na ordem exibida.
5. Em **Gráfico**, selecione barras ou linhas, os eixos X/Y e um título. O gráfico usa as linhas visíveis e se atualiza quando o filtro ou os dados mudam. Se houver mais de 48 pontos, navegue pelas páginas. Abra **Dashboard** e clique em **Adicionar gráfico atual** para guardar até seis gráficos. Indicadores, grupos e gráficos do dashboard acompanham o filtro e as edições.
6. Use **Arquivo → Salvar** para criar um `.jetsheet` e **Arquivo → Abrir projeto** para recuperar tabela, filtro, ordenação, agrupamento, gráfico e dashboard. O arquivo é local e versionado. Ao fechar com alterações pendentes, **Salvar** conclui a saída somente depois que a gravação termina sem erro.
7. Em **Configurações**, escolha **Jet Dark** ou **Jet Light**. A mudança é imediata e vale para a sessão atual.

O interpretador aceita comparações `>`, `>=`, `<`, `<=`, igualdade, diferença, `entre`, `contém`, `começa com` e `termina com`, além de condições `e`, `ou` e negação. `E` tem precedência sobre `OU`; o intervalo `entre` inclui os limites. Ele entende `5.000`, `R$ 5.000`, `5.000,50` e `5 mil`. Se uma coluna for ambígua, a interface pede uma escolha antes de gerar a prévia.

## Estrutura

Veja [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Os módulos de dados e interpretação não dependem da interface. A importação e a leitura/gravação de projetos executam em uma thread de trabalho.

## Limites desta fase

- A grade edita células individualmente, seleciona intervalos com Shift+clique ou arraste e copia/cola blocos dentro da área existente. A colagem não cria linhas ou colunas; navegação por atalhos entre células e desfazer edições de dados ainda não estão implementados.
- A linguagem natural executa filtros, ordenação, contagem por grupo e soma de uma coluna numérica por grupo. Médias por grupo, comandos de gráfico e fórmulas ficam para etapas seguintes. Comandos não suportados retornam erro explícito.
- Os resultados exibem a tabela filtrada e ordenada, estatísticas e exportação CSV. A ordenação preserva a ordem original dos dados, mantém valores vazios ao final e é salva no projeto. O dashboard permite indicadores e até seis cartões de barras ou linhas; layouts livres e outros tipos de gráfico ainda não estão disponíveis.
- CSV é lido em UTF-8, com `,` ou `;`, aspas e BOM. XLSX ainda não é suportado.
- `.jetsheet` usa JSON versionado, sem compactação nesta fase. O salvamento escreve e sincroniza um arquivo temporário antes da substituição; recuperação automática de versões anteriores ainda não existe.
- A grade cria widgets apenas para linhas visíveis e descarta colunas fora da área de rolagem, mas filtragem e inferência de tipos ainda processam a tabela em memória. Não há promessa de escala para milhões de linhas nesta fase.
