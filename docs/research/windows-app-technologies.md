# Tecnologias para construir o Pixmeter (régua de tela para Windows 11)

Pesquisa do ticket WPC-266. Fontes consultadas em **26/10/2026**, todas primárias: documentação oficial, páginas de política de suporte, repositórios e issues dos próprios projetos. Cada afirmação traz o número da fonte entre colchetes; a lista completa está no fim. O que não pude confirmar está marcado como **não confirmado**. Este documento compara e aponta finalistas; **a escolha fica para outro ticket**.

## Resumo

1. Captura de tela, leitura de pixels, atalhos globais e ícone na bandeja são recursos do próprio Windows, ao alcance de qualquer tecnologia que fale direto com ele [[31]][31] [[35]][35] [[73]][73]. O que separa as opções é o peso da instalação, a nitidez em monitores de escalas diferentes e o trabalho das telas de configuração com tema claro/escuro.
2. A Microsoft já fez esta ferramenta: a Régua de Tela do PowerToys desenha a camada em C++ puro (Win32, Direct2D, Windows.Graphics.Capture) e usa WinUI 3 só na barra de ferramentas; código aberto, MIT [[37]][37] [[39]][39] [[40]][40].
3. Descartados: Electron (runtime de 158 MB, suporte de ~6 meses por versão), Flutter (janelas múltiplas experimentais), Qt, WinForms e Avalonia. O WPF ficou de fora por pouco: exige o .NET (60 MB) ou uma cópia inteira dele, e tem bugs abertos de posição de janela na escala por monitor [[3]][3] [[4]][4] [[25]][25].
4. **Finalistas.** A e B desenham a camada em código nativo, como o PowerToys; o que muda entre eles são as telas de configuração. C tem dois caminhos para a camada, ambos com riscos em aberto:
   - **(A) Nativo puro (Rust ou C++):** o mais leve e com controle total; telas e tema escuro feitos à mão.
   - **(B) Híbrido estilo PowerToys (núcleo nativo + WinUI 3):** visual do Windows 11 pronto; duas linguagens e app maior.
   - **(C) Tauri 2 (Rust + telas web no WebView2 do Windows 11):** leve e fácil de fazer com IA; duas linguagens. A camada em janela Tauri esbarra num bug aberto de tamanho entre monitores; em janela nativa ao lado do Tauri, a viabilidade não foi confirmada.

## Tabela 1 — Fatos de versão, suporte e instalação

Tamanhos: só entram números oficiais ou medidos no servidor oficial de download (pelo tamanho do arquivo informado no download, em 26/10/2026). **Nenhum app de exemplo foi compilado**: o tamanho final do Pixmeter em cada tecnologia é **não confirmado**.

| Tecnologia | Versão estável atual | Suporte anunciado pelo fornecedor | O que o usuário precisa ter instalado | Tamanho com fonte |
|---|---|---|---|---|
| **WinUI 3 / Windows App SDK** (C# ou C++) | Windows App SDK 2.5.1, de 16/09/2026; a linha 2.0 saiu em 29/04/2026 [[6]][6] | A linha 2.0 recebe correções até 29/04/2027. Sai uma versão principal no máximo a cada 6 meses, e só a última correção tem suporte. Segue o Modern Lifecycle da Microsoft [[6]][6]. Mudanças que quebram compatibilidade só acontecem em versões principais; a próxima será a 3.0 [[7]][7] | Sem pacote: o Windows App Runtime mais o Visual C++ Redistributable [[10]][10]. Ou o app leva o runtime dentro de si (modo "autocontido"), o que deixa a pasta "significativamente maior" [[9]][9]. Com C#, também o .NET 10 ou uma cópia dele [[8]][8] | Instalador do Windows App Runtime 2.5.1 x64: **120,2 MB** [[11]][11]. App autocontido: não confirmado |
| **WPF** (.NET 10) | .NET 10.0.14, de 25/10/2026 [[1]][1] | .NET 10 é LTS, com suporte até 14/11/2028. O .NET 8 e o 9 acabam em 10/11/2026 [[1]][1] | .NET Desktop Runtime 10 [[2]][2], ou o app leva uma cópia do .NET. O WPF não pode ser "enxugado" (trimming), então a cópia vai inteira [[4]][4] | Instalador do Desktop Runtime 10.0.14 x64: **60,0 MB** (zip: 39,2 MB) [[3]][3] |
| **WinForms** (.NET 10) | igual ao WPF [[1]][1] | igual ao WPF [[1]][1] | igual ao WPF; o trimming também está desativado para WinForms [[4]][4] | igual ao WPF [[3]][3] |
| **Tauri 2** (Rust + interface web) | tauri 2.12.5, de 21/10/2026. A 3.0 está em alfa [[42]][42] | Sem datas. A política de segurança diz apenas "versões > 1.0 com suporte" [[43]][43]. O projeto é mantido por uma fundação comunitária (Tauri Programme, dentro da Commons Conservancy) [[41]][41] | WebView2, que "é distribuído como parte do sistema operacional" no Windows 10 (desde abril de 2018) e no Windows 11 [[44]][44]. A Microsoft recomenda verificar mesmo assim [[49]][49] | Sem número oficial; a documentação diz apenas "binários muito pequenos" [[45]][45]. Não confirmado |
| **Electron** | 45.0.0, publicado em 21/10/2026 [[52]][52] | Só as 3 versões principais mais recentes têm suporte, e sai uma nova a cada 8 semanas [[50]][50]. O fim previsto da 45 é 27/04/2027 [[51]][51] | Nada além do app: o Chromium vai dentro dele | Runtime Windows x64 (zip) da 45.0.0: **158,2 MB** [[52]][52] |
| **Rust direto no Win32/Direct2D** (crates `windows`) | Crates focadas 0.100.0, de 03/09/2026. A crate "guarda-chuva" `windows` está na 0.62.2, de 06/10/2025 [[59]][59] | Nenhuma política de suporte encontrada (não confirmado). O projeto está na organização da Microsoft no GitHub [[57]][57]. Versões abaixo de 1.0 | Nenhum framework. A dependência do runtime do Visual C++ pode ser evitada com link estático: não confirmado | Não confirmado |
| **C++ direto no Win32/Direct2D** | Faz parte do Windows SDK; não há versão própria | Nenhum cronograma próprio encontrado; Win32 e Direct2D fazem parte do Windows (não confirmado) | Nenhum framework. Visual C++ Redistributable ou link estático: não confirmado | Não confirmado |
| **Avalonia** (.NET) | 12.1.4, de 09/10/2026, e 11.3.22, de 11/09/2026 [[60]][60] | A página de suporte diz "Avalonia 12.0 é uma versão LTS, com cinco anos de atualizações de segurança" e, em outro trecho, lista "versões LTS com cinco anos de atualizações de segurança" entre os benefícios do plano pago (Enterprise). Para quem não paga, cita "correções de segurança na versão atual" [[63]][63]. Se o LTS vale para quem não paga: não confirmado | .NET (mínimo .NET 8, recomendado .NET 10) [[62]][62], ou uma cópia dele dentro do app | Não confirmado |
| **Flutter** | Documentação reflete a 3.47 [[67]][67] | Não confirmado | Não confirmado | Não confirmado |
| **Qt 6** | 6.12.0, e 6.8.8 na linha LTS [[69]][69] | 6.8 LTS até 08/10/2029; 6.12 até 30/09/2031 [[69]][69]. O LTS de 5 anos é comercial. A edição gratuita recebe correções só até a próxima versão menor (cerca de 6 meses); depois, com 12 meses de atraso [[70]][70] | As bibliotecas do Qt vão dentro do app (não confirmado) | Não confirmado |

## Tabela 2 — Os quatro critérios do dono

Notas: **Bom / Médio / Fraco** são uma avaliação minha sobre os fatos citados. O critério 2 (manter com ajuda de IA) não tem fonte primária. Ele foi avaliado por sinais observáveis: quantas linguagens o projeto teria, quanto código "de ligação" com o Windows seria escrito à mão, quantas vezes a tecnologia muda de forma incompatível e se o compilador pega erros antes de rodar.

| Tecnologia | 1. Moderno no Windows 11 (nitidez, multi-monitor, tema) | 2. Manter com IA (avaliação) | 3. Futuro / risco de abandono | 4. Leve e independente | Camada transparente por monitor |
|---|---|---|---|---|---|
| **WinUI 3** | Telas: **Bom** — segue o tema do sistema sozinho [[36]][36] e tem localização embutida [[20]][20]. Camada: **Fraco** — veja a última coluna | **Médio**: C# ajuda, mas a camada e a captura exigem muita ligação manual com o Win32 | **Bom**, com ressalva: lacunas antigas seguem abertas (bandeja desde 2021 [[18]][18]; transparência desde 2019 [[13]][13]) | **Fraco/Médio**: runtime de 120 MB ou app autocontido maior [[9]][9] [[11]][11] | Sem suporte nativo: pedido aberto desde 2019 [[13]][13]. Contorno pela biblioteca comunitária WinUIEx [[13]][13]. Bug aberto: abrir uma janela WinUI faz janelas "sempre no topo" de outros programas perderem essa condição [[14]][14] |
| **WPF** | **Médio**: o tema Fluent (claro/escuro) chegou no .NET 9 [[22]][22], mas "ainda está em andamento" no .NET 10 [[23]][23]. Bugs abertos de posição e de área de trabalho no modo por monitor [[25]][25] [[26]][26] | **Bom**: uma linguagem (C#), tecnologia madura | **Bom**: segue o .NET (LTS até 11/2028) e recebeu novidades no .NET 10 [[1]][1] [[23]][23] | **Médio/Fraco**: .NET instalado (60 MB) ou cópia inteira sem trimming [[3]][3] [[4]][4] | Suportada (`AllowsTransparency`, exige janela sem borda) [[24]][24] |
| **WinForms** | **Fraco/Médio**: o modo escuro ficou estável no .NET 10 [[28]][28], mas a escala por monitor é "limitada, para alguns controles" [[21]][21], com bugs abertos em monitor secundário [[29]][29] | **Bom**: C#, simples | **Bom**: segue o .NET [[1]][1] | **Médio/Fraco**: igual ao WPF [[3]][3] [[4]][4] | Não confirmado |
| **Tauri 2** | **Médio**: telas web, com aparência do jeito que forem desenhadas. Bug aberto de tamanho de janela entre monitores com escalas diferentes no Windows [[48]][48] | **Médio**: duas linguagens (Rust e web); a parte web é fácil para IA | **Médio**: fundação comunitária sem datas de suporte [[41]][41] [[43]][43]; a 3.0 já em alfa indica uma migração futura [[42]][42] | **Bom**: WebView2 já vem no Windows 11 [[44]][44] [[49]][49] | Janela transparente no WebView, ou desenho nativo em Rust: detalhes de nitidez em escala fracionária não confirmados |
| **Electron** | **Médio/Fraco**: bug de posição e tamanho no modo por monitor aberto desde 2017 [[56]][56] | **Médio**: só JavaScript/TypeScript, mas uma versão nova a cada 8 semanas [[50]][50] | **Médio**: muito ativo, mas obriga a atualizar a cada poucos meses [[50]][50] [[51]][51] | **Fraco**: 158 MB [[52]][52] | Janela transparente sem redimensionar [[54]][54]; pode ignorar o mouse e ficar acima da barra de tarefas [[55]][55] |
| **Rust direto** | Camada: **Bom** (controle total da escala) [[21]][21]. Telas: **Fraco** — tema escuro e controles feitos à mão [[36]][36] | **Médio**: compilador rígido e uma linguagem, mas muito código de ligação com o Windows e crates que ainda mudam (abaixo de 1.0) [[57]][57] [[59]][59] | **Médio/Bom**: as APIs são do Windows; as crates não têm política de suporte publicada | **Bom** (tamanho não medido) | Total controle (Win32 + Direct2D) |
| **C++ direto** | Igual ao Rust; é o caminho usado pela Microsoft na Régua do PowerToys [[37]][37] [[38]][38] | **Médio/Fraco**: o compilador não pega erros de memória (avaliação) | **Bom**: as APIs são do próprio Windows (cronograma formal não confirmado) | **Bom** (tamanho não medido) | Total controle (Win32 + Direct2D) |
| **Avalonia** | **Médio**: bugs abertos com monitores de escalas diferentes [[65]][65] | **Médio/Bom**: C# | **Médio**: LTS de 5 anos anunciado, mas sem confirmação de que vale para quem não paga [[63]][63] | **Médio** (tamanho não confirmado) | Não confirmado |
| **Flutter** | **Fraco** para o Pixmeter: as APIs de janelas "não são ainda para uso em produção" e só existem no canal de desenvolvimento [[66]][66] | Não avaliado | Não confirmado | Não confirmado | Precisa de várias janelas → **não atende hoje** |
| **Qt 6** | **Médio/Bom** em escala: Per-Monitor V2 por padrão, com escalas fracionárias [[68]][68]. Porém, a escala pode deixar "buracos" na geometria da área de trabalho entre monitores [[68]][68], o que pesa para posicionar uma camada por monitor. Bugs conhecidos não pesquisados | **Médio**: C++/QML | **Médio**: LTS comercial; a edição gratuita tem correções curtas [[70]][70] | **Médio** (não confirmado) | Não confirmado |

### Requisitos funcionais por tecnologia (resumo)

| Tecnologia | Bandeja | Atalhos globais | Captura de pixels | Dois idiomas |
|---|---|---|---|---|
| WinUI 3 | Sem API própria; pedido aberto desde 2021 [[18]][18] [[19]][19]. Usar `Shell_NotifyIcon` [[73]][73] | `RegisterHotKey` [[35]][35] | APIs do Windows [[31]][31] [[34]][34] | MRT Core (arquivos de recursos por idioma) [[20]][20] |
| WPF / WinForms | Componente próprio: não confirmado; `Shell_NotifyIcon` funciona [[73]][73] | `RegisterHotKey` [[35]][35] | APIs do Windows [[31]][31] [[34]][34] | Arquivos `.resx` do .NET [[30]][30] |
| Tauri 2 | Recurso do núcleo (`tray-icon`) [[46]][46] | Plugin oficial `global-shortcut` [[47]][47] | Nenhuma API do Tauri encontrada (não confirmado); código Rust direto no Windows | Sem mecanismo próprio encontrado (não confirmado) |
| Electron | `Tray` [[75]][75] | `globalShortcut`, funciona sem foco [[74]][74] | `desktopCapturer` [[53]][53]; precisão de pixel não documentada | Sem mecanismo próprio encontrado (não confirmado) |
| Rust / C++ direto | `Shell_NotifyIcon` [[73]][73] | `RegisterHotKey` [[35]][35] | APIs do Windows [[31]][31] [[34]][34] | Feito no próprio app (não confirmado) |
| Avalonia | `TrayIcon` com suporte completo no Windows [[64]][64] | `RegisterHotKey` via .NET [[35]][35] | APIs do Windows [[31]][31] [[34]][34] | `.resx` do .NET [[30]][30]; integração com o Avalonia não confirmada |
| Qt 6 | `QSystemTrayIcon` [[76]][76] | Não confirmado | Não confirmado | Qt Linguist (arquivos TS/QM) [[72]][72] |
| Flutter | Não confirmado | Não confirmado | Não confirmado | Não confirmado |

## Finalistas

A e B usam a mesma base para a parte crítica, a camada sobre a tela: janelas Win32 com Direct2D, uma por monitor, no modo Per-Monitor V2, com captura por Windows.Graphics.Capture ou Desktop Duplication. É o mesmo desenho da Régua do PowerToys [[37]][37] [[38]][38]. Entre A e B, a diferença está em **como se fazem as telas de configuração** e no **peso** que isso traz.

C tem dois caminhos para a camada, e nenhum está confirmado:
- **Desenhar a camada em janelas do próprio Tauri (WebView):** esbarra no bug aberto de tamanho de janela entre monitores [[48]][48], e a nitidez de linhas finas em escala fracionária não foi confirmada.
- **Abrir janelas Win32 nativas em Rust ao lado do Tauri:** a viabilidade não foi confirmada. Se funcionar, C vira na prática "núcleo nativo + telas web".

### (A) Nativo puro: Rust ou C++ direto no Win32 / Direct2D

**Prós**
- O mais leve: nenhum framework ou runtime extra a instalar; o app fica isolado de mudanças de terceiros.
- Controle total da escala por monitor e da transparência, exatamente o ponto mais difícil [[21]][21].
- Usa só APIs do próprio Windows, então o risco de abandono vem quase só da linguagem escolhida.
- Há uma implementação de referência pública da Microsoft, em C++ [[37]][37] [[40]][40].
- Uma linguagem só.

**Contras**
- Telas de configuração e tema escuro feitos à mão: o Windows só escurece a barra de título [[36]][36]. É o ponto mais fraco no critério 1.
- Mais código para escrever e manter, inclusive para a escala de cada controle [[21]][21].
- Sub-escolha de linguagem, com riscos diferentes:
  - **Rust:** as crates da Microsoft estão abaixo de 1.0 e mudam com frequência [[57]][57] [[59]][59].
  - **C++:** o compilador não protege contra erros de memória (avaliação).
- Tamanho final e dependência do runtime do Visual C++: não confirmados.

### (B) Híbrido no formato do PowerToys: núcleo nativo + WinUI 3 nas telas

**Prós**
- O caminho que a própria Microsoft usou para esta ferramenta [[37]][37] [[39]][39].
- Telas com visual do Windows 11 e tema claro/escuro automáticos [[36]][36].
- Localização embutida (MRT Core) [[20]][20].
- A camada fica fora do WinUI, o que evita o problema da transparência [[13]][13].
- Fornecedor forte, com ciclo de vida publicado [[6]][6].

**Contras**
- Duas linguagens e duas formas de compilar (C++ ou Rust no núcleo, C# nas telas).
- App bem mais pesado: runtime de 120,2 MB ou cópia autocontida "significativamente maior" [[9]][9] [[11]][11]. Com C#, ainda o .NET 10 [[8]][8].
- O Windows App SDK exige atualizações frequentes: cada linha tem cerca de 1 ano de correções [[6]][6].
- Bandeja sem API própria [[18]][18].
- Bug aberto de "sempre no topo" ao abrir janelas WinUI [[14]][14].
- A própria Microsoft depende de uma biblioteca comunitária (WinUIEx) nessas telas [[39]][39].

### (C) Tauri 2: núcleo em Rust + telas em HTML/CSS (WebView2)

**Prós**
- Leve, porque o WebView2 já vem no Windows 11 [[44]][44] [[49]][49].
- Bandeja e atalhos globais oficiais [[46]][46] [[47]][47].
- Telas em tecnologia web, a mais fácil de fazer e ajustar com IA (avaliação).
- Aparência fica totalmente sob controle do projeto.
- Se um dia fizer sentido, abre a porta para outros sistemas [[41]][41].

**Contras**
- Duas linguagens (Rust e web).
- A captura e, provavelmente, a camada precisam de código Rust direto no Windows de qualquer forma (não confirmado). Na prática, isso vira um "nativo + telas web".
- Bug aberto de tamanho de janela entre monitores no Windows (relatado na 1.x; na 2.x não confirmado) [[48]][48].
- Nitidez de linhas de 1 pixel no WebView em escala fracionária: não confirmada.
- Sem datas de suporte publicadas [[43]][43], com a 3.0 já em alfa [[42]][42].
- Tema claro/escuro e visual do Windows 11 nas telas: feitos pelo projeto.

## O que o Pixmeter exige, em termos simples

- **Escala (DPI) por monitor.** O Windows aumenta textos e imagens em telas de alta resolução, por exemplo 150–175% num monitor 4K e 100–125% num Full HD. Para ficar nítido nos dois ao mesmo tempo, o app precisa usar o modo "Per-Monitor V2" (disponível desde o Windows 10 versão 1703) e redesenhar cada janela na escala do monitor onde ela está [[21]][21].
  - Em Win32 puro, essa adaptação é responsabilidade do próprio app. No WPF e no WinForms, o framework faz uma parte [[21]][21].
- **Camada transparente, sempre no topo, uma por monitor.** A régua tem de enxergar os pixels físicos reais de cada tela. É a exigência mais difícil e a que mais separa as opções.
- **Captura e leitura exata de pixels**, **atalhos globais**, **ícone na bandeja**, **telas de configuração** e **dois idiomas** (inglês e português do Brasil).

## Peças que vêm do Windows e servem para todas as opções nativas

- **Captura de tela**, por dois caminhos:
  - **Windows.Graphics.Capture**: captura um monitor inteiro quadro a quadro [[31]][31]. Durante a captura, o Windows desenha uma borda amarela em volta do que está sendo capturado [[31]][31].
    - Para tirar a borda, o app precisa pedir consentimento ao usuário e declarar uma permissão no manifesto do pacote [[32]][32] [[33]][33]. **Não confirmado:** se um app distribuído sem pacote (instalador comum) consegue remover a borda. O PowerToys tenta remover (`IsBorderRequired(false)`) [[37]][37].
  - **Desktop Duplication (DXGI)**: entrega a imagem de cada monitor quadro a quadro, sempre no formato de cor BGRA de 8 bits [[34]][34]. A documentação não menciona borda amarela.
- **Atalhos globais**: `RegisterHotKey`, disponível desde o Windows Vista [[35]][35].
  - Falha se a combinação de teclas já estiver registrada por outro programa.
  - A tecla F12 é reservada para depuração.
  - Combinações com a tecla Windows são reservadas ao sistema [[35]][35].
- **Ícone na bandeja**: `Shell_NotifyIcon`, disponível desde o Windows XP [[73]][73].
- **Tema claro e escuro sem framework.** O Windows não pinta sozinho um app Win32 com o tema escuro.
  - O app precisa detectar o tema e redesenhar a própria interface. O Windows só oferece a barra de título escura, via `DWMWA_USE_IMMERSIVE_DARK_MODE` [[36]][36].
  - A Microsoft diz que o WinUI 3 detecta o tema do sistema automaticamente [[36]][36].

## Detalhes por candidato

### WinUI 3 / Windows App SDK

- É a tecnologia de interface mais nova da Microsoft para apps desktop. A linha 2.0 foi a primeira mudança de versão principal desde a 1.0, de novembro de 2021 [[7]][7].
- **Distribuição sem a Microsoft Store**, em dois modos:
  - O app depende do runtime instalado. Nesse caso, o usuário precisa do Windows App Runtime (instalador x64 de 120,2 MB) e do Visual C++ Redistributable [[10]][10] [[11]][11].
  - O app leva o runtime junto ("autocontido"). A pasta fica "significativamente maior", e cada atualização carrega o runtime inteiro de novo [[9]][9].
  - Duas páginas oficiais, ambas de 02/10/2026, se contradizem sobre gerar um único `.exe`. Uma diz que não é possível [[8]][8]. A outra diz que é possível desde a 1.5, com os arquivos extraídos para uma pasta temporária na primeira execução [[9]][9].
- Compilação nativa antecipada (Native AOT) é suportada desde a 1.6. O desenvolvedor fica responsável por evitar que o "enxugamento" remova código necessário [[12]][12].
- **Riscos para a camada do Pixmeter:**
  - Janela transparente e sem borda é um pedido aberto desde 30/08/2019, com 141 comentários. O contorno usado pela comunidade é a biblioteca WinUIEx (`TransparentTintBackdrop`), que não é da Microsoft [[13]][13].
  - Bug aberto: ao abrir uma janela WinUI 3 (Windows App SDK 1.6), as janelas "sempre no topo" de outros programas perdem essa condição [[14]][14]. **Não confirmado:** se o bug continua na 2.x e se afeta a camada do próprio app.
  - Escala: há bugs abertos, como menus cortados após mudança de DPI [[15]][15] e uma dica de arrastar que não respeita a escala por monitor [[16]][16]. Um bug antigo de janela que não se redimensionava entre monitores foi fechado [[17]][17]. A 2.5.1 ainda corrigiu um defeito visual em escala de 175% [[7]][7].
- **Bandeja:** não há API no Windows App SDK; o pedido está aberto desde 08/04/2021 [[18]][18]. Um pedido de maio de 2026 foi fechado como duplicado desse [[19]][19].

### WPF no .NET 10

- **Tema:** o tema Fluent, com modos claro e escuro e cor de destaque, chegou no .NET 9 [[22]][22].
  - Ligar o tema pelo XAML funciona. Trocar o tema pelo código ainda é experimental (aviso WPF0001) [[22]][22].
  - No .NET 10, o suporte ao Fluent "ainda está em andamento" [[23]][23].
- **Transparência:** suportada com `AllowsTransparency`, que exige janela sem borda [[24]][24].
- **Escala por monitor:** a tabela da Microsoft diz que apps WPF nativos escalam por monitor [[21]][21]. Há bugs abertos no modo Per-Monitor V2:
  - `Window.Left/Top` quebrados, aberto desde 03/02/2021 e atualizado em 21/10/2026 [[25]][25].
  - `SystemParameters.WorkArea` quebrado [[26]][26].
  - Isso pesa para o Pixmeter, que precisa posicionar uma camada exatamente sobre cada monitor.
- **Tamanho:** sem o .NET instalado, o app precisa levar o runtime. O trimming está desativado para WPF ("quase nenhum app WPF roda depois do trimming") [[4]][4] [[27]][27]. Como o Native AOT exige trimming, também não se aplica [[5]][5].

### WinForms no .NET 10

- O modo escuro deixou de ser experimental no .NET 10 [[28]][28].
- **Escala por monitor:** a Microsoft a descreve como "limitada, para alguns controles" [[21]][21]. Há bugs abertos com controles mal escalados em monitor secundário [[29]][29].
- Mesmas limitações de tamanho do WPF (sem trimming) [[4]][4].

### Tauri 2

- **Tecnologia:** núcleo em Rust; telas desenhadas pelo WebView2 (o motor do Edge embutido no Windows). Funciona do Windows 10 em diante. Licença MIT/Apache-2.0 [[41]][41].
- **Recursos:**
  - Bandeja é um recurso do núcleo [[46]][46].
  - Atalhos globais vêm de um plugin oficial, que exige Rust 1.90 ou mais novo [[47]][47].
- **WebView2:**
  - Por padrão, o instalador baixa o WebView2 só se ele faltar (0 MB a mais). Embutir o instalador offline acrescenta cerca de 127 MB [[44]][44].
  - A documentação da Microsoft dá mais de 250 MB para a versão "fixa" do runtime; a do Tauri dá cerca de 180 MB [[44]][44] [[49]][49]. Nenhuma das duas é necessária no Windows 11.
  - O WebView2 se atualiza sozinho, mas um app que fica aberto o tempo todo (como um app de bandeja) continua usando a versão antiga até reiniciar [[49]][49].
- **Riscos:**
  - Bug aberto no Windows: uma janela criada num monitor secundário com escala diferente nasce com o tamanho errado. Foi relatado na versão 1.x; **não confirmado** se acontece na 2.x [[48]][48].
  - Não há API de captura no Tauri; isso seria código Rust direto no Windows (não confirmado).
  - A 3.0 em alfa indica uma migração no futuro [[42]][42].

### Electron

- **Runtime grande:** o runtime do Windows tem 158,2 MB [[52]][52]. Isso contraria diretamente o critério 4 (leve).
- **Atualização constante:** sai uma versão principal a cada 8 semanas e só as 3 últimas têm suporte [[50]][50]. Na prática, cada versão vive cerca de 6 meses; a 45 sai do suporte em 27/04/2027 [[51]][51].
- **Recursos:**
  - Captura com `desktopCapturer` [[53]][53]; atalhos globais [[74]][74] e bandeja [[75]][75].
  - Janelas transparentes não podem ser redimensionadas [[54]][54]. Podem ignorar o mouse e ficar acima da barra de tarefas [[55]][55].
- **Escala por monitor:** um bug de posição e tamanho está aberto desde 20/10/2017, com 71 comentários [[56]][56].

### Rust direto no Win32 / Direct2D

- **Ferramentas:**
  - As crates da Microsoft cobrem COM, WinRT, janelas e Direct2D (`windows-canvas`, `windows-window`, `windows-composition`) [[57]][57].
  - O próprio repositório avisa que as crates "guarda-chuva" `windows` e `windows-sys` "acrescentam peso de dependências e mudanças frequentes de versão" [[57]][57].
  - Todas as versões estão abaixo de 1.0 [[59]][59].
  - Existe uma biblioteca para usar WinUI 3 a partir do Rust (`windows-reactor`), mas ela "está em preview" e **não entra na comparação** pela regra de usar só versões estáveis [[58]][58].
- **Escala e tema:** a escala por monitor e o tema escuro são responsabilidade do app [[21]][21] [[36]][36].

### C++ direto no Win32 / Direct2D

- **É a referência da Microsoft:** a Régua de Tela do PowerToys usa esse caminho [[37]][37] [[38]][38]:
  - um arquivo de captura por monitor com Windows.Graphics.Capture;
  - arquivos separados de Direct2D e de desenho da camada.
- **Barra de ferramentas:** é um projeto WinUI 3 separado, sem pacote e autocontido, que depende da biblioteca WinUIEx [[39]][39].
- **Licença:** o código do PowerToys é MIT, então pode servir de referência [[40]][40].

### Avalonia

- **Versões:**
  - A Avalonia 12 foi anunciada em 07/04/2026, mirando o .NET 10 [[61]][61].
  - O mínimo é o .NET 8 [[62]][62]. O suporte do .NET 8 acaba em 10/11/2026 [[1]][1], então na prática o Pixmeter teria de usar o .NET 10.
- **Desenho:** o desenho no Windows passou a ser só por Skia; o Direct2D foi removido [[62]][62].
- **Recursos:** bandeja embutida [[64]][64]. O anúncio da 12 cita inicialização mais rápida com Native AOT [[61]][61].
- **Riscos:** bugs abertos com escalas mistas [[65]][65]. Ser multiplataforma não traz ganho ao Pixmeter, que é só para Windows.

### Flutter

- A 3.44 (20/05/2026) traz as APIs de janelas como experimentais: "ainda não são para uso em produção", só no canal de desenvolvimento [[66]][66].
- O Pixmeter precisa de uma janela por monitor mais as janelas de configuração. Por isso, **não atende hoje**.

### Qt 6

- **Escala:** Per-Monitor V2 por padrão e escalas fracionárias automáticas [[68]][68].
  - Limitação documentada: no Windows, o Qt escala o tamanho de cada tela mas não a posição, o que pode deixar "buracos" na área de trabalho virtual. A documentação recomenda não supor que uma posição logo fora de uma tela caia na tela vizinha [[68]][68]. Isso afeta diretamente o posicionamento de uma camada por monitor.
  - O rastreador de bugs do Qt não foi pesquisado: problemas conhecidos com escalas mistas ficam como não confirmados.
- **Suporte:**
  - O LTS de 5 anos vale da 6.8 em diante [[70]][70] e está disponível "com as licenças comerciais" [[71]][71].
  - Para usuários de código aberto, só as primeiras correções de cada LTS saem de graça [[69]][69].
- **Recursos:** bandeja com `QSystemTrayIcon` [[76]][76]; tradução com Qt Linguist [[72]][72].
- **Não analisados:** as obrigações da licença de código aberto do Qt (não confirmado).

### Não avaliados nesta pesquisa

.NET MAUI, Uno Platform, Slint, egui e outros frameworks Rust de interface ficaram fora do levantamento. Não houve tempo de verificá-los em fonte primária, e a escolha não depende deles.

## O que ficou sem confirmação

- **Tamanho final** do Pixmeter em todas as tecnologias: nenhum app de exemplo foi compilado.
  - Os números da Tabela 1 são de runtimes e instaladores oficiais, não do app.
- **Borda amarela da captura:** se um app sem pacote consegue removê-la [[32]][32] [[33]][33].
- **Bug WinUI 3 de "sempre no topo"** [[14]][14]: se continua na linha 2.x e se afeta a camada do próprio app.
- **Bug do Tauri** com monitores de escalas diferentes [[48]][48]: se acontece na 2.x.
- **Tauri:** se existe captura de tela pronta; se há mecanismo de tradução próprio; se dá para abrir janelas Win32 nativas em Rust ao lado das janelas do Tauri; nitidez de linhas finas no WebView em escala fracionária.
- **Rust e C++:** se o runtime do Visual C++ pode ser embutido por link estático; se há política formal de suporte para as crates `windows` e para Win32/Direct2D.
- **WPF e WinForms:** componente próprio de bandeja no WPF; transparência no WinForms.
- **Qt:** atalhos globais, captura, obrigações da licença de código aberto, tamanho e bugs conhecidos com escalas mistas (o rastreador de bugs do Qt não foi pesquisado).
  - Também a classificação da 6.12 como LTS: a página de LTS a chama assim [[71]][71], mas a tabela de versões não a marca como LTS [[69]][69].
- **Flutter:** política de suporte, bandeja e atalhos.
- **Avalonia:** integração dos arquivos `.resx` com o Avalonia; se o LTS de 5 anos da versão 12 vale para quem não tem o plano pago.
- **Contradição oficial** sobre gerar um único `.exe` com WinUI 3 [[8]][8] [[9]][9].
- **Critério 2 (manter com IA):** é uma avaliação, sem fonte primária.

## Fontes (consultadas em 26/10/2026)

1. [.NET and .NET Core Support Policy][1]
2. [Install .NET on Windows][2]
3. [.NET 10.0 release metadata][3] — tamanhos medidos pelo cabeçalho HTTP dos arquivos oficiais `windowsdesktop-runtime-10.0.14-win-x64.exe` (60.035.296 bytes) e `.zip` (39.169.811 bytes)
4. [Known trimming incompatibilities][4]
5. [Native AOT deployment overview][5]
6. [Windows App SDK release channels (inclui ciclo de vida)][6]
7. [Windows App SDK 2.0 release notes][7]
8. [Windows App SDK deployment guide for self-contained apps][8]
9. [Distribute an unpackaged WinUI 3 app][9]
10. [Deployment guide for framework-dependent unpackaged apps][10]
11. [Latest Windows App SDK downloads][11] — tamanho medido pelo cabeçalho HTTP de `windowsappruntimeinstall-x64.exe` 2.5.1 (120.199.480 bytes)
12. [Windows App SDK 1.6 release notes (Native AOT)][12]
13. [microsoft-ui-xaml #1247 — janela transparente sem borda][13]
14. [microsoft-ui-xaml #9990 — janelas TOPMOST de outros processos][14]
15. [microsoft-ui-xaml #9512 — menus cortados após mudança de DPI][15]
16. [microsoft-ui-xaml #9717 — dica de arrastar sem DPI por monitor][16]
17. [microsoft-ui-xaml #6720 — redimensionar entre monitores (fechado)][17]
18. [WindowsAppSDK #713 — Modern System Tray Icon][18]
19. [WindowsAppSDK #6487 — fechado como duplicado de #713][19]
20. [MRT Core overview][20]
21. [High DPI Desktop Application Development on Windows][21]
22. [What's new in WPF for .NET 9][22]
23. [What's new in WPF for .NET 10][23]
24. [Window.AllowsTransparency][24]
25. [dotnet/wpf #4127 — Window.Left/Top com PerMonitorV2][25]
26. [dotnet/wpf #10459 — WorkArea com PerMonitorV2][26]
27. [dotnet/wpf #3811 — WPF is not trim-compatible][27]
28. [What's new in WinForms for .NET 10][28]
29. [dotnet/winforms #8580 — ScrollBar em monitor secundário][29] (ver também [#8263][29b])
30. [Localization in .NET (.resx)][30]
31. [Screen capture (Windows.Graphics.Capture)][31]
32. [GraphicsCaptureSession.IsBorderRequired][32]
33. [GraphicsCaptureAccess.RequestAccessAsync][33]
34. [Desktop Duplication API][34]
35. [RegisterHotKey][35]
36. [Support Dark and Light themes in Win32 apps][36]
37. [PowerToys — MeasureToolCore/ScreenCapturing.cpp][37]
38. [PowerToys — pasta MeasureToolCore][38]
39. [PowerToys — MeasureToolUI.csproj][39]
40. [PowerToys — LICENSE (MIT)][40]
41. [Tauri — repositório e README][41]
42. [Tauri — release tauri-v2.12.5][42] (e [tauri-v3.0.0-alpha.5][42b])
43. [Tauri — SECURITY.md][43]
44. [Tauri — Windows Installer / WebView2][44]
45. [Tauri — App Size][45]
46. [Tauri — System Tray][46]
47. [Tauri — Global Shortcut plugin][47]
48. [tauri #6843 — tamanho de janela com vários monitores][48]
49. [Distribute your app and the WebView2 Runtime][49]
50. [Electron Release Timelines][50]
51. [Electron Release Schedule][51]
52. [Electron v45.0.0 release][52] — `electron-v45.0.0-win32-x64.zip` com 158.223.852 bytes
53. [Electron desktopCapturer][53]
54. [Electron custom window styles (transparência)][54]
55. [Electron BrowserWindow][55]
56. [electron #10862 — Per monitor DPI awareness][56]
57. [windows-rs — docs/readme.md][57]
58. [windows-rs — windows-reactor (preview)][58]
59. [crates.io — windows][59] (ver também [windows-core][59b])
60. [Avalonia — releases][60]
61. [Avalonia 12 — anúncio][61]
62. [Breaking changes in Avalonia 12][62]
63. [Avalonia Enterprise / support][63]
64. [Avalonia TrayIcon][64]
65. [Avalonia #17834 — escala com telas de DPI misto][65] (ver também [#13917][65b] e [#14755][65c])
66. [What's new in Flutter 3.44][66]
67. [Flutter release archive][67]
68. [Qt 6 — High DPI][68]
69. [Qt 6 — Qt Releases][69]
70. [Qt — Maintenance & Security Updates][70]
71. [Qt LTS][71]
72. [Qt 6 — Localization][72]
73. [Shell_NotifyIconW][73]
74. [Electron globalShortcut][74]
75. [Electron Tray][75]
76. [Qt 6 — QSystemTrayIcon][76]

[1]: https://dotnet.microsoft.com/en-us/platform/support/policy/dotnet-core
[2]: https://learn.microsoft.com/en-us/dotnet/core/install/windows
[3]: https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json
[4]: https://learn.microsoft.com/en-us/dotnet/core/deploying/trimming/incompatibilities
[5]: https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/
[6]: https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/release-channels
[7]: https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/release-notes/windows-app-sdk-2-0
[8]: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/self-contained-deploy/deploy-self-contained-apps
[9]: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/unpackage-winui-app
[10]: https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/deploy-unpackaged-apps
[11]: https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/downloads
[12]: https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/release-notes/windows-app-sdk-1-6
[13]: https://github.com/microsoft/microsoft-ui-xaml/issues/1247
[14]: https://github.com/microsoft/microsoft-ui-xaml/issues/9990
[15]: https://github.com/microsoft/microsoft-ui-xaml/issues/9512
[16]: https://github.com/microsoft/microsoft-ui-xaml/issues/9717
[17]: https://github.com/microsoft/microsoft-ui-xaml/issues/6720
[18]: https://github.com/microsoft/WindowsAppSDK/issues/713
[19]: https://github.com/microsoft/WindowsAppSDK/issues/6487
[20]: https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/mrtcore/mrtcore-overview
[21]: https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows
[22]: https://learn.microsoft.com/en-us/dotnet/desktop/wpf/whats-new/net90
[23]: https://learn.microsoft.com/en-us/dotnet/desktop/wpf/whats-new/net100
[24]: https://learn.microsoft.com/en-us/dotnet/api/system.windows.window.allowstransparency
[25]: https://github.com/dotnet/wpf/issues/4127
[26]: https://github.com/dotnet/wpf/issues/10459
[27]: https://github.com/dotnet/wpf/issues/3811
[28]: https://learn.microsoft.com/en-us/dotnet/desktop/winforms/whats-new/net100
[29]: https://github.com/dotnet/winforms/issues/8580
[29b]: https://github.com/dotnet/winforms/issues/8263
[30]: https://learn.microsoft.com/en-us/dotnet/core/extensions/localization
[31]: https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture
[32]: https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired
[33]: https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscaptureaccess.requestaccessasync
[34]: https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api
[35]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey
[36]: https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-windows-themes
[37]: https://github.com/microsoft/PowerToys/blob/main/src/modules/MeasureTool/MeasureToolCore/ScreenCapturing.cpp
[38]: https://github.com/microsoft/PowerToys/tree/main/src/modules/MeasureTool/MeasureToolCore
[39]: https://github.com/microsoft/PowerToys/blob/main/src/modules/MeasureTool/MeasureToolUI/MeasureToolUI.csproj
[40]: https://github.com/microsoft/PowerToys/blob/main/LICENSE
[41]: https://github.com/tauri-apps/tauri
[42]: https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.5
[42b]: https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.5
[43]: https://github.com/tauri-apps/tauri/blob/HEAD/SECURITY.md
[44]: https://v2.tauri.app/distribute/windows-installer/
[45]: https://v2.tauri.app/concept/size/
[46]: https://v2.tauri.app/learn/system-tray/
[47]: https://v2.tauri.app/plugin/global-shortcut/
[48]: https://github.com/tauri-apps/tauri/issues/6843
[49]: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution
[50]: https://www.electronjs.org/docs/latest/tutorial/electron-timelines
[51]: https://releases.electronjs.org/schedule
[52]: https://github.com/electron/electron/releases/tag/v45.0.0
[53]: https://www.electronjs.org/docs/latest/api/desktop-capturer
[54]: https://www.electronjs.org/docs/latest/tutorial/custom-window-styles
[55]: https://www.electronjs.org/docs/latest/api/browser-window
[56]: https://github.com/electron/electron/issues/10862
[57]: https://github.com/microsoft/windows-rs/blob/master/docs/readme.md
[58]: https://github.com/microsoft/windows-rs/blob/master/docs/crates/windows-reactor.md
[59]: https://crates.io/crates/windows
[59b]: https://crates.io/crates/windows-core
[60]: https://github.com/AvaloniaUI/Avalonia/releases
[61]: https://avaloniaui.net/blog/avalonia-12
[62]: https://docs.avaloniaui.net/docs/avalonia12-breaking-changes
[63]: https://avaloniaui.net/support
[64]: https://docs.avaloniaui.net/docs/reference/controls/tray-icon
[65]: https://github.com/AvaloniaUI/Avalonia/issues/17834
[65b]: https://github.com/AvaloniaUI/Avalonia/issues/13917
[65c]: https://github.com/AvaloniaUI/Avalonia/issues/14755
[66]: https://flutter.dev/blog/whats-new-in-flutter-3-44
[67]: https://docs.flutter.dev/release/archive
[68]: https://doc.qt.io/qt-6/highdpi.html
[69]: https://doc.qt.io/qt-6/qt-releases.html
[70]: https://www.qt.io/development/qt-framework/maintenance-periods
[71]: https://www.qt.io/development/qt-framework/qt-lts
[72]: https://doc.qt.io/qt-6/localization.html
[73]: https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw
[74]: https://www.electronjs.org/docs/latest/api/global-shortcut
[75]: https://www.electronjs.org/docs/latest/api/tray
[76]: https://doc.qt.io/qt-6/qsystemtrayicon.html
