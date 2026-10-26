# Como distribuir de graça um app Windows de código aberto

> Nota (2026-10-26): depois desta pesquisa, o app foi renomeado de Pixler para Pixmeter, porque "Pixler" soava igual a Pixlr. A seção 7 e o item 7 do resumo tratam do nome anterior.

Pesquisa do ticket WPC-268. Consulta feita em 2026-10-26. Todas as informações vêm de fontes oficiais (Microsoft Learn, documentação do GitHub, páginas dos provedores). O que não consegui confirmar está marcado como "não confirmado".

## Resumo

1. O caminho mais barato e simples para começar é o **GitHub Releases** (grátis) com um `.exe` portátil ou um instalador. O custo real é o aviso de segurança do Windows (SmartScreen), que aparece para programas novos ou sem assinatura.
2. O **winget** (instalar por `winget install`) é grátis e aceita executáveis portáteis, ZIP, Inno Setup, MSI e MSIX. A condição é que o instalador fique num endereço estável (por exemplo, o GitHub Releases). Não exige assinatura, mas exige instalação silenciosa.
3. A **Microsoft Store** hoje é grátis para pessoa física (sem taxa de cadastro, com verificação de identidade por documento e selfie). Se o app for empacotado como **MSIX**, a Microsoft assina de graça e o aviso do SmartScreen nunca aparece. Não encontrei caminho especial para código aberto; o caminho comum já é gratuito.
4. **Assinatura de código** (para o Windows confiar no programa): a opção da Microsoft (Artifact Signing, antigo Trusted Signing) custa a partir de **US$ 9,99 por mês**, mas para pessoa física só vale para quem mora nos **Estados Unidos ou Canadá**. O Brasil não está na lista de países. Alternativa gratuita para código aberto: **SignPath Foundation**, com regras que o Pixmeter provavelmente cumpre.
5. Certificado EV não vale mais a pena: a Microsoft diz que ele **não evita mais** o aviso do SmartScreen. Mesmo assinado, o programa mostra aviso até acumular reputação (semanas e centenas de instalações).
6. **Aviso de nova versão** sem quebrar "nada sai do computador": a opção mais honesta é **não checar sozinho** e deixar um botão/link "Ver novidades" que abre a página de releases no navegador. Se um dia houver checagem automática, deve ser **opcional (desligada por padrão)**, consultando só a página de releases do GitHub. Na Store com MSIX, a própria Store atualiza e o app não precisa falar com a internet.
7. **Nome "Pixler"**: não achei produto Windows com esse nome, nem pacote no winget. Existem repositórios pequenos no GitHub com o nome (nenhum é régua de tela) e um nome parecido registrado, **Pixlr**. Marcas (INPI, USPTO, EUIPO) e a Store **não consegui confirmar** por ferramenta; recomendo checagem manual antes de divulgar.

## 1. GitHub Releases

- **Custo:** grátis para repositório público.
- **Como funciona:** você publica uma "release" com arquivos anexos (um `.exe` portátil, um `.zip` ou um instalador). Os usuários baixam direto da página.
- **Vantagens:** zero infraestrutura, histórico de versões, é o endereço que o winget e a checagem de versão usam.
- **Desvantagens:** sem assinatura, o Windows mostra "O Windows protegeu seu computador" (SmartScreen) e o usuário precisa clicar em "Executar assim mesmo". Cada versão nova sem assinatura recomeça sem reputação. Fonte: [SmartScreen reputation for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation).
- **Executável portátil x instalador:** o portátil é o mais simples de manter e combina com "leve e independente"; porém não cria atalho, não aparece em "Aplicativos instalados" e não se atualiza. A Microsoft descreve esse modelo como adequado a "utilitários simples" e a público técnico ([Choose a distribution path](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/choose-distribution-path)).

## 2. winget

- **Custo:** grátis. O catálogo comunitário é o repositório aberto [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs).
- **Como entrar:** você envia um "manifesto" (arquivo YAML que descreve o app) por Pull Request. A ferramenta `wingetcreate` monta e envia isso. Um robô valida o pacote automaticamente. Fontes: [Submit packages to Windows Package Manager](https://learn.microsoft.com/en-us/windows/package-manager/package/), [Create your package manifest](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest), [Authoring Manifests](https://github.com/microsoft/winget-pkgs/blob/master/doc/Authoring.md).
- **Formatos aceitos:** EXE, ZIP, Inno, Nullsoft, MSI, WiX, APPX, MSIX, Burn, **Portable**, Font ([WinGet docs](https://learn.microsoft.com/en-us/windows/package-manager/winget/)).
- **Requisitos principais:**
  - o link do instalador deve ser a página oficial de lançamentos do autor (sites de download de terceiros não são aceitos) e deve ser estável e específico da versão;
  - deve instalar de forma silenciosa ("All tools must support a silent install");
  - o app não pode conter malware, nem baixar e executar código que mude o que ele promete; precisa ser testável;
  - se coletar dados pessoais, precisa de política de privacidade (o Pixmeter não coleta);
  - o nome do pacote e do publicador devem bater com o que aparece em "Aplicativos instalados".
  Fonte: [Windows Package Manager repository policies](https://learn.microsoft.com/en-us/windows/package-manager/package/windows-package-manager-policies).
- **Assinatura:** a documentação consultada não a exige para entrar no winget. (Outras páginas recomendam certificado para evitar o SmartScreen.)
- **Atualizações:** o usuário roda `winget upgrade`; cada versão nova exige um novo Pull Request no repositório (pode ser automatizado). Prazo de aprovação: não informado na documentação (não confirmado).

## 3. Microsoft Store

- **Custo da conta de pessoa física:** **sem taxa de cadastro**, desde setembro de 2025. Entrada pelo site [storedeveloper.microsoft.com](https://storedeveloper.microsoft.com/); exige conta Microsoft pessoal e verificação com documento oficial com foto e selfie. Fontes: [Steps to open a Microsoft Store developer account](https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account), [anúncio no blog do Windows](https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/). Atenção: uma busca na web mostrou que a página [developer.microsoft.com/store/register](https://developer.microsoft.com/en-us/store/register) ainda citava uma taxa de US$ 19; parece desatualizada, mas não confirmei a página diretamente.
- **Regra de conta individual:** a Microsoft diz que a conta individual é para quem publica "sem relação com negócio, comércio ou profissão", em projetos pessoais e não comerciais. Trocar depois de individual para empresa **não é possível** (precisa criar outra conta). Mesma página acima.
- **Caminho especial para código aberto:** **não encontrei** nenhum na documentação oficial. Não é necessário, porque o cadastro já é grátis.
- **Dois jeitos de enviar o app:**
  - **MSIX:** a Microsoft assina e hospeda o pacote (assinatura grátis), com atualização pela Store e sem aviso do SmartScreen.
  - **MSI/EXE (instalador próprio):** você hospeda um endereço HTTPS fixo por versão e precisa **assinar** o instalador com certificado de uma autoridade reconhecida (autoassinado não é aceito); a Store **não** atualiza, o app cuida disso. O instalador precisa ser silencioso e offline (não pode ser "baixador"). Certificação leva até 3 dias úteis.
  Fontes: [Choose a distribution path](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/choose-distribution-path), [certificação MSI/EXE](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msi/app-certification-process).
- **Observação:** um app empacotado em MSIX roda com algumas regras diferentes (instalação isolada, identidade de pacote). Para uma régua de tela que desenha por cima de outras janelas, isso precisa ser testado antes; não pesquisei esse ponto (não confirmado).

## 4. Formatos de pacote e o que muda para o usuário

| Formato | Custo da ferramenta | Para o usuário | Atualização | Assinatura |
|---|---|---|---|---|
| `.exe` portátil / ZIP | grátis | baixa e executa, sem instalar | manual | recomendada |
| Inno Setup (instalador `.exe`) | grátis, inclusive uso comercial ([licença](https://jrsoftware.org/files/is/license.txt)) | assistente clássico, atalho, desinstalador | por conta do app | recomendada |
| WiX / MSI | WiX é gratuito, mas organizações com receita acima de US$ 10 mil/ano devem patrocinar o projeto (taxa de manutenção). A página não menciona exceção para pessoa física ou código aberto (não confirmado) ([OSMF](https://docs.firegiant.com/wix/osmf/)) | formato corporativo, bom para TI de empresas | por conta do app | recomendada |
| MSIX | grátis | instala e remove limpo, atualiza sozinho | pela Store, ou por arquivo `.appinstaller` | **obrigatória** fora da Store (certificado confiável) |

Resumo prático: para um app leve e independente, o ZIP/portátil e o Inno Setup são os mais simples; o MSIX só compensa se for pela Store. Fonte geral: [Choose a distribution path](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/choose-distribution-path).

## 5. Assinatura de código e o aviso do SmartScreen

O que a Microsoft diz ([SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)):

- **Store:** nenhum aviso (a Microsoft assina).
- **Certificado válido (OV ou EV):** ainda há aviso de "app não reconhecido" até acumular reputação; o nome do publicador aparece. A reputação pode levar "várias semanas e centenas de instalações limpas". A reputação passa de uma versão para a outra se o mesmo certificado/identidade for usado.
- **Sem assinatura ou autoassinado:** aviso "O Windows protegeu seu computador"; cada versão recomeça do zero.
- **EV:** "não evita mais o SmartScreen"; pagar mais por EV só para isso "não se justifica".
- No Windows 11, o recurso **Smart App Control** pode bloquear arquivos sem assinatura que não tenham reputação positiva.

Opções:

| Opção | Custo | Requisitos | Observações |
|---|---|---|---|
| **Artifact Signing** (Microsoft, antigo Trusted Signing) | a partir de **US$ 9,99/mês** (informado em [SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)); a [página de preços](https://azure.microsoft.com/en-us/pricing/details/artifact-signing/) lista dois planos (Basic: 5.000 assinaturas/mês; Premium: 100.000/mês), mas os valores não apareceram na minha consulta | assinatura Azure **paga** (conta gratuita/teste não serve); validação de identidade com documento; pessoa física só nos **EUA ou Canadá**; empresas só em EUA, Canadá, UE, Reino Unido, Austrália, Nova Zelândia, Japão, Coreia do Sul, Singapura, Suíça, Noruega e Israel. **O Brasil não está na lista.** Cobrança integral do mês, sem proporcional | sem token físico, funciona em automação (GitHub Actions). Não emite EV. Fontes: [Quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart), [FAQ](https://learn.microsoft.com/en-us/azure/artifact-signing/faq) |
| **SignPath Foundation** | **grátis** para código aberto ([termos](https://signpath.org/terms)) | licença aprovada pela OSI (MIT serve), sem malware, projeto ativo e já lançado, o time desenvolve o código que assina, página do projeto com seção "Code signing policy" (papéis e política de privacidade), autenticação em dois fatores para a equipe, aprovação manual de cada release, nome e versão do produto definidos em todo build | o certificado fica em nome da SignPath Foundation (ela é a publicadora que aparece); aceitação a critério deles, sem recurso. Tempo de aprovação: não informado (não confirmado) |
| **Certificado OV comercial** | tipicamente US$ 150 a 300 por ano segundo a Microsoft ([Choose a distribution path](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/choose-distribution-path)); preços de cada autoridade certificadora não consultados (não confirmado) | validação de identidade/empresa | ainda mostra aviso no início |
| **Certificado EV** | mais caro (valor não consultado) | idem, com token físico (não confirmado) | sem vantagem para o SmartScreen hoje |
| **Store com MSIX** | grátis | conta de desenvolvedor | Microsoft assina; sem aviso |

Leitura para o Pixmeter: o caminho mais barato e sem aviso é **Store com MSIX**; fora da Store, o caminho gratuito mais provável é **SignPath Foundation**. Artifact Signing só vale se o dono morar nos EUA/Canadá (ou abrir uma empresa em país listado), o que não parece ser o caso.

## 6. Como avisar sobre versão nova sem quebrar "nada sai do computador"

Qualquer checagem automática contacta um servidor e, no mínimo, revela o endereço IP ao GitHub. Por isso é uma exceção real à regra. Opções, da mais privada para a menos:

1. **Sem checagem.** O app só mostra a versão e um link para a página de releases (abre no navegador, por ação do usuário). Nada é enviado pelo app.
2. **Store (MSIX):** a Store atualiza sozinha. O app não faz conexão nenhuma. Não vale para a rota MSI/EXE da Store, em que a atualização fica por conta do app ([fonte](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/choose-distribution-path)).
3. **winget:** quem instalou por winget atualiza com `winget upgrade`; também sem conexão do app.
4. **Checagem opcional no app (desligada por padrão):** uma única consulta ao endereço `GET /repos/{dono}/{repo}/releases/latest` da [API do GitHub](https://docs.github.com/en/rest/releases/releases#get-the-latest-release), sem cadastro. Sem autenticação o limite é 60 consultas por hora por IP ([fonte](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)), mais que suficiente para uma checagem por dia. O servidor vê o IP e o tipo de programa que consultou. Deve haver opção clara para ligar/desligar e o texto deve explicar isso. A resposta traz a versão e o endereço do download; o app só avisa, não baixa nem instala sozinho.

Observação: a política do winget exige que dados pessoais enviados a terceiros dependam de consentimento explícito na interface ([política 1.5.2](https://learn.microsoft.com/en-us/windows/package-manager/package/windows-package-manager-policies)); a opção 4, opt-in, atende.

## 7. Disponibilidade do nome "Pixler"

Resultado: **nenhum conflito direto encontrado**, mas parte das checagens não foi possível.

- **winget:** busca de código no repositório microsoft/winget-pkgs por "pixler" retornou 0 resultados (consulta pela API do GitHub em 2026-10-26; o índice de busca do GitHub pode estar incompleto). Repositório: https://github.com/microsoft/winget-pkgs
- **Microsoft Store:** **não confirmado.** A busca da Store não carrega sem navegador e a busca na web não mostrou nenhuma página "Pixler" na Store; só apareceu o **Pixlr** (editor de fotos, grafia diferente), [anúncio de 2015](https://pixlr.com/blog/autodesk-pixlr-now-available-on-the-microsoft-store/). A reserva do nome é feita no Partner Center ao criar o app; é o teste definitivo.
- **GitHub:** o nome já é usado, sem relação com régua de tela:
  - usuário [pixler](https://github.com/pixler): conta de 2013, vazia (0 repositórios);
  - [SaraVieira/pixler](https://github.com/SaraVieira/pixler): converte imagens em pixel art (web, MIT, 32 estrelas, parado desde 2021);
  - [slembcke/pixler](https://github.com/slembcke/pixler): biblioteca em C para jogos de NES;
  - [grapgrap/pixler](https://github.com/grapgrap/pixler): converte imagem em pixel art;
  - [IAmBullsaw/Pixler](https://github.com/IAmBullsaw/Pixler): "pixelsorting".
  Busca feita: https://api.github.com/search/repositories?q=pixler+in:name (72 resultados, nenhum relevante). O nome de usuário/organização `pixler` está ocupado; o repositório pode ficar em outra conta (como `carreirodev/pixler`).
- **Domínios:** [pixler.com](https://pixler.com) é página de domínio estacionado, possivelmente à venda; [pixler.io](https://pixler.io) é uma página "em breve" sem informação de produto; pixler.app e pixler.dev responderam (conteúdo não verificado).
- **Marcas registradas:** **não confirmado.** Os buscadores oficiais (INPI no Brasil, [USPTO](https://tmsearch.uspto.gov/), [EUIPO](https://euipo.europa.eu/eSearch/), WIPO) exigem consulta manual no navegador e não retornaram dados para minhas ferramentas. Busca na web não achou marca nem software chamado exatamente "Pixler". Nome parecido encontrado: **PIXLR** (PIXLR PTE. LTD., Singapura), marca para software de edição de imagens ([Justia](https://trademarks.justia.com/owners/pixlr-pte-ltd-5046449)). A grafia é diferente (Pixlr x Pixler), mas o som é igual e o campo (software gráfico) é próximo; vale o dono verificar com atenção. Isto não é parecer jurídico.

## O que ficou sem confirmar

- Valor exato do Artifact Signing (só o "a partir de US$ 9,99/mês" citado pela Microsoft; a página de preços não mostrou números).
- Preço de certificados OV/EV nas autoridades certificadoras (só a faixa citada pela Microsoft).
- Se a taxa de US$ 19 em developer.microsoft.com é só texto antigo.
- Prazo de aprovação no winget e na SignPath Foundation.
- Como uma régua de tela se comporta empacotada em MSIX.
- Reserva do nome na Microsoft Store e busca de marcas (INPI, USPTO, EUIPO, WIPO).
- País de residência do dono (define se o Artifact Signing é possível).
