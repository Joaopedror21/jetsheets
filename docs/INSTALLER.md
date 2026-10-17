# Instalador e atualizações

## Fluxo

`jetsheets-installer` é uma interface desktop Rust/egui para Windows. A origem fixa é `Joaopedror21/JetSheets`. Ao iniciar, busca a última Release estável na API do GitHub, apresenta a tag e as notas e compara a versão com `jetsheets-install.json` da pasta selecionada.

Esse fluxo sem login exige que o repositório e suas Releases sejam públicos. O repositório está público; ainda não há uma Release, então a consulta retorna 404 até a primeira publicação. O instalador distribuído também contém a versão atual do aplicativo e permite instalá-la sem download. Para um repositório privado seria necessário outro fluxo de autenticação antes de oferecer downloads.

O ativo esperado chama-se `JetSheets-windows-x86_64.exe`. O instalador exige HTTPS, nome e URL esperados, tamanho válido e o campo `digest` SHA-256 do ativo. Sem esse digest, ele não instala. O download é gravado em `.part` na própria pasta escolhida. Ao pausar, fecha a resposta HTTP; ao continuar, solicita `Range` e confere `Content-Range`. Se o servidor ignorar `Range`, reinicia o arquivo desde o começo. Cancelar apaga o `.part`.

Depois de conferir tamanho e SHA-256, o instalador grava um novo registro e move o executável anterior para `.jetsheets-rollback.exe` antes de ativar o novo. Um início posterior recupera a versão anterior se encontrar uma troca incompleta. Ele recusa downgrade e recusa uma Release que reuse a mesma tag com um digest diferente. O usuário deve fechar o aplicativo antes da troca do executável. O arquivo de projeto não faz parte da atualização.

## Publicação por GitHub Actions

O workflow `.github/workflows/release.yml` roda em Windows quando uma tag `vMAJOR.MINOR.PATCH` é enviada. Ele verifica que a tag corresponde às versões de `jetsheets-ui` e `jetsheets-installer`, executa format/check/test/clippy, compila primeiro o aplicativo e depois o inclui no instalador, e publica os dois `.exe` em uma GitHub Release. O instalador lê as Releases; artefatos temporários de runs do Actions não são usados como feed de atualização.

Para a primeira publicação, após revisar e enviar o código para o repositório:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Para cada versão posterior, altere `version` nos dois `Cargo.toml`, atualize `Cargo.lock`, envie o código e crie uma nova tag correspondente. Não reutilize uma tag ou substitua um ativo publicado. A publicação exige que o repositório permita ao `GITHUB_TOKEN` criar Releases; o workflow pede somente `contents: write` no job de publicação.

Link direto após a primeira publicação: `https://github.com/Joaopedror21/JetSheets/releases/latest/download/JetSheets-Installer-windows-x86_64.exe`.

## Limites de segurança

A verificação SHA-256 detecta arquivo incompleto ou diferente do digest retornado pelo GitHub. Ela não substitui assinatura de código: quem controla a Release e o digest no repositório ainda pode publicar um binário malicioso. Não há certificado Authenticode configurado, por isso o Windows pode exibir avisos ao executar os arquivos. Para reforçar a cadeia de publicação, habilite proteção das tags e Releases imutáveis no GitHub e adicione assinatura de código quando houver certificado apropriado.

Referências: [API de Releases](https://docs.github.com/en/rest/releases/releases), [digest dos ativos](https://docs.github.com/en/rest/releases/assets), [gh release create](https://cli.github.com/manual/gh_release_create).
