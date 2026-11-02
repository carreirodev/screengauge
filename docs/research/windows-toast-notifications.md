# Notificações padrão do Windows num `.exe` sem pacote e no pacote MSIX

Pesquisa do ticket WPC-347 ("Windows notifications que ficam na central e esperam o 'Não perturbe'") para o ScreenGauge, a régua de tela para Windows 11 escrita em Rust sobre Win32, sem o Windows App SDK (ADR 0001). Hoje as notificações são balões do tray icon (ADR 0003). Medidos no computador do dono, os balões ficam uns 6 segundos, não ficam guardados na central (Win + N) e o Windows os descarta com o "Não perturbe" ligado ou com um programa em tela cheia na frente.

A pergunta: o que a documentação da Microsoft diz sobre a notificação padrão do Windows 11 (`Windows.UI.Notifications`, o "toast") enviada por um `.exe` **sem pacote** (a cópia do instalador) e pela cópia **empacotada em MSIX** (a cópia da Store)? Este documento só reúne fatos e o que eles permitem. Ele não decide nada.

Fontes: documentação da Microsoft (learn.microsoft.com e support.microsoft.com), o histórico público da documentação no GitHub (`MicrosoftDocs/windows-dev-docs`), o código-fonte de repositórios da Microsoft (`microsoft/WindowsAppSDK`, `microsoft/Windows-classic-samples`, `microsoft/windows-rs`) e o código da crate `windows` 0.62.2, a versão fixada no `Cargo.toml` do projeto. Tudo foi consultado em **2026-11-02**. Cada fato traz a fonte entre colchetes, como [F1]; a lista com os endereços está no fim. O que não consegui confirmar está marcado como **não confirmado**. Deduções minhas (não escritas pela Microsoft) estão marcadas como **dedução**. Fontes que não são da Microsoft estão marcadas como **terceiros** e valem como indício, não como prova. Trechos entre aspas são traduções minhas dos textos originais em inglês.

## Resumo

1. **Sem um "ativador" registrado, a notificação não fica guardada na central.** A página da Microsoft para apps C++ sem pacote dizia: se as notificações "aparecem mas não ficam guardadas na Central de Ações (somem depois que o balão sai), você não implementou o ativador COM corretamente" [F2]. O evento `Activated` do próprio `ToastNotification` vem do Windows 8, quando as notificações eram só balões passageiros. O ativador COM foi criado no Windows 10 justamente para a central [F2]. Ou seja: só com o `Activated`, o primeiro critério do ticket (ficar na central) não se cumpre. O ativador COM é o custo principal deste ticket.
2. **Registrar o app sem atalho no menu Iniciar** (só com a chave `HKCU\Software\Classes\AppUserModelId\<AUMID>`) aparece em uma página da Microsoft de 2025 [F1], que a própria Microsoft **tirou do ar em 2026-04-08**. Essa página usa `HKEY_LOCAL_MACHINE` no exemplo, não fala em atalho e não mostra o registro do servidor COM. A página de referência da API, ainda no ar, continua dizendo que um app de desktop "precisa ter um atalho na tela Iniciar" [F6]. As duas fontes se contradizem, e só o teste decide qual vale no Windows 11 de hoje.
3. O Windows App SDK e a biblioteca de notificações do Community Toolkit, que a Microsoft recomendava antes, gravam exatamente essas chaves em `HKCU` e funcionam sem atalho [F27][F31]. O Toolkit chega a usar o mesmo `Windows.UI.Notifications` que o ScreenGauge usaria. Mas o Windows App SDK também chama uma função interna do Windows antes de gravar as chaves [F27], então ele não prova que as chaves bastam sozinhas.
4. **Nome e ícone no cabeçalho:** no Windows 11, o Windows mostra o nome e o ícone do app no topo da notificação, e o app não consegue mudá-los pelo conteúdo da notificação [F10]. Pelo registro, eles vêm de `DisplayName` e `IconUri`, este último o caminho de um arquivo de imagem [F1][F27]. Pelo atalho, a documentação não diz de onde vêm; o Windows App SDK lê os dois do atalho [F24][F27] (**dedução**: vêm do nome e do ícone do atalho).
5. **Clique:** com o ativador registrado, um clique no canto da tela ou na central chama o método `Activate` dentro do ScreenGauge, se ele estiver rodando. Se não estiver, o Windows abre o `.exe` e depois chama o `Activate` [F2]. O custo: uma chave `LocalServer32` no registro, um ponteiro para essa chave (no atalho ou no registro) e, dentro do programa, um objeto COM que o Windows chama numa linha de execução separada [F2][F5][F28].
6. **"Não perturbe":** a página de suporte do Windows diz que, com ele ligado, as outras notificações "vão direto para a central de notificações" [F12]. **Ela não diz que o balão aparece depois que o "Não perturbe" acaba.** O ticket supõe que a notificação "espera e aparece depois"; isso não está documentado. O tratamento da tela cheia (`QUNS_BUSY`) para toasts também não está documentado.
7. **Não usar** os cenários `reminder`, `alarm`, `incomingCall` ou `urgent` para furar o "Não perturbe". Os três primeiros ficam presos na tela até o usuário fechar e exigem um botão; o último depende de uma permissão que o usuário dá numa janela do Windows [F10][F11]. A propriedade `Priority` é só uma "dica" [F8].
8. **As notificações expiram.** A Microsoft diz, para a API do Windows App SDK, que o prazo padrão e máximo na central é de **3 dias** [F9]. **Dedução**: vale também para `Windows.UI.Notifications`, que é a mesma central. Isso contradiz o critério do ticket "ficam na central até o usuário fechá-las".
9. **Cópia da Store (MSIX):** o manifesto precisa de duas extensões, `com:ExeServer` e `desktop:ToastNotificationActivation`, com o mesmo CLSID [F2][F15][F16]. O pacote não grava nada no registro, e o Windows apaga tudo ao desinstalar [F4]. O processo ainda precisa registrar o objeto COM ao iniciar [F2].
10. **Com as duas cópias instaladas no mesmo computador**, a cópia empacotada "toma o lugar" da sem pacote nos cliques: clicar numa notificação da cópia do instalador abre a cópia da Store [F2]. Isso toca o WPC-339 (duas cópias no mesmo computador).
11. **Crate `windows` 0.62.2:** as features novas são `UI_Notifications`, `Data_Xml_Dom` e `Win32_UI_Notifications`. As demais (`Win32_System_Com`, `Win32_UI_Shell`, `Win32_System_Registry`) já estão no projeto [F29]. Para escrever o objeto COM com a macro `#[implement]`, o projeto provavelmente precisa declarar a crate `windows-core` como dependência direta (**dedução**; ver seção 7). Isso exige aprovação do dono.
12. **Microsoft mudou a documentação em 2026.** As páginas que ensinavam toasts sem o Windows App SDK (C++ WRL, "outros apps sem pacote", C# com o Toolkit, "ativação em apps de desktop") foram apagadas entre 2026-04-08 e 2026-04-09. Hoje a Microsoft manda apps sem pacote usarem o Windows App SDK [F25][F32], que o ADR 0001 rejeitou. As versões finais das páginas apagadas foram lidas no histórico público do GitHub e são citadas com o commit.

## 0. A documentação mudou durante 2026

A página que o ticket menciona, "Send a local app notification from other types of unpackaged apps", não existe mais em learn.microsoft.com (o endereço devolve erro 404). O histórico público da documentação mostra o que aconteceu:

- Em 2025-07-28 as páginas de notificações foram movidas de pasta [F1].
- Em 2026-04-08, um commit chamado "Add per-framework notification pages, condense WRL to legacy" resumiu a página de C++ WRL e a marcou como "abordagem legada". No mesmo dia, outro commit, "Update content, titles, cross-links; delete legacy pages", apagou essa página, a de "outros apps sem pacote" e a de C# [F1][F2][F4].
- Em 2026-04-09, a página "Activating app notifications from desktop apps" foi "aposentada" [F3].

A pasta atual (commit de 2026-05-19) só tem páginas do Windows App SDK [F25][F32]. Este documento cita as versões finais completas das páginas apagadas, com o commit exato. Para a página de C++ WRL, usa a versão de 2025-07-28 de antes do resumo, que tinha mais detalhes [F2]. A API em si (`Windows.UI.Notifications`) continua documentada e no ar [F6][F7][F8].

**Dedução**: a Microsoft não declarou o caminho sem Windows App SDK como "não suportado". Ela só parou de documentá-lo. O que não está mais escrito precisa ser conferido no teste.

## 1. Registro do AppUserModelID num `.exe` sem pacote

O AppUserModelID (AUMID) é o nome único com que o Windows identifica o app. Cada notificação é enviada em nome de um AUMID, com `ToastNotificationManager.CreateToastNotifier(aumid)`. A referência da API diz: "não use esta sobrecarga [sem AUMID] para um app de desktop; use `CreateToastNotifier(appID)` para fornecer o AppUserModelID necessário" [F6].

A documentação descreve dois caminhos.

### 1.1 Atalho no menu Iniciar

É o caminho mais antigo e mais documentado:

- A página de referência, ainda no ar, diz: "Para um app de desktop mostrar um toast, o app precisa ter um atalho na tela Iniciar. O atalho precisa ter um AppUserModelID" [F6].
- A página arquivada de 2018 diz: "Sem um atalho válido instalado na tela Iniciar ou em Todos os Programas, você não consegue mostrar uma notificação toast de um app de desktop" [F20].
- A página de C++ WRL (2025) mandava gravar no atalho duas propriedades: `System.AppUserModel.ID` e `System.AppUserModel.ToastActivatorCLSID` [F2]. Dizia também: "Se as notificações simplesmente não aparecem (e nenhum erro é lançado), provavelmente o atalho no Iniciar não existe, ou o AUMID usado no código não bate com o do atalho" [F2].
- A página de C++/WinRT, no ar e atualizada em 2026-07-02, cria o atalho com essas duas propriedades e grava a chave `LocalServer32` [F5].
- O exemplo oficial `DesktopToasts` faz o mesmo, no código e no instalador WiX [F28].

### 1.2 Só o registro, sem atalho

A página "Send a local app notification from other types of unpackaged apps" (2025-07-28, apagada em 2026-04-08) tinha como passo 1 "Registre seu app no registro", com este exemplo [F1]:

| Valor | Tipo | Exemplo da página |
| --- | --- | --- |
| `DisplayName` | `REG_EXPAND_SZ` | `My App` |
| `IconUri` | `REG_EXPAND_SZ` | `C:\icon.png` |
| `IconBackgroundColor` | `REG_SZ` | `AARRGGBB` |
| `CustomActivator` | `REG_SZ` | o GUID do ativador COM |

Pontos que a página **não** cobre:

- Ela usa `HKEY_LOCAL_MACHINE\Software\Classes\AppUserModelId\<AUMID>`, não `HKCU` [F1]. O `HKCU` (só para o usuário atual) aparece apenas nas implementações abaixo.
- Ela não menciona atalho, nem para dizer que ele é dispensável.
- Ela não mostra a chave `LocalServer32`, sem a qual o Windows não sabe que programa abrir no clique (ver seção 3).

Duas implementações gravam essas chaves em `HKCU` e funcionam sem atalho:

- **Windows App SDK** (código da Microsoft) [F27]. Grava em `HKCU\Software\Classes\AppUserModelId\<id>` os valores `DisplayName`, `IconUri` e `CustomActivator` (não grava `IconBackgroundColor`). Grava também `HKCU\Software\Classes\CLSID\{GUID}\LocalServer32` com `"<caminho do exe>" ----AppNotificationActivated:`. **Mas**, antes de gravar, ele chama a função interna `PushNotifications_RegisterFullTrustApplication` e se registra num serviço de longa duração do próprio SDK [F27]. Por isso ele não prova que as chaves sozinhas bastam. E ele mostra a notificação por uma API interna, não pelo `Windows.UI.Notifications` [F27].
- **Community Toolkit**, a biblioteca de C# que a Microsoft recomendava até 2026 [F4] (**terceiros**, repositório da organização CommunityToolkit). Grava as mesmas chaves em `HKCU`, mais `IconBackgroundColor = FFDDDDDD`, e a chave `LocalServer32` com `"<caminho do exe>" -ToastActivated` [F31]. Registra o objeto COM com `CoRegisterClassObject` e envia pelo próprio `ToastNotificationManager.CreateToastNotifier(aumid)` do `Windows.UI.Notifications` [F31]. Ou seja, faz exatamente o que o ScreenGauge faria, sem atalho. É o indício mais forte de que o caminho do registro funciona com a API que o ScreenGauge usaria, mas é indício, não documentação.

O que está documentado, o que só está em código e o que é contraditório:

| Afirmação | Situação |
| --- | --- |
| Precisa de atalho no Iniciar com AUMID | Documentado e no ar [F6]; texto antigo (data da página: 2017) |
| Basta o registro em `AppUserModelId`, sem atalho | Documentado só em página apagada [F1], e com `HKLM`; usado em `HKCU` pelo Windows App SDK e pelo Toolkit [F27][F31] |
| O registro em `HKCU` basta sem a função interna do Windows App SDK | Só o Toolkit indica que sim [F31] (**terceiros**); **não confirmado** pela Microsoft |
| O caminho do registro é "não suportado" | Nenhuma fonte diz isso |

### 1.3 `SetCurrentProcessExplicitAppUserModelID`

A documentação dessa função fala só da barra de tarefas: ela dá ao processo um AUMID que "agrupa seus processos e janelas sob um único botão da barra de tarefas" e precisa ser chamada "antes de o app mostrar qualquer interface" [F19]. Nada sobre notificações.

A página de AppUserModelIDs diz que, se o app usar um AUMID explícito, precisa aplicar o mesmo em todas as janelas, processos, atalhos e associações de arquivo [F18]. O formato é `NomeDaEmpresa.NomeDoProduto`, até 128 caracteres, sem espaços [F18]. A página apagada [F1] diz que, para notificações agendadas, o limite é 129 caracteres.

O Windows App SDK usa o AUMID do processo, se o app tiver chamado essa função, como o AUMID das notificações [F27]. **Dedução**: para o `Windows.UI.Notifications` a função não é necessária, porque o AUMID vai explícito em `CreateToastNotifierWithId`. Se o ScreenGauge a chamar, os ícones da barra de tarefas passam a se agrupar por esse AUMID; o atalho, se existir, deveria ter o mesmo.

## 2. Nome e ícone no cabeçalho e na central

No Windows 11, a "área de atribuição" no topo da notificação mostra o nome e o ícone do app, mais um botão de fechar e um menu "…" para desligar as notificações do app ou abrir as configurações dele. "A área de atribuição é configurada pelo shell e não pode ser trocada pelo conteúdo XML" [F10]. A imagem `appLogoOverride` não substitui mais o ícone do app no Windows 11; ela aparece num quadrado à esquerda do texto [F10].

| Caminho | Nome | Ícone | Fonte |
| --- | --- | --- | --- |
| Registro | `DisplayName` | `IconUri`: caminho de um arquivo local (exemplo `C:\icon.png`) | [F1] |
| Registro, como o Windows App SDK grava | Nome tirado do atalho ou, sem atalho, do nome do `.exe` com a primeira letra maiúscula | Imagem tirada do atalho ou do ícone do `.exe`, gravada como PNG em `%LOCALAPPDATA%\Microsoft\WindowsAppSDK\{id}.png`; aceita `.bmp`, `.ico`, `.jpg` e `.png` | [F27] |
| Registro, sem `IconUri` | `DisplayName` | "o sistema mostra um ícone padrão" (comentário no código) | [F27] |
| Atalho | Não documentado | Não documentado | — |
| Pacote MSIX | Não documentado | Não documentado | — |

Observações:

- O Windows App SDK diz que, sem pacote, "recursos como o nome e o ícone do app são obtidos do shell" [F24], e o código os lê do "melhor atalho" do AUMID [F27]. O Toolkit faz o mesmo, com uma imagem de 48×48 pixels [F31]. **Dedução**: pelo caminho do atalho, o Windows usa o nome e o ícone do atalho.
- `IconBackgroundColor`: o comentário no código do Toolkit diz que "a cor de fundo só aparece na página de configurações" [F31] (**terceiros**). A Microsoft não documenta o efeito.
- **Dedução** para o pacote: o nome e o ícone vêm de `DisplayName` e `Square44x44Logo` do `uap:VisualElements`, que a documentação descreve como o nome e o ícone da lista de apps [F17]. A página do elemento não menciona notificações.
- O ADR 0003 mediu que o balão mostra "ScreenGauge", lido do recurso de versão do `.exe`. Nada na documentação diz que o toast lê o recurso de versão; o Windows App SDK só usa o nome do arquivo `.exe` [F27].
- A nitidez do ícone em cada escala de tela não é documentada para `IconUri`.

## 3. O clique: no canto da tela e na central (Win + N)

### 3.1 O que a documentação diz

- O evento `ToastNotification.Activated` "ocorre quando o usuário ativa uma notificação com um clique ou toque. Apps que estão rodando assinam este evento" [F7]. A observação da página: "no caso de toast mostrado por um app de desktop, o app precisa assinar pelo menos o evento `Activated`" [F7].
- História, na página de C++ WRL: "O Windows 8 [...] tratava a ativação pelo evento `Activated` em memória, porque as notificações eram só balões passageiros que não ficavam guardados. O Windows 10 [...] introduziu a Central de Ações, onde as notificações ficam guardadas por vários dias. A Central exigiu um ativador COM, para que a notificação possa ser ativada dias depois de criada" [F2].
- Na mesma página: o ativador "é necessário para que a notificação fique guardada na Central de Ações" e, se as notificações "aparecem mas não ficam guardadas na Central (somem depois que o balão sai), você não implementou o ativador COM corretamente" [F2].
- Página "Activating app notifications from desktop apps" (apagada) [F3]. Com o ativador COM, a notificação fica guardada, pode ser clicada no balão e na central e funciona com o `.exe` fechado. A alternativa barata é um "CLSID falso" (qualquer GUID, sem servidor COM): a notificação fica guardada, mas, sem pacote, só funciona um clique que abra um endereço próprio do app (ativação por protocolo, como `screengauge:`). O CLSID falso "quebra a ativação de qualquer outra notificação" [F3].
- No exemplo oficial, com o ativador registrado, um clique ativa o objeto COM **e** dispara o `Activated`, e "não há garantia de qual acontece primeiro" [F28].
- `INotificationActivationCallback::Activate` é "chamado quando o usuário interage com um toast na central". Se o método devolver um erro, "a ativação falha e o usuário pode tentar ativar o app de novo" [F21].

### 3.2 O que acontece em cada caso

| Situação | Com ativador COM | Só com `Activated` |
| --- | --- | --- |
| App rodando, clique no balão do canto | O Windows chama `Activate` [F2]; o `Activated` também dispara, em ordem não garantida [F28] | O `Activated` dispara [F7] |
| App rodando, clique na central depois que o balão saiu | O Windows chama `Activate` [F2][F5] | **Não documentado**. **Dedução** pela história em [F2]: não funciona, e a notificação nem fica na central |
| App fechado, clique na central | O Windows abre o `.exe` pela chave `LocalServer32` e depois chama `Activate` [F2] | Impossível: o evento só existe dentro de um processo rodando [F7] |

Pontos que valem com o ativador:

- O `Activate` "é chamado numa linha de execução separada da principal" [F2] e, no Toolkit, "numa linha de fundo" [F4]. **Dedução**: o ScreenGauge teria de repassar o clique à janela residente por mensagem (`PostMessage`), como já faz com outros eventos.
- Com o app fechado, o Windows inicia o programa com o texto que está na chave `LocalServer32`. O `-ToastActivated` das páginas é só uma convenção do app, gravada nessa chave [F2][F16]. O próprio COM acrescenta `-Embedding` ao abrir um servidor COM: o exemplo de C++/WinRT, que grava o caminho puro, confere `argv[1] == "-Embedding"` [F5]. A linha de comando exata com os dois argumentos é **não confirmada**.
- Use o modelo `ToastGeneric`. Os modelos antigos do Windows 8 "não ativam o servidor COM" [F2].

### 3.3 O custo de registrar um ativador COM (cópia sem pacote)

1. **Um GUID fixo (o CLSID)**, escolhido uma vez e nunca mais trocado [F2].
2. **A chave** `HKCU\Software\Classes\CLSID\{CLSID}\LocalServer32`, com valor padrão `"<caminho do ScreenGauge.exe>"` mais um argumento opcional [F5][F27][F28][F31]. A página de C++ WRL fazia o próprio app gravar essa chave ao iniciar, "independente do instalador" [F2]; o exemplo oficial a grava no instalador [F28].
3. **Um ponteiro para o CLSID**: a propriedade `System.AppUserModel.ToastActivatorCLSID` no atalho [F2][F5][F28] **ou** o valor `CustomActivator` na chave `AppUserModelId` [F1].
4. **Dentro do programa:** um objeto que implementa `INotificationActivationCallback` e uma "fábrica" (`IClassFactory`) que o cria, registrados ao iniciar com `CoRegisterClassObject(CLSID, fábrica, CLSCTX_LOCAL_SERVER, REGCLS_MULTIPLEUSE)` [F5][F27][F31]. O registro é desfeito com `CoRevokeClassObject` ao sair [F22]. O Windows App SDK usa `REGCLS_MULTIPLEUSE` para que os cliques "sejam entregues dentro do processo" que está rodando; com `REGCLS_SINGLEUSE`, "o COM abre um processo novo do app a cada clique" [F27]. Ele soma `REGCLS_AGILE` porque linhas de execução STA (como a principal do ScreenGauge, que inicia o COM em modo `COINIT_APARTMENTTHREADED`) precisam dele "para que outras linhas possam ativar o objeto" [F27].
5. **Na partida a frio** (app fechado), o processo aberto pelo clique precisa registrar o objeto antes de qualquer outra coisa [F2]. **Dedução**: o código de instância única do ScreenGauge (`single_instance.rs`) não pode encerrar esse processo antes do registro, ou o clique se perde.

## 4. "Não perturbe", Focus e tela cheia

O que a Microsoft documenta para o Windows 11 [F12]:

- Ligar o "Não perturbe" "silencia todas as notificações até você desligá-lo".
- "Com o não perturbe ligado, você só recebe balões de alarmes, lembretes e dos apps que escolher. As outras notificações vão direto para a central de notificações até você desligá-lo."
- Ele pode ligar sozinho em situações que o usuário escolhe: em certos horários, ao duplicar a tela, durante um jogo, com um app em tela cheia e na primeira hora depois de uma atualização do Windows.

O que **não** está documentado:

- **Se o balão aparece quando o "Não perturbe" acaba.** A página só diz que a notificação vai para a central. O ticket escreve "espera o 'Não perturbe' acabar"; com base no texto, o certo é "fica na central, sem balão".
- **Quais das regras automáticas vêm ligadas no Windows 11.** A página de Focus diz que o "Focus assist" vem configurado para ligar ao duplicar a tela, durante jogos e com app em tela cheia [F13], mas o trecho aponta o caminho de configuração do Windows 10. **Não confirmado** para o Windows 11.
- **O que acontece com um toast quando `SHQueryUserNotificationState` devolve `QUNS_BUSY`** (o estado que o ADR 0003 mediu com a tela congelada na frente). A página da função só orienta apps que desenham seus próprios balões a não mostrá-los nesse estado [F26].
- **A notificação que chega com o app em segundo plano e o "Não perturbe" ligado conta como "vista"?** Não há API documentada que diga ao app se o usuário viu.

Opções que mudam esse comportamento, e por que **não** servem aqui:

| Opção | O que faz | Por que não usar |
| --- | --- | --- |
| `scenario="reminder"` | Fica na tela, aberta, até o usuário fechar; precisa de um botão [F10]. O esquema diz que é "ignorada em silêncio" sem um botão de ativação em segundo plano [F11] | Prende um aviso informativo na tela; e lembretes furam o "Não perturbe" [F12], o oposto do que o ticket quer |
| `scenario="alarm"` | Igual ao lembrete, com som de alarme em repetição [F10][F11] | Mesmo motivo, mais som |
| `scenario="incomingCall"` | Formato de chamada, toque em repetição, fica na tela [F10][F11] | Não é uma chamada |
| `scenario="urgent"` | Notificação "importante" que pode furar o "Não perturbe" se o usuário permitir; o Windows mostra uma notificação do sistema perguntando se o usuário permite [F10]. A página ainda diz "requer o Windows Insider Preview build 22546 ou posterior" [F10] | Faz o Windows perguntar ao usuário; e furar o "Não perturbe" não é o pedido |
| `Priority = High` | "Dica" sobre como e com que urgência mostrar; "se a notificação é mostrada em prioridade alta depende do estado e da política de energia do aparelho" [F8] | Sem efeito documentado sobre o "Não perturbe" |
| `SuppressPopup = true` | Põe a notificação direto na central, sem balão [F7][F14] | Útil só se o ScreenGauge quiser, de propósito, não mostrar o balão |

Nenhuma das páginas lidas diz que algum cenário exige uma "capability" especial. Para um `.exe` sem pacote, capabilities nem existem; para o pacote, **não confirmado**.

**Expiração.** "O prazo padrão e máximo de uma notificação é de 3 dias" [F9], escrito na página do Windows App SDK. **Dedução**: vale para o `Windows.UI.Notifications`, porque a central é a mesma. O app pode pedir, com a propriedade `ExpiresOnReboot`, que a notificação saia da central ao reiniciar [F7][F9]; o valor padrão dela não está escrito nas páginas lidas. Para o dono decidir: o critério do ticket "ficam na central até o usuário fechá-las" vale por no máximo 3 dias. E a marca "já avisei" do WPC-339 continua sem prova de que o usuário viu.

## 5. Cópia da Store (MSIX)

O manifesto do pacote precisa de [F2][F4]:

1. As declarações `xmlns:com="http://schemas.microsoft.com/appx/manifest/com/windows10"` e `xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10"`, as duas em `IgnorableNamespaces`.
2. `com:Extension Category="windows.comServer"` com `com:ComServer` → `com:ExeServer Executable="ScreenGauge.exe" Arguments="-ToastActivated" DisplayName="Toast activator"` → `com:Class Id="<CLSID>"`. O `Executable` é um caminho relativo à raiz do pacote e "precisa apontar para um arquivo do pacote"; o `ExeServer` "corresponde à chave `LocalServer32`" [F16].
3. `desktop:Extension Category="windows.toastNotificationActivation"` → `desktop:ToastNotificationActivation ToastActivatorCLSID="<CLSID>"` (GUID obrigatório, sem chaves) [F15].
4. O `uap:VisualElements` obrigatório de todo pacote (`DisplayName`, `Square44x44Logo` etc.) [F17]. A página dele não fala em notificações.

Trabalho no registro: **nenhum**. A função de registro da biblioteca de C++ WRL "não faz nada" num app empacotado [F2]. E "quando apps MSIX são desinstalados, todas as notificações e os recursos relacionados são apagados automaticamente" [F4]. O processo ainda precisa registrar o objeto COM ao iniciar, como na cópia sem pacote: "Para apps empacotados e sem pacote, você precisa registrar o tipo do ativador" [F2]. O Windows App SDK exige que "o processo que registra e o processo declarado no manifesto como servidor COM sejam o mesmo" [F24].

Outros pontos:

- Num app empacotado, o AUMID é o do pacote. O Windows App SDK o lê com `GetCurrentApplicationUserModelId` [F27].
- "Se você adicionar o ativador COM a um app empacotado, as ativações [...] vão para o ativador COM, não para a linha de comando" [F3].
- **Duas cópias no mesmo computador:** "Se você instalou tanto o app empacotado quanto o sem pacote, o empacotado toma o lugar do sem pacote ao tratar as ativações. As notificações do app sem pacote abrem o empacotado quando clicadas. Desinstalar o empacotado devolve as ativações ao sem pacote" [F2]. A página fala do mesmo app nas duas formas, provavelmente com o mesmo CLSID. **Dedução**: com CLSID e AUMID diferentes em cada cópia, o conflito pode não acontecer.

## 6. Cópia do instalador (sem pacote)

O que o instalador cria, em cada caminho documentado:

| Item | Caminho do atalho [F2][F5][F28] | Caminho do registro [F1][F27][F31] |
| --- | --- | --- |
| Atalho `.lnk` no menu Iniciar com `System.AppUserModel.ID` | Sim | Não |
| Propriedade `System.AppUserModel.ToastActivatorCLSID` no atalho | Sim | Não |
| `HKCU\Software\Classes\AppUserModelId\<AUMID>` com `DisplayName`, `IconUri`, `CustomActivator` (e `IconBackgroundColor`) | Não | Sim (a página usa `HKLM`) |
| Arquivo de ícone para o `IconUri` | Não | Sim, se quiser um ícone próprio |
| `HKCU\Software\Classes\CLSID\{CLSID}\LocalServer32` | Sim | Sim |

**Dedução**: no caminho do registro, quem grava as chaves pode ser o próprio ScreenGauge, a cada partida, como fazem o Windows App SDK e o Toolkit [F27][F31]. Assim as chaves acompanham o caminho atual do `.exe` mesmo se a pasta mudar. O exemplo oficial avisa que "não limpa o atalho nem o registro COM" [F28].

Na desinstalação:

- Apagar o atalho, as chaves `AppUserModelId\<AUMID>` e `CLSID\{CLSID}` e o arquivo de ícone. O `UnregisterAll` do Windows App SDK faz exatamente isso [F27]. A Microsoft orienta chamá-lo "se o app não vai mais usar notificações" [F24]. Um comentário no código admite que sobram ícones "de apps desinstalados que não chamaram `UnregisterAll`" [F27].
- **O que acontece com as notificações que já estão na central depois de desinstalar a cópia sem pacote: não documentado.** O Toolkit, antes de apagar as chaves, limpa as agendadas e as da central (`History.Clear()`). Seu comentário diz que isso "precisa ser chamado do `.exe` principal do app, não de um `.exe` desinstalador separado" [F31] (**terceiros**).
- A API `ToastNotificationHistory.Clear(applicationId)` existe. A documentação a descreve só para "outro app dentro do mesmo pacote" [F23]. **Não confirmado**: se funciona com o AUMID de um app sem pacote, chamada por outro processo.

## 7. A crate `windows` (versão 0.62.2)

A versão está fixada em `windows = "=0.62.2"` no `Cargo.toml` da raiz e em `Cargo.lock`. Os nomes abaixo foram lidos do `Cargo.toml` e do código-fonte dessa versão exata [F29]:

| O que | Feature | Onde está no código da crate |
| --- | --- | --- |
| `ToastNotificationManager`, `ToastNotifier`, `ToastNotification`, eventos `Activated`/`Dismissed`/`Failed`, `ToastActivatedEventArgs::Arguments`, `ToastNotificationHistory`, `SetPriority`, `SetSuppressPopup`, `SetExpiresOnReboot`, `NotificationSetting` | `UI_Notifications` (puxa `UI` e `Foundation`) | `src/Windows/UI/Notifications/mod.rs` |
| `ToastNotificationManager::CreateToastNotifierWithId(&HSTRING)` | `UI_Notifications`, sem outra exigência | mesmo arquivo, linha 3197 |
| `ToastNotification::CreateToastNotification(&XmlDocument)` (o construtor), `GetTemplateContent`, `Content` | `UI_Notifications` **e** `Data_Xml_Dom` | o construtor e os outros dois têm `#[cfg(feature = "Data_Xml_Dom")]` |
| `XmlDocument`, `LoadXml` | `Data_Xml_Dom` (puxa `Data_Xml`, `Data`, `Foundation`) | `src/Windows/Data/Xml/Dom/mod.rs` |
| `ToastActivatedEventArgs::UserInput` (só para campos de texto) | `Foundation_Collections` | `#[cfg(feature = "Foundation_Collections")]` |
| `ToastNotificationHistory::ClearWithId`, `RemoveGroupWithId` | `UI_Notifications` | mesmo arquivo, linhas 3110–3130 |
| `INotificationActivationCallback`, `INotificationActivationCallback_Impl`, `NOTIFICATION_USER_INPUT_DATA` | `Win32_UI_Notifications` | `src/Windows/Win32/UI/Notifications/mod.rs` |
| `CoRegisterClassObject`, `CoRevokeClassObject`, `IClassFactory_Impl`, `CLSCTX`, `REGCLS` | `Win32_System_Com` (já no projeto) | `src/Windows/Win32/System/Com/mod.rs` |
| `SetCurrentProcessExplicitAppUserModelID`, `ShellLink`, `IShellLinkW` | `Win32_UI_Shell` (já no projeto) | `src/Windows/Win32/UI/Shell/mod.rs` |
| `IPropertyStore` (só se o próprio app criar o atalho) | `Win32_UI_Shell_PropertiesSystem` | `src/Windows/Win32/UI/Shell/PropertiesSystem/mod.rs` |
| `PKEY_AppUserModel_ID`, `PKEY_AppUserModel_ToastActivatorCLSID` (idem) | `Win32_Storage_EnhancedStorage` | `src/Windows/Win32/Storage/EnhancedStorage/mod.rs` |
| `RegCreateKeyExW`, `RegSetValueExW`, `RegDeleteTreeW` | `Win32_System_Registry` (já no projeto) | — |

O objeto COM do ativador precisa da macro `#[implement]`:

- A `windows-core` 0.62.2 depende da `windows-implement` 0.60.2 de forma **não opcional** e a reexporta como `windows::core::implement` [F29].
- O código gerado pela macro usa caminhos absolutos `::windows_core::...` (`windows-implement-0.60.2/src/gen.rs`) [F29].
- Os exemplos do próprio repositório da `windows-rs` que usam `#[implement]` declaram `windows-core` como dependência direta [F30].
- **Dedução**: o ScreenGauge precisaria acrescentar `windows-core = "=0.62.2"` ao `Cargo.toml`. A crate já está no `Cargo.lock` como dependência da `windows`, mas a linha nova é uma dependência direta e precisa da aprovação do dono. A alternativa é escrever a tabela de funções (vtable) à mão, com mais `unsafe`.
- O projeto não usa `#[implement]` hoje.

Aviso: a branch principal da `windows-rs` mudou o nome das features para nomes de cabeçalhos do C (`combaseapi`, `shellapi` etc.) [F30]. Documentação e exemplos dessa branch não valem para a 0.62.2.

## 8. O que o Windows 11 muda em relação ao Windows 10

- O topo da notificação mostra o nome e o ícone do app, configurados pelo Windows, e o `appLogoOverride` não os substitui mais [F10].
- A "Central de Ações" do Windows 10 virou a "central de notificações"; a documentação recente usa "Notification Center" [F9][F14].
- O "Focus assist" virou "Não perturbe", e o Windows 11 tem as "Focus sessions", que ligam o "Não perturbe" [F12][F13]. A API `FocusSessionManager` diz ao app se há uma sessão de Focus ativa [F14].
- O cenário `urgent` ("notificações importantes", que podem furar o "Não perturbe" com permissão do usuário) é do Windows 11 [F10].
- Botões coloridos e dicas de ferramenta nos botões são do Windows 11 [F10]; o ScreenGauge não precisa deles.
- Nenhuma das páginas lidas diz que o registro do AUMID, o atalho ou o ativador COM mudaram no Windows 11. A única mudança achada é antiga e não pesa aqui: o Toolkit evita barras invertidas no AUMID por um defeito "em versões 19042 e anteriores do Windows 10" [F31].

## 9. O que a documentação deixa aberto (o teste no computador do dono precisa checar)

Cada item é uma pergunta com resposta observável, num `.exe` sem pacote de teste, no Windows 11 do dono:

1. **Registro sem atalho:** com só `HKCU\Software\Classes\AppUserModelId\<AUMID>` (`DisplayName`, `IconUri`, `CustomActivator`) e `HKCU\Software\Classes\CLSID\{CLSID}\LocalServer32`, sem atalho no Iniciar, a notificação aparece? (Fontes em conflito: [F1] contra [F6].)
2. **Guardada na central:** a mesma notificação continua na central (Win + N) depois que o balão sai? Repetir em três variações: (a) sem `CustomActivator`; (b) com `CustomActivator` apontando para um CLSID sem `LocalServer32`; (c) completo.
3. **`HKCU` contra `HKLM`:** a página usa `HKLM` [F1]. Com `HKCU` funciona igual?
4. **Nome e ícone:** que nome e ícone aparecem no topo e na central em cada caminho (registro com `IconUri` em `.png` e em `.ico`; só atalho; sem `IconUri`)? Quantos pixels do ícone mostrado batem com o desenho, em 100% e 150% de escala? O recurso de versão "ScreenGauge" entra em algum lugar? Onde aparece a cor de `IconBackgroundColor`?
5. **Clique com o app rodando:** clicar no balão do canto e clicar na central depois que o balão saiu. Em cada caso: o `Activate` é chamado dentro do processo que já roda? O `Activated` dispara? Em que ordem chegam os dois?
6. **Só `Activated`, sem servidor COM:** com o app rodando, o `Activated` ainda chega num clique vindo da central depois que o balão saiu? (Provável que não, pela história em [F2], mas não está escrito.)
7. **Só `CoRegisterClassObject` com `REGCLS_MULTIPLEUSE`, sem a chave `LocalServer32`:** com o app rodando, o clique na central chega?
8. **App fechado:** com que linha de comando o Windows abre o `ScreenGauge.exe` (`-ToastActivated`, `-Embedding`, os dois)? O processo de instância única registra o objeto COM a tempo, ou sai antes e o clique se perde? E com uma cópia já rodando: o Windows usa a que roda ou abre outra?
9. **"Não perturbe" ligado à mão:** a notificação vai para a central sem balão ([F12])? Quando o "Não perturbe" desliga, o balão aparece ou não?
10. **Tela cheia e tela congelada (`QUNS_BUSY`):** com a tela congelada do ScreenGauge na frente e com um jogo ou vídeo em tela cheia, a notificação vai para a central, aparece por cima ou some? Anotar se o "Não perturbe" automático estava ligado nas configurações do Windows naquele momento, e qual era o estado de fábrica de cada regra automática.
11. **Expiração:** a notificação ainda está na central depois de 3 dias ([F9])? E depois de reiniciar, sem `ExpiresOnReboot`?
12. **Desinstalação da cópia sem pacote:** depois de apagar as chaves (e o atalho), as notificações que estavam na central continuam lá? Clicar numa delas faz o quê? `ToastNotificationHistory::ClearWithId(<AUMID>)` chamado por outro processo (o desinstalador) as remove?
13. **Duas cópias instaladas** (pacote MSIX de teste e `.exe` sem pacote), com AUMID e CLSID diferentes: o clique numa notificação da cópia sem pacote abre a cópia sem pacote ou a empacotada ([F2])? Repetir com o mesmo CLSID.
14. **Pacote MSIX** (com o WPC-334): com só o manifesto da seção 5, a notificação aparece, fica na central e o clique chega, com o app rodando e fechado? Que nome e ícone aparecem? Desinstalar o pacote apaga as notificações da central ([F4])?
15. **Compilação:** o objeto COM feito com `#[implement]` compila sem `windows-core` como dependência direta? (Se não compilar, isso confirma a dedução da seção 7 e o dono decide sobre a dependência.)

## Fontes

Microsoft, páginas apagadas de learn.microsoft.com e lidas no histórico público da documentação (`github.com/MicrosoftDocs/windows-dev-docs`):

- **F1** — "Send a local app notification from other types of unpackaged apps" (`ms.date` 2025-07-28; apagada em 2026-04-08, commit `6c2851bf3fe91cd1325a88a6c8712884a3c73bf1`). Versão lida no commit `bcb5b837219af0840f9b6f18a399ebda28be5714`: <https://github.com/MicrosoftDocs/windows-dev-docs/blob/bcb5b837219af0840f9b6f18a399ebda28be5714/hub/apps/develop/notifications/app-notifications/app-notifications-other-apps.md>. A mudança de pasta de 2025-07-28 (commit `a2477a927c72cdbaa53b73b2cabe2cf71b7c2864`) e a mudança de nome de 2026-04-08 (commit `a5bec3dae597019cfa84aa4f7d30ecebe315fe29`) estão no mesmo histórico.
- **F2** — "Send a local app notification from a WRL C++ desktop app" (`ms.date` 2025-07-28), versão completa de antes do resumo de 2026-04-08 (commit `296d5307447655ed76e273ee211ea75633dfba79`); apagada no commit `6c2851bf…`. Lida no commit `f0e37753c820347f99ada6b4c326279052b74a84`: <https://github.com/MicrosoftDocs/windows-dev-docs/blob/f0e37753c820347f99ada6b4c326279052b74a84/hub/apps/develop/notifications/app-notifications/send-local-toast-desktop-cpp-wrl.md>
- **F3** — "Activating app notifications from desktop apps" (`ms.date` 2025-02-27; aposentada em 2026-04-09, commit `77438c77c1abbe3e4ec15c829a0b22767d72e48e`). Lida no commit `0444d723df84bdeb4acb8d146167d28bad3c0197`: <https://github.com/MicrosoftDocs/windows-dev-docs/blob/0444d723df84bdeb4acb8d146167d28bad3c0197/hub/apps/develop/notifications/app-notifications/toast-desktop-apps.md>
- **F4** — "Send a local app notification from a C# app" (`ms.date` 2026-04-01; apagada em 2026-04-08). Lida no commit `bcb5b837…`: <https://github.com/MicrosoftDocs/windows-dev-docs/blob/bcb5b837219af0840f9b6f18a399ebda28be5714/hub/apps/develop/notifications/app-notifications/app-notifications-csharp-legacy.md>

Microsoft, learn.microsoft.com e support.microsoft.com (no ar em 2026-11-02):

- **F5** — Author COM components with C++/WinRT (`ms.date` 2026-07-02), seção do exemplo `ToastAndCallback`: <https://learn.microsoft.com/en-us/windows/apps/develop/cpp-winrt/author-coclasses>
- **F6** — ToastNotificationManager.CreateToastNotifier: <https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotificationmanager.createtoastnotifier>
- **F7** — ToastNotification (classe) e ToastNotification.Activated: <https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification> e <https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification.activated>
- **F8** — ToastNotification.Priority: <https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification.priority>; ToastNotification.ExpirationTime: <https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification.expirationtime>
- **F9** — Remove and Manage App Notifications (prazo de 3 dias, `ExpiresOnReboot`): <https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/manage-app-notifications>
- **F10** — App notification content (área de atribuição no Windows 11, `appLogoOverride`, cenários, notificações importantes; `ms.date` 2026-07-15): <https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-content>
- **F11** — Esquema XML do toast, elemento `toast` (atributos `scenario` e `duration`): <https://learn.microsoft.com/en-us/uwp/schemas/tiles/toastschema/element-toast>
- **F12** — Notifications and Do Not Disturb in Windows (suporte, aba Windows 11): <https://support.microsoft.com/en-us/windows/notifications-and-do-not-disturb-in-windows-feeca47f-0baf-5680-16f0-8801db1a8466>
- **F13** — Focus: Stay on Task Without Distractions in Windows (suporte): <https://support.microsoft.com/en-us/windows/focus-stay-on-task-without-distractions-in-windows-cbcc9ddb-8164-43fa-8919-b9a2af072382>
- **F14** — Notifications design basics (central de notificações, notificação sem balão, Focus sessions): <https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-ux-guidance>
- **F15** — Manifesto MSIX, `desktop:ToastNotificationActivation`: <https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-desktop-toastnotificationactivation>
- **F16** — Manifesto MSIX, `com:ExeServer`: <https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-com-exeserver>
- **F17** — Manifesto MSIX, `uap:VisualElements`: <https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap-visualelements>
- **F18** — Application User Model IDs (AppUserModelIDs): <https://learn.microsoft.com/en-us/windows/win32/shell/appids>
- **F19** — SetCurrentProcessExplicitAppUserModelID: <https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-setcurrentprocessexplicitappusermodelid>
- **F20** — How to enable desktop toast notifications through an AppUserModelID (arquivada, 2018): <https://learn.microsoft.com/en-us/previous-versions/windows/desktop/legacy/hh802762(v=vs.85)>
- **F21** — INotificationActivationCallback::Activate: <https://learn.microsoft.com/en-us/windows/win32/api/notificationactivationcallback/nf-notificationactivationcallback-inotificationactivationcallback-activate>
- **F22** — CoRegisterClassObject: <https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-coregisterclassobject>
- **F23** — ToastNotificationHistory.Clear: <https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotificationhistory.clear>
- **F24** — AppNotificationManager.Register e AppNotificationManager.UnregisterAll (Windows App SDK): <https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.windows.appnotifications.appnotificationmanager.register> e <https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.windows.appnotifications.appnotificationmanager.unregisterall>
- **F25** — Use app notifications with a console app (`ms.date` 2026-04-08) e App Notifications for WPF, WinForms, Console, and UWP: <https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-console> e <https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-other-frameworks>. Conteúdo atual da pasta, commit `e25c65d9528b7bbbedce2325e80c8079c5e4e496` (2026-05-19): <https://github.com/MicrosoftDocs/windows-dev-docs/tree/e25c65d9528b7bbbedce2325e80c8079c5e4e496/hub/apps/develop/notifications/app-notifications>
- **F26** — SHQueryUserNotificationState: <https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shqueryusernotificationstate>
- **F32** — App Notifications Overview (`ms.date` 2026-10-13; limitação de apps elevados): <https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/>

Microsoft, código-fonte:

- **F27** — Windows App SDK, `dev/AppNotifications/AppNotificationUtility.cpp`, `AppNotificationUtility.h`, `AppNotificationManager.cpp` e `ShellLocalization.cpp`, lidos no commit `32dbd99499d1554d5d4ed5d512311d1339833740` (2026-10-30): <https://github.com/microsoft/WindowsAppSDK/tree/32dbd99499d1554d5d4ed5d512311d1339833740/dev/AppNotifications>
- **F28** — Windows-classic-samples, exemplo `DesktopToasts` (`CPP/DesktopToastsSample.cpp`, `CPP/Product.wxs`), lido no commit `434f6002bdf9cf9829406c3ff2b33387982d6168`: <https://github.com/microsoft/Windows-classic-samples/tree/434f6002bdf9cf9829406c3ff2b33387982d6168/Samples/DesktopToasts>
- **F29** — Crate `windows` 0.62.2 (`Cargo.toml` e `src/Windows/...`), `windows-core` 0.62.2 (`Cargo.toml`) e `windows-implement` 0.60.2 (`src/gen.rs`), lidas na cópia local do registro do Cargo que corresponde ao `Cargo.lock` do projeto; a mesma versão está em <https://docs.rs/windows/0.62.2> e <https://crates.io/crates/windows/0.62.2>
- **F30** — windows-rs, exemplo `crates/samples/windows/core_app/Cargo.toml` (declara `windows-core` direto para usar `#[implement]`), commit `73c74c0d79282e1295af1856e6c8987c51b7ccdd` da branch principal (2026-11-01): <https://github.com/microsoft/windows-rs/tree/73c74c0d79282e1295af1856e6c8987c51b7ccdd/crates/samples/windows/core_app>

Terceiros (indício, não prova):

- **F31** — Community Toolkit, `Microsoft.Toolkit.Uwp.Notifications/Toasts/Compat/ToastNotificationManagerCompat.cs` e `Desktop/Win32AppInfo.cs`, commit `c8f76d072df53d3622fb5440d63afb06cb9e7a10` (2024-10-31). É a biblioteca que a documentação da Microsoft recomendava para C# até 2026 [F4], mantida fora da organização `microsoft`: <https://github.com/CommunityToolkit/WindowsCommunityToolkit/tree/c8f76d072df53d3622fb5440d63afb06cb9e7a10/Microsoft.Toolkit.Uwp.Notifications/Toasts/Compat>
