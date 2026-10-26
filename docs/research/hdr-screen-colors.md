# Cores da tela com o HDR do Windows ligado

Pesquisa do ticket WPC-282 ("Cores da tela com o HDR do Windows ligado") para o Pixmeter, a régua de tela para Windows. A pergunta: com o HDR ligado no Windows 10 ou 11, a cor que um programa lê de um pixel da tela é a mesma que o programa por baixo desenhou? Exemplo: uma página web pinta um botão de `#1E90FF`; com o HDR ligado, o Inspect leria `#1E90FF` ou outro valor? Este documento só reúne fatos e as opções que eles permitem, não decide nada.

Fontes: documentação primária da Microsoft (learn.microsoft.com) e o código-fonte aberto do PowerToys (repositório da própria Microsoft). Todas consultadas em **2026-10-26**. Cada fato traz a fonte entre colchetes, como [F1]; a lista com os endereços está no fim. O que não consegui confirmar está marcado como **não confirmado**. Contas e deduções minhas (não escritas pela Microsoft) estão marcadas como **dedução**. Fontes de terceiros (não são da Microsoft) estão marcadas como **terceiros** e valem como indício, não como prova. Trechos entre aspas são traduções minhas dos textos originais em inglês.

## Resumo

1. Com o HDR ligado, o Windows muda a forma como monta a imagem da tela: o compositor (DWM, o programa do Windows que junta todas as janelas) passa a trabalhar em ponto flutuante de 16 bits, no espaço de cor "scRGB", em vez de números inteiros de 8 bits [F1].
2. Apps comuns (SDR, os que pintam `#1E90FF` em 8 bits) continuam sendo aceitos normalmente. O Windows converte a cor deles para esse formato interno e **clareia** o branco deles até o nível "brilho do conteúdo SDR" que o usuário escolhe nas configurações (tipicamente por volta de 200 nits em monitores de mesa, contra 80 nits de referência) [F1][F2].
3. Isso quer dizer que o número guardado dentro da imagem da tela **não é mais o `#1E90FF` original**. Para voltar ao valor original é preciso desfazer a conversão. A Microsoft documenta a conta no sentido de ida (multiplicar pelo nível de branco SDR dividido por 80) [F1]; ela **não** documenta o caminho de volta nem diz o que cada método de captura entrega a um app que pede 8 bits.
4. Os três métodos de captura se comportam assim segundo a documentação: **GDI (BitBlt)**: só diz que converte o formato da cor se os formatos diferirem e que não há gerenciamento de cor [F8]; nada sobre HDR (**não confirmado**). **DXGI Desktop Duplication**: a versão antiga entrega sempre 32 bits BGRA [F4][F5]; a `DuplicateOutput1` permite pedir outros formatos e, se o formato atual não estiver na lista pedida, o Windows converte para um dos pedidos [F3]. **Windows.Graphics.Capture**: a Microsoft avisa que, com HDR, o formato de 8 bits pode "estourar" (a imagem sai desbotada) e recomenda capturar em ponto flutuante de 16 bits e processar depois [F6].
5. A Microsoft diz, nos dois métodos modernos, que depois da captura em formato HDR "é necessário processamento adicional", como converter de HDR para SDR (tone mapping) [F1]. A conversão de volta para o `#1E90FF` exato fica a cargo do app.
6. O PowerToys **não trata o HDR**. O Color Picker lê o pixel com GDI (`CopyFromScreen`, 1×1 pixel, depois `GetPixel`) e a lupa também [F10][F11]; o Screen Ruler captura com Windows.Graphics.Capture em 8 bits (B8G8R8A8) e compara canal por canal [F12]. O pedido para o Color Picker "ler a cor original do conteúdo SDR" está aberto desde outubro de 2023 [F13], e um relato de "cor desbotada com HDR" foi fechado como duplicata dele [F14].
7. Um projeto de terceiros mostra que, num monitor HDR real, capturar o quadro em ponto flutuante de 16 bits pela `DuplicateOutput1` e converter usando o nível de branco SDR devolveu os mesmos pixels que a captura com o HDR desligado, em uma imagem de teste com 16 níveis de cinza e blocos de cor [F17] (**terceiros**). O mesmo texto afirma que, no Windows 11, a leitura por GDI já voltava correta e que, no Windows 10, saía tudo branco [F17] (**terceiros**; a Microsoft não documenta isso).
8. Ou seja: para **apps SDR comuns**, o valor original provavelmente é recuperável, mas por um caminho que a Microsoft não descreve de ponta a ponta. Para **conteúdo HDR de verdade** (vídeo HDR, jogos em HDR), não existe "valor original em 8 bits": os valores passam do branco SDR e só dá para aproximar por tone mapping [F1][F6].

## 1. O que acontece com as cores de apps comuns quando o HDR liga

### 1.1 O compositor muda de formato

A Microsoft chama o conjunto de recursos de HDR, gama de cores ampla e cores de maior precisão de **Advanced Color** ("cor avançada"). Ele chegou ao Windows 10 versão 1709 para monitores HDR e ao Windows 11 versão 22H2 para alguns monitores SDR provisionados [F1].

Antes, o DWM limitava os apps a 8 bits por canal. Com o Advanced Color ligado, "o DWM faz a composição em ponto flutuante de meia precisão (FP16)" [F1]. No modo HDR, o espaço de composição é o **scRGB** (primárias iguais às do sRGB, mas com gama linear) em FP16. Nesse formato, o número 1,0 do canal significa 80 nits de luminância [F1].

O Windows faz o gerenciamento de cor em duas etapas: (1) o DWM converte cada app do espaço de cor dele para o scRGB antes de misturar as janelas; (2) o núcleo de vídeo do Windows converte o quadro final para o formato do cabo (HDR10, BT.2100 ST.2084). Em ambas, a conversão é uma matriz mais uma tabela 1D, e cores fora da gama da tela são cortadas numericamente [F1].

### 1.2 O branco dos apps SDR é clareado

A Microsoft define o **nível de branco de referência SDR** (SDR white level): é "a luminância em que o Windows desenha o branco sRGB (1,0, 1,0, 1,0)". O usuário ajusta esse valor, e em monitores de mesa HDR ele costuma ficar perto de 200 nits [F1]. O nível padrão nominal é 80 nits. Para o app SDR aparecer correto, a conta é multiplicar o valor (em gama linear) por `SdrWhiteLevelInNits / 80` [F1].

Quando o app deixa o sistema compor as janelas, o Windows faz esse ajuste sozinho: "o Windows vai automaticamente aumentar o conteúdo SDR até o nível de branco desejado" [F1].

### 1.3 Como um programa lê o nível de branco SDR

Em apps de desktop: `DISPLAYCONFIG_SDR_WHITE_LEVEL`, via `DisplayConfigGetDeviceInfo`/`QueryDisplayConfig`. O campo `SDRWhiteLevel` é "um múltiplo de 80 nits, multiplicado por 1000": 1000 = 80 nits, 2000 = 160 nits. A conta oficial é `nits = SDRWhiteLevel / 1000 * 80` [F2]. Em apps UWP: `AdvancedColorInfo.SdrWhiteLevelInNits` [F1].

### 1.4 Em tela SDR ou HDR desligado

Com o HDR desligado e a tela sem Advanced Color, o caminho antigo vale: 8 bits por canal, sRGB implícito, sem gerenciamento de cor do sistema [F1]. É esse o caso em que "o valor lido é o valor desenhado" é a premissa de ferramentas como o PowerToys.

### 1.5 Telas SDR com Advanced Color (Windows 11 22H2)

Telas SDR especialmente provisionadas também ganham composição em FP16 e gerenciamento de cor automático. A diferença é que o branco 1,0 vira "o branco máximo da tela", e não 80 nits [F1]. A própria Microsoft avisa que o DXGI (`IDXGIOutput6`) **não distingue** essa tela de uma SDR comum: as duas aparecem como `DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709` [F1]. Para o Pixmeter isso importa só porque a detecção de "HDR ligado" pelo DXGI cobre telas HDR, não essas telas SDR com Advanced Color.

### 1.6 A curva do SDR dentro do HDR

Fontes de terceiros e um pedido aberto no repositório do PowerToys afirmam que o Windows converte o SDR para HDR usando a curva sRGB "em pedaços" (piecewise) e não a gama 2,2 pura, o que levanta os pretos em conteúdo feito para 2,2 [F15] (**terceiros**; a Microsoft não documenta isso em nenhuma página que eu tenha lido; **não confirmado**). Para a pergunta do ticket isso é, na prática, um detalhe de **como a tela mostra** a cor. O que importa para o Inspect é o número que o app lê de volta.

## 2. O que cada método de captura entrega com o HDR ligado

Quadro-resumo do que a documentação da Microsoft permite afirmar:

| Método | Formato que entrega | O que a Microsoft documenta sobre HDR | Recupera o valor SDR original? |
| --- | --- | --- | --- |
| GDI (`BitBlt`, `CopyFromScreen`, `GetPixel`) | 8 bits por canal, no formato do contexto de destino [F8] | Nada. Só: converte o formato se os formatos diferirem; "nenhum gerenciamento de cor é feito" [F8] | **Não confirmado.** Terceiros dizem que sim no Windows 11 e que não no Windows 10 [F17] |
| DXGI Desktop Duplication, `DuplicateOutput` | Sempre 32 bits BGRA (B8G8R8A8) [F4][F5] | Nada específico | **Não confirmado** (o texto não descreve a conversão feita com HDR ligado) |
| DXGI `DuplicateOutput1` | O formato que o app pedir da lista; sem pedir, o do quadro original (ex.: FP16, R10G10B10A2) [F3] | Tema coberto só indiretamente: "é necessário processamento adicional", como tone mapping HDR para SDR [F1] | Sim, **por conta do app**: pedindo FP16 e dividindo pelo nível de branco SDR (dedução, ver 2.2) |
| Windows.Graphics.Capture | O formato pedido ao criar o `Direct3D11CaptureFramePool`; o exemplo usa B8G8R8A8 [F6][F7] | Com HDR, o formato do conteúdo "pode não ser B8G8R8A8"; para evitar cortes ("desbotado"), a Microsoft sugere FP16 de ponta a ponta [F6] | Igual ao anterior: por conta do app |

### 2.1 GDI

A página do `BitBlt` não menciona HDR, DWM nem composição. Diz apenas que, se os formatos de cor de origem e destino não coincidem, o Windows converte o formato de origem, e que não há gerenciamento de cor na cópia [F8]. Portanto **a Microsoft não documenta o que o GDI devolve com o HDR ligado**. O que existe é indício de terceiros: um projeto de código aberto relata que no Windows 10 a leitura por GDI saía "toda branca" com HDR ligado, e que no Windows 11 (build 29683, em teste dele) a leitura já saía correta [F17] (**terceiros**; build de pré-lançamento, uma máquina só).

### 2.2 DXGI Desktop Duplication

A documentação da `DuplicateOutput` diz que a imagem sai em BGRA de 32 bits [F4][F5]. A `DuplicateOutput1` foi criada para receber "o formato original do buffer" e "a gama completa de cores quando um formato de alta cor (como R10G10B10A2) está sendo apresentado"; se o formato atual não estiver na lista pedida, o DXGI escolhe um da lista e converte antes de devolver o quadro. A Microsoft recomenda que a lista contenha sempre `B8G8R8A8_UNORM` [F3]. A página de Advanced Color diz que a `DuplicateOutput1` e o Windows.Graphics.Capture "permitem capturar conteúdo HDR e de gama ampla sem perder informação de pixel", mas que depois é preciso processar (tone mapping, salvar no formato certo) [F1].

O que **não** está escrito em página nenhuma da Microsoft que li: qual fórmula o Windows usa ao converter o quadro FP16 para 8 bits quando só o BGRA é pedido. Há uma conversa no fórum de perguntas da Microsoft em que um desenvolvedor pediu só 8 bits e recebeu "cores incorretas" numa página do Edge; a resposta de uma pessoa da equipe de suporte da Microsoft foi "pelo que entendo, a API faz a conversão por você" e, depois, sugerir um problema no Edge; o autor resolveu forçando o perfil de cor do Edge para sRGB, e o thread não tem resposta aceita [F9]. Vale como relato, não como especificação.

**Dedução**: se o app pedir FP16 e dividir cada canal por `SdrWhiteLevelInNits / 80` (a conta inversa da ida em [F1]), depois aplicar a curva sRGB e arredondar para 8 bits, o resultado para conteúdo SDR comum deve voltar ao valor original, porque o FP16 tem folga de sobra para 8 bits. Isso é uma conta inversa minha, não um procedimento publicado pela Microsoft, e depende de a curva usada pelo Windows na ida ser exatamente a sRGB (ver 1.6). O teste de terceiros em [F17] é um indício a favor, não uma garantia.

### 2.3 Windows.Graphics.Capture

A página oficial de captura de tela usa B8G8R8A8 no exemplo e traz o aviso: "em sistemas com Windows HD Color ligado, o formato de pixel do conteúdo pode não ser necessariamente B8G8R8A8. Para evitar corte excessivo de pixels (ou seja, o conteúdo capturado parecer desbotado) ao capturar conteúdo HDR, considere usar `DXGI_FORMAT_R16G16B16A16_FLOAT` em todos os componentes do pipeline de captura." E acrescenta que o artigo trata só de conteúdo SDR [F6]. A página do `Direct3D11CaptureFramePool.Create` só descreve o parâmetro de formato, sem nota sobre HDR [F7].

A página não diz o que acontece quando o app pede B8G8R8A8 com o HDR ligado (**não confirmado**), nem como o conteúdo SDR é mapeado nesse caso.

### 2.4 É possível recuperar o valor SDR original?

- **Caminho 1: pedir 8 bits e confiar na conversão do Windows.** Funciona se o Windows fizer a conversão inversa certa. A Microsoft não garante isso por escrito. Indício de terceiros: varia por versão do Windows [F17].
- **Caminho 2: pedir FP16 e converter você mesmo.** Dividir pelo nível de branco SDR (lido por [F2]) e aplicar a curva sRGB, tudo no app. Dá controle, mas exige código de GPU/CPU a mais, e o resultado para conteúdo HDR de verdade passa de 1,0 e precisa de tone mapping ou corte [F1][F6].
- **Limite que nenhum caminho resolve**: se o conteúdo é HDR de verdade (vídeo HDR, jogo em HDR, ou um app que desenha em scRGB com valores acima de 1,0), não existe um "valor SDR original" a recuperar, só uma aproximação. E o relato em [F14] mostra o caso intermediário: uma cor definida em outro espaço (Display P3) que o app SDR converte para sRGB antes de pintar, deixando a "cor original" ambígua. Isso não é do HDR e acontece também com HDR desligado, mas aparece nas queixas.

## 3. O que o PowerToys e outros fazem

### 3.1 Color Picker

- Lê um pixel com GDI: `Graphics.CopyFromScreen` de 1×1 pixel para um `Bitmap` 32bppArgb e depois `GetPixel(0, 0)` [F10]. A lupa captura 50×50 pixels da mesma forma [F11]. Não há nenhuma menção a HDR, nível de branco SDR ou captura em ponto flutuante no código [F10][F11].
- O pedido "Color Picker HDR Display Support" (issue 29421, aberto em 2023-10-25, ainda aberto) descreve exatamente a pergunta do ticket: com o HDR ligado o Color Picker lê frequentemente `#ffffff` ou outra cor clareada, também em conteúdo SDR clareado pelo controle de brilho do Windows, e pede que ele "ignore essas configurações para achar a cor original". Um comentarista disse que, no caso dele, o culpado era o Firefox em picture-in-picture e que outros apps liam bem com o HDR ligado; um mantenedor respondeu só que era "um problema interessante" [F13].
- Outro relato (issue 41107, fechado em 2026-04-18 como duplicata da 29421) diz que "a cor desbotada" foi lida com o HDR ligado e que, com ele desligado, a leitura chegou "muito mais perto", mas ainda devolveu a cor sRGB de substituição e não a Display P3 pretendida [F14]. É relato de usuário, não análise.
- Conclusão para o PowerToys: não há tratamento de HDR no código lido, e o problema segue em aberto. **Não confirmado** se o relato do usuário de 29421 vale para a versão atual no Windows 11, porque o comentarista de [F13] relata o contrário para outros apps.

### 3.2 Screen Ruler (régua, "Bounds" do PowerToys)

- Captura o monitor inteiro com Windows.Graphics.Capture (`CreateForMonitor`) e pede `B8G8R8A8UIntNormalized` ao criar o pool de quadros [F12]. Não há tratamento de HDR no arquivo [F12].
- Acha as bordas comparando pixels vizinhos canal por canal, com uma tolerância escolhida pelo usuário (`PixelsClose<PerChannel>(..., tolerance)`) [F12]. Ou seja, compara **diferenças** entre cores, não cores absolutas.
- Não achei relato de bug do Screen Ruler específico de HDR (**não confirmado** que não exista). Existe um pedido de captura de tela HDR no PowerToys, fechado com a resposta de que o PowerToys não pretende substituir as ferramentas padrão do Windows [F16].

### 3.3 Outras ferramentas

O PowerToys foi a única ferramenta aberta que li no código. Outras ferramentas (por exemplo, um projeto de código aberto de OCR em tela) tratam o HDR capturando em FP16 pela `DuplicateOutput1` e convertendo com o nível de branco SDR [F17] (**terceiros**; li só a descrição da mudança, não o código-fonte). Não pesquisei Greenshot, ShareX nem outros (**não confirmado**).

## 4. O que muda para as outras ferramentas do Pixmeter

Tudo abaixo é **dedução** minha a partir dos fatos acima, não resultado de teste.

- **Dimensions e Bounds** (acham bordas comparando cores de pixels vizinhos): como o Screen Ruler do PowerToys [F12], comparam diferenças. Se a conversão que o Windows aplica é suave e uniforme, as bordas se mantêm. O risco é perto do branco: valores acima do branco SDR são cortados ou espremidos na conversão para 8 bits [F1][F6], e duas cores quase brancas diferentes poderiam virar a mesma. Também é possível que a tolerância ajustada para HDR desligado se comporte diferente com o HDR ligado, porque os degraus entre cores vizinhas mudam de tamanho.
- **Inspect** (lupa e cor copiada como `#1E90FF`): é a ferramenta que a pergunta atinge de frente. Ela só é fiel se a leitura voltar ao valor que o app desenhou.
- **Snip** (imagem salva na área de transferência ou em arquivo): salvar em PNG de 8 bits sRGB funciona para conteúdo SDR se a conversão estiver certa; com conteúdo HDR de verdade a imagem sai desbotada ou estourada, como alerta a própria Microsoft [F6]. Salvar um recorte HDR fiel exigiria capturar em FP16 e gravar num formato próprio (a Microsoft cita JPEG XR) [F1].

## 5. Opções que os fatos permitem

Não decido aqui. O custo de cada uma:

### 5.1 Promessa de cor exata no v1

- **A. Cor exata só com o HDR desligado; com HDR ligado, limitação conhecida.** O Pixmeter detecta o HDR (a Microsoft documenta como: `IDXGIOutput6::GetDesc1`, espaço de cor `DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020` = HDR [F1]), mostra um aviso e documenta a limitação. É o que o PowerToys faz na prática (sem o aviso) [F10][F13]. Custo baixo; o Inspect pode dar um valor "errado" com HDR ligado, e o aviso explica por quê. A detecção por DXGI não cobre telas SDR com Advanced Color [F1], mas nessas o branco é o máximo da tela e não há clareamento por nível de branco SDR [F1].
- **B. Cor exata para apps SDR comuns também com o HDR ligado.** Capturar em FP16 (`DuplicateOutput1` ou Windows.Graphics.Capture) e converter para 8 bits com o nível de branco SDR (lido por [F2]) e a curva sRGB. Resultado esperado: o valor original volta para conteúdo SDR [F1][F17], mas só é seguro prometer depois de testar em monitores HDR reais, em Windows 10 e 11. Custo maior: código de captura e conversão por GPU, mais testes com hardware que o projeto precisa ter, e conteúdo HDR de verdade continuaria fora da promessa.
- **C. Confiar na conversão do Windows (8 bits) e não prometer nada.** Mesmo caminho do PowerToys. Custo mínimo, mas o resultado varia conforme a versão do Windows (indício em [F17]) e o Pixmeter não saberia dizer quando está errado.

### 5.2 Independente da opção

- Se a escolha for A ou B, vale um **teste de bancada** antes de prometer: pintar `#1E90FF` e uma escala de cinzas numa janela e ler de volta em Windows 10 e 11, com o HDR desligado e ligado, em vários níveis de "brilho do conteúdo SDR". Este documento não fez isso (não há monitor HDR disponível a esta pesquisa).
- Distinguir na linguagem do produto "cor do app" (SDR) e "cor de conteúdo HDR" (vídeo, jogo), porque a segunda não tem valor original recuperável [F1][F6].
- Snip: salvar PNG de 8 bits com a conversão certa (ou limitação documentada) e deixar "captura HDR" fora do v1.

## 6. O que ficou não confirmado

- O que o GDI (`BitBlt`/`CopyFromScreen`/`GetPixel`) devolve com o HDR ligado, em Windows 10 e em Windows 11. A Microsoft não documenta; o indício de terceiros [F17] é de uma máquina e um build de pré-lançamento.
- Qual conversão o Windows aplica quando o app pede só 8 bits (B8G8R8A8) com o HDR ligado, na `DuplicateOutput`, na `DuplicateOutput1` e no Windows.Graphics.Capture.
- Se a `DuplicateOutput1` devolve de fato FP16 no desktop com HDR ligado: a página da Microsoft fala de "aplicativo em tela cheia"; a confirmação vem apenas de terceiros [F17].
- A curva exata (sRGB em pedaços ou gama 2,2) que o Windows usa ao levar o SDR para o HDR [F15]; só terceiros e um pedido de recurso aberto a descrevem.
- Se a conta inversa (dividir pelo nível de branco SDR e aplicar a curva sRGB) devolve `#1E90FF` exato em todas as versões do Windows, ou se há diferença de arredondamento de 1 nível.
- O efeito de Luz noturna, filtros de cor e perfis ICC de monitor sobre a leitura (não pesquisado).
- Telas SDR com Advanced Color (Windows 11 22H2): como se comporta a leitura nelas; a Microsoft diz que o DXGI não as distingue [F1].
- Se o Color Picker do PowerToys ainda erra com o HDR ligado na versão atual: os relatos [F13][F14] são de usuários e divergem entre si.
- O comportamento do Screen Ruler com HDR (não achei relato nem teste).
- Se o "problema do Edge" do fórum [F9] é do navegador (perfil de cor da página) ou da captura; o autor só resolveu forçando o perfil sRGB no Edge.
- Greenshot, ShareX e outras ferramentas abertas: não pesquisadas.

## Fontes

Microsoft (consultadas em 2026-10-26):

- **F1** — Use DirectX with Advanced Color on high/standard dynamic range displays: <https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range>
- **F2** — DISPLAYCONFIG_SDR_WHITE_LEVEL: <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_sdr_white_level>
- **F3** — IDXGIOutput5::DuplicateOutput1: <https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_5/nf-dxgi1_5-idxgioutput5-duplicateoutput1>
- **F4** — IDXGIOutput1::DuplicateOutput: <https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutput1-duplicateoutput>
- **F5** — DXGI_OUTDUPL_DESC ("o formato da imagem da área de trabalho é sempre DXGI_FORMAT_B8G8R8A8_UNORM"): <https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_outdupl_desc>
- **F6** — Screen capture (Windows.Graphics.Capture), nota sobre HDR: <https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture>
- **F7** — Direct3D11CaptureFramePool.Create: <https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.direct3d11captureframepool.create>
- **F8** — BitBlt: <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-bitblt>
- **F9** — Microsoft Q&A, "Using the Desktop Duplication API with HDR - interpreting DXGI_FORMAT_R16G16B16A16_FLOAT bytes" (dezembro de 2023; fórum de comunidade, com resposta de pessoa da equipe de suporte, sem resposta aceita): <https://learn.microsoft.com/en-us/answers/questions/1457052/using-the-desktop-duplication-api-with-hdr-interpr>

PowerToys (repositório da Microsoft; código lido na revisão `415e04ebd32390d189547ffbd1320fe4944747de` do branch `main`):

- **F10** — Color Picker, `MouseInfoProvider.cs` (`CopyFromScreen` 1×1 + `GetPixel`): <https://github.com/microsoft/PowerToys/blob/main/src/modules/colorPicker/ColorPickerUI/Mouse/MouseInfoProvider.cs>
- **F11** — Color Picker, `ZoomWindowHelper.cs` (`CopyFromScreen` 50×50): <https://github.com/microsoft/PowerToys/blob/main/src/modules/colorPicker/ColorPickerUI/Helpers/ZoomWindowHelper.cs>
- **F12** — Screen Ruler, `ScreenCapturing.cpp` (`CreateForMonitor`, `B8G8R8A8UIntNormalized`) e `EdgeDetection.cpp` (`PixelsClose<PerChannel>` com tolerância): <https://github.com/microsoft/PowerToys/tree/main/src/modules/MeasureTool/MeasureToolCore>
- **F13** — Issue 29421, "Color Picker HDR Display Support" (aberto): <https://github.com/microsoft/PowerToys/issues/29421>
- **F14** — Issue 41107, "Color Picker not picking correct color if HDR is on" (fechado como duplicata): <https://github.com/microsoft/PowerToys/issues/41107>
- **F15** — Issue 29949, pedido de opções de gama para SDR em HDR (aberto; texto de usuário, não da Microsoft): <https://github.com/microsoft/PowerToys/issues/29949>
- **F16** — Issue 27940, "Capture HDR Screenshots" (fechado): <https://github.com/microsoft/PowerToys/issues/27940>

Terceiros (indício, não prova):

- **F17** — Pull request 2259 do projeto Fushi, "capture HDR monitors via DXGI FP16 duplication for screen OCR" (descrição da mudança e teste em um monitor HDR real, Windows 11 build 29683; o código não foi lido por mim): <https://github.com/hajisensai/Fushi/pull/2259>
