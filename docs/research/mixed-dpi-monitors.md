# Monitores com resolução e escala diferentes no Windows 10 e 11

Pesquisa do ticket WPC-267 ("Como o Windows trata monitores com resolução e escala diferentes") para o Pixmeter, a régua de tela para Windows. Alimenta o ticket de decisão "Unidade de medida e comportamento com vários monitores": este documento só reúne fatos e as opções que eles permitem, não decide nada.

Fontes: somente documentação primária (learn.microsoft.com, support.microsoft.com, o blog oficial de desenvolvedores do Windows e a norma VESA E-EDID). Todas consultadas em **2026-10-26**. Cada fato traz a fonte entre colchetes, como [F1]; a lista completa com os endereços está no fim. O que não consegui confirmar está marcado como **não confirmado**. Contas e deduções minhas (não escritas pela Microsoft) estão marcadas como **dedução**.

## Resumo

1. O Windows trabalha com dois tipos de pixel: o **físico** (o ponto real do monitor) e o **lógico** (uma unidade de "1/96 de polegada lógica"). Na escala 100% os dois coincidem; em 150%, cada pixel lógico vale 1,5 pixel físico [F10].
2. A "polegada lógica" do Windows não é uma polegada de verdade: o tamanho real depende do tamanho do monitor [F10][F11].
3. Cada monitor tem a sua escala. Um app só enxerga a escala e o tamanho real de cada monitor se declarar o modo **Per-Monitor V2** no manifesto (Windows 10 versão 1703 em diante) [F1][F3].
4. Um app que não declara isso recebe números "maquiados" (virtualizados): o Windows reescala coordenadas e tamanhos para o mundo de 96 DPI do app, e a própria Microsoft admite que não documenta direito quais funções fazem isso [F1][F5].
5. O tamanho físico do monitor vem do EDID, um bloco de dados gravado pelo fabricante. A norma grava a largura e a altura **arredondadas ao centímetro** e diz que isso serve para "ter uma ideia aproximada"; existe também um campo em milímetros. Os valores podem faltar (zero) ou estar errados [F38][F31][F32].
6. Uma medida que atravessa a borda entre dois monitores com escalas diferentes dá três respostas diferentes, conforme a unidade: no exemplo deste documento, 400 pixels físicos, 333 pixels lógicos ou 86,5 mm para a mesma linha (seção 6).
7. As opções de unidade são: pixel físico, pixel lógico, centímetro/polegada (com EDID ou calibração) ou mais de uma ao mesmo tempo. Para a borda: bloquear, dividir a medida por monitor ou converter cada trecho separadamente (seção 7).

## 1. Vocabulário: pixel físico, pixel lógico, DPI e escala

**Pixel físico.** É o ponto aceso de verdade na tela. Um monitor 4K tem 3840 × 2160 deles; um Full HD tem 1920 × 1080. A Microsoft também chama esse pixel de "raw pixel" (pixel bruto) [F27].

**DPI no Windows.** Apesar do nome ("pontos por polegada"), no Windows DPI quer dizer "pixels por **polegada lógica**". Por convenção, 1 polegada lógica = 96 pixels; isso é a escala 100% [F10]. O valor base se chama `USER_DEFAULT_SCREEN_DPI` e vale 96; o fator de escala é DPI ÷ 96 [F9][F10]. Tabela publicada pela Microsoft: 96 = 100%, 120 = 125%, 144 = 150%, 192 = 200% [F9][F33]. **Dedução** pela mesma fórmula: 175% = 168 DPI.

**Pixel lógico (DIP).** É a unidade "1/96 de polegada lógica". A conversão documentada é: pixels lógicos = pixels físicos ÷ (DPI ÷ 96) [F10]. Exemplo da Microsoft: a 144 DPI, um retângulo de 200 × 100 pixels lógicos ocupa 300 × 150 pixels físicos [F10].

**A polegada lógica não é uma polegada real.** A Microsoft diz que o tamanho de um pixel depende da resolução e do tamanho físico do monitor e que "não há relação fixa entre polegadas físicas e pixels" [F10]. Na documentação de UI Automation, a polegada de 96 pixels é chamada de "polegada nocional". O exemplo dado: num monitor de 12 polegadas de largura com 1280 pixels, uma linha de 96 pixels mede cerca de 9/10 de polegada [F11].

**Quais escalas existem.** A diretriz de hardware do Windows 10 lista 100%, 125%, 150%, 200%, 250%, 300%, 400% e 500% e não cita 175% [F31]. A enumeração `DEVICE_SCALE_FACTOR` da API inclui `SCALE_175_PERCENT` [F34]. A lista exata que a tela Configurações oferece para cada monitor é **não confirmada** nas fontes primárias. A página de suporte só diz para escolher a opção marcada como "(Recomendado)" [F36].

**A escala é por monitor.** Desde o Windows 8.1, o padrão é cada monitor ter o seu fator de escala [F33] (página antiga, de 8.1, usada aqui só como histórico). No Windows 10 e 11, o suporte da Microsoft manda escolher o monitor antes de mudar escala e resolução [F36][F37].

**A escala recomendada vem do EDID.** O Windows usa o tamanho físico e a resolução nativa informados pelo monitor "para os cálculos de escala de DPI" [F31]. A fórmula exata é **não confirmada**.

## 2. Como um app descobre a escala e os limites de cada monitor

### 2.1 Os quatro modos de "consciência de DPI" (DPI awareness)

Todo app desktop precisa dizer ao Windows como lida com escala. Se não disser nada, o Windows o trata como "sem consciência de DPI" e estica a imagem da janela dele [F1][F5].

| Modo | Desde | O que o app enxerga | O que acontece ao mudar de monitor |
| --- | --- | --- | --- |
| Unaware (sem consciência) | sempre | Todos os monitores parecem ter 96 DPI | O Windows estica a imagem (fica borrada) [F1] |
| System aware (do sistema) | Vista | Todos os monitores parecem ter o DPI do monitor principal no momento em que o usuário entrou no Windows | O Windows estica a imagem (fica borrada) [F1] |
| Per-Monitor (V1) | 8.1 | O DPI do monitor onde está a maior parte da janela | Só a janela principal é avisada; nada é ajustado automaticamente [F1] |
| Per-Monitor V2 | Windows 10 1703 | O DPI do monitor onde está a maior parte da janela | Todas as janelas são avisadas; bordas, menus e diálogos se ajustam sozinhos [F1][F2] |

A Microsoft diz que um processo em Per-Monitor V2 "vê os pixels brutos de cada monitor" e "nunca é esticado pelo Windows" [F1]. Sobre o V1, ela diz que o app "vê todos os monitores em pixels físicos" [F1]. A recomendação oficial é usar o V2; o V1 é descrito como "muito limitado" [F1].

Detalhe que importa para uma régua: o DPI de uma janela é o do monitor "onde a janela está principalmente" [F1][F8]. Uma janela que cobre os dois monitores ao mesmo tempo tem um único DPI.

### 2.2 Como declarar o modo

- **Manifesto do app (recomendado pela Microsoft).** O elemento `<dpiAwareness>` existe desde o Windows 10 1607 e aceita uma lista em ordem de preferência; o valor `PerMonitorV2` só existe nele. O elemento antigo `<dpiAware>` (`true` = system, `true/pm` = per-monitor V1) é ignorado quando `<dpiAwareness>` está presente. Dá para usar os dois juntos, para que versões antigas do Windows caiam no modo antigo [F3].
- **Por código.** `SetProcessDpiAwarenessContext` funciona, mas a Microsoft "não recomenda" e exige chamá-la antes de criar qualquer janela [F3][F10].
- **Por fio de execução (thread).** Desde o 1607, `SetThreadDpiAwarenessContext` deixa cada thread e cada janela principal ter um modo diferente ("mixed mode"). Toda chamada feita depois da troca roda no novo contexto "e pode ser virtualizada" [F4].
- **Padrão:** sem nada declarado, o app é Unaware [F5].

### 2.3 Funções que informam escala e limites

- **Limites de cada monitor:** `GetMonitorInfo` devolve `rcMonitor` (retângulo do monitor) e `rcWork` (área útil sem a barra de tarefas), "em coordenadas da tela virtual". As coordenadas podem ser negativas quando o monitor não é o principal [F17]. Para o DirectX, `DXGI_OUTPUT_DESC.DesktopCoordinates` dá os mesmos limites; a documentação avisa que essas coordenadas "dependem do DPI" [F19].
- **Escala de um monitor:** `GetDpiForMonitor` tem três tipos de leitura [F6][F7]:
  - `MDT_EFFECTIVE_DPI`: a escala escolhida pelo usuário, a que serve para dimensionar a interface;
  - `MDT_ANGULAR_DPI`: sem a escala do usuário;
  - `MDT_RAW_DPI`: "o DPI linear da tela medido na própria tela", também sem a escala do usuário.

  O valor devolvido muda conforme o modo do app: Unaware recebe 96, System recebe o DPI do sistema e Per-Monitor recebe "o valor real" [F6]. A mesma página também diz que essa função "não é consciente de DPI e não deve ser usada" por threads per-monitor, e manda usar `GetDpiForWindow` [F6]. As duas frases se contradizem; registro as duas sem resolver.
- **Escala de uma janela:** `GetDpiForWindow` devolve 96 para janelas Unaware, o DPI do sistema para System e o DPI do monitor onde a janela está para Per-Monitor (Windows 10 1607+) [F8].
- **Aviso de mudança:** `WM_DPICHANGED` chega quando a janela muda para um monitor com outro DPI ou quando o DPI do monitor muda. Ela traz o novo DPI e um retângulo sugerido para a janela [F9].
- **Outras:** `GetScaleFactorForMonitor` devolve um fator de escala da lista `DEVICE_SCALE_FACTOR` [F35][F34]. Algumas funções antigas não sabem de monitor nenhum (por exemplo `GetSystemMetrics`) e respondem sempre no DPI do sistema; elas têm versões novas "ForDpi" [F1][F4].

## 3. O que acontece quando o app NÃO declara Per-Monitor

### 3.1 A janela é esticada como uma foto

Um app Unaware desenha como se a tela fosse 96 DPI. Em telas com escala maior, o Windows "estica a imagem do app até o tamanho físico esperado" e o resultado fica borrado [F1]. Exemplo da Microsoft: a 144 DPI (150%), uma janela que o app criou com 500 × 500 aparece na tela com 750 × 750 pixels [F10]. Um app System aware fica nítido só no DPI do sistema e é esticado (para cima ou para baixo) nos outros monitores [F1][F5].

A partir do Windows 10 1809 existe uma variante, `DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED`, que melhora a nitidez de texto e formas desenhadas com GDI, mas continua sendo um modo Unaware [F2].

### 3.2 Os números que o app recebe são "maquiados" (virtualização)

Quando o app não é Per-Monitor, o Windows converte informações que dependem de DPI para o "espaço de coordenadas" do próprio app. A Microsoft chama isso de virtualização [F1][F5][F39]. Exemplos documentados:

- Um app Unaware que pergunta o tamanho da tela num monitor de alta resolução recebe a resposta "como se a tela estivesse em unidades de 96 DPI" [F1].
- Exemplo oficial com três monitores a 100%, 200% e 300%: uma janela Unaware de 500 × 500 no monitor de 300% ocupa 1500 × 1500 pixels reais. Quem pergunta o tamanho dessa janela recebe 500 × 500 se for Unaware, 1000 × 1000 se for System aware com DPI de sistema 192, e 1500 × 1500 se for Per-Monitor [F5].
- Exemplo da documentação de UI Automation: um botão em (100, 48) aparece nas coordenadas físicas (125, 60) a 120 DPI, mas as coordenadas lógicas continuam (100, 48) [F11].
- Mouse: a mesma página diz que `GetCursorPos` "tipicamente" devolve coordenadas lógicas [F11]. `GetPhysicalCursorPos` devolve a posição em coordenadas físicas [F14]. A página de `GetCursorPos` não fala de DPI [F15].
- Desde o Windows 8.1, `PhysicalToLogicalPoint` e `LogicalToPhysicalPoint` não convertem mais nada. O sistema já entrega tudo no espaço de coordenadas de cada app. Para converter entre apps de modos diferentes existem as versões `...ForPerMonitorDPI` [F12][F13].
- Para apps Per-Monitor, coordenadas lógicas e físicas são idênticas [F13].

**Dedução** a partir desses exemplos: valor recebido = valor físico × (DPI do contexto de quem pergunta ÷ DPI do monitor). Para o Unaware, o DPI do contexto é sempre 96; para o System, é o DPI do monitor principal no momento em que o usuário entrou no Windows.

**A Microsoft admite a lacuna:** "não é fácil saber quais chamadas de API podem devolver valores virtualizados; essa informação não está suficientemente documentada pela Microsoft" [F1].

### 3.3 Captura de tela

- **Per-Monitor V2:** o app "vê os pixels brutos de cada monitor" [F1]. Então as coordenadas e os pixels capturados estão na mesma unidade (**dedução** a partir de [F1] e [F13]).
- **Unaware ou System:** a documentação não descreve o que uma captura feita com GDI (`GetDC`/`BitBlt`) devolve nesse caso: se a imagem vem esticada, encolhida ou cortada. **Não confirmado.** O que está documentado é que as coordenadas e os tamanhos que o app usa para escolher a região são virtualizados [F1][F5]. **Dedução:** nesse modo, as coordenadas que o app conhece e os pixels reais da tela podem não bater.
- **Windows.Graphics.Capture** (a API moderna de captura): captura um monitor ou uma janela. Para capturar um monitor pelo identificador (`HMONITOR`) é preciso o Windows 10 1903 ou mais novo [F23]. O sistema desenha uma borda amarela em volta do que está sendo capturado [F21]. `GraphicsCaptureItem.Size` é descrito só como "o tamanho do alvo da captura", sem dizer a unidade [F22]. Se esse tamanho muda conforme o modo de DPI do app é **não confirmado**.
- **Desktop Duplication (DXGI):** a descrição da saída duplicada traz o "modo de exibição" do monitor (`ModeDesc`) [F20]. **Dedução:** a imagem vem na resolução do modo de vídeo, ou seja, em pixels físicos.

## 4. Dá para saber o tamanho físico do monitor (centímetros ou polegadas)?

### 4.1 De onde vem a informação: o EDID

O EDID é um bloco de dados que o fabricante grava no monitor e que todo monitor deve ter. Ele segue um formato da VESA [F32]. O Windows lê o EDID e, desde o Windows 11, também o DisplayID 2.0, um formato mais novo que o Windows 11 prefere quando está presente [F31]. Para o Windows, o "tamanho físico" no descritor é **obrigatório** e "usado nos cálculos de escala de DPI". A mesma diretriz avisa: "projetores podem informar o tamanho físico como zero" [F31].

O que a norma VESA E-EDID (Release A, Rev. 2, de 25/09/2006) define [F38]:

- **Bloco principal, bytes 15h e 16h:** largura e altura da tela "arredondadas ao centímetro mais próximo", de 1 a 255 cm. Esses bytes podem trazer só a proporção da tela (por exemplo 16:9) em vez do tamanho, e 00h/00h quer dizer "tamanho desconhecido ou indefinido". A norma diz que o computador deve usar esse dado "para ter uma ideia aproximada do tamanho da imagem e gerar texto e ícones na escala certa". Ou seja, o campo não foi feito para medição precisa.
- **Descritor de temporização detalhada:** largura e altura da imagem em **milímetros**, com 12 bits (0 a 4095 mm). A norma diz que os fabricantes "devem incluir" esse tamanho em milímetros.
- **Exceção:** em certos produtos, como projetores com zoom, os dois campos ficam indefinidos e valem 0 mm e 0 cm.
- **O que é medido:** o tamanho de tela é "a área retangular onde a luz pode ser controlada", ou seja, a área ativa, sem a moldura.

### 4.2 O EDID pode estar errado

A Microsoft documenta que "informação de EDID incorreta ou inválida" na memória do monitor existe e causa problemas. Por isso há um mecanismo oficial (arquivo INF, às vezes distribuído pelo Windows Update) para o fabricante substituir o EDID. Quando existe, a substituição tem prioridade sobre o que está gravado no monitor [F32]. Nenhuma fonte da Microsoft dá garantia de precisão para o tamanho físico. **Não confirmado** o quanto os monitores do mercado erram na prática.

### 4.3 Funções que expõem o tamanho físico

- `DisplayMonitor` (WinRT, desde o Windows 10 1803) traz informações "do EDID e do DisplayID" [F28]:
  - `PhysicalSizeInInches` devolve um valor nulo quando o tamanho é desconhecido. A documentação diz que é "o tamanho diagonal", mas o tipo do dado é `Size` (largura e altura). A inconsistência está na própria documentação; o significado real é **não confirmado** [F29].
  - `RawDpiX` e `RawDpiY`: "DPI físico baseado na resolução nativa e no tamanho físico do monitor" [F28].
  - `NativeResolutionInRawPixels`: a resolução nativa em pixels físicos [F28].
  - `GetDescriptor`: devolve os bytes crus do EDID ou do DisplayID [F30].
- `DisplayInformation` (WinRT) acompanha a janela, não um monitor fixo [F24]:
  - `RawDpiX` "pode devolver 0 quando o monitor não informa dimensões físicas e quando o usuário duplica a imagem em vários monitores" [F25];
  - `DiagonalSizeInInches` pode ser nulo "com projetor ou com telas duplicadas" [F26];
  - `RawPixelsPerViewPixel` dá quantos pixels físicos há em cada pixel lógico [F27].
- `GetDpiForMonitor` com `MDT_RAW_DPI`: o DPI "medido na própria tela"; a Microsoft avisa que esse valor "não tem garantia de ser um valor de DPI suportado" [F7]. Todas as leituras de `MONITOR_DPI_TYPE` "são afetadas" pelo modo de DPI do app [F7]. Se um app Unaware recebe 96 também na leitura bruta é **não confirmado**.
- `GetDeviceCaps` com `HORZSIZE`/`VERTSIZE` (largura e altura em mm): com drivers de vídeo modernos (WDDM, desde o Vista), o GDI "não recebe a informação" e "precisa sempre calcular". O cálculo pode assumir 96 DPI fixo, e um driver que informa errado faz a função devolver o valor errado [F18]. Além disso, `LOGPIXELSX` é o mesmo para todos os monitores, e o contexto da área de trabalho devolve dados do monitor principal [F18]. **Dedução:** essas leituras não servem para medir.
- Como ligar o identificador de monitor do Win32 (`HMONITOR`) ao objeto `DisplayMonitor`: a documentação diz que a maioria das APIs Win32 fornece o "identificador de interface" do monitor, que serve para `FromInterfaceIdAsync` [F28]. Qual API Win32 entrega esse identificador para um `HMONITOR` é **não confirmado**.

### 4.4 Resolução diferente da nativa

O monitor aceita resoluções menores que a nativa e estica a imagem; o texto perde nitidez [F36]. **Dedução:** se o usuário põe um monitor 4K de 27" em 1920 × 1080, cada pixel que o Windows desenha ocupa cerca de 0,311 mm, e não 0,156 mm. A conversão para milímetros precisa usar a resolução atual, não a nativa. Usar a nativa (que é a base do `RawDpi` [F28]) erraria por um fator de 2.

## 5. Medindo através da borda entre dois monitores

**A tela virtual.** O Windows junta todos os monitores num único retângulo chamado "tela virtual". O monitor principal contém o ponto (0, 0). Monitores à esquerda ou acima dele têm coordenadas negativas. As coordenadas são números de 16 bits com sinal (de −32768 a 32767) [F16]. Os limites de cada monitor vêm nessas coordenadas [F17].

**A disposição é feita pelo usuário.** O usuário arrasta os monitores em Configurações para o mouse "passar suavemente" de um para o outro [F37]. **Dedução:** o Windows só conhece a disposição em pixels. Nenhuma fonte diz que ele sabe a distância física entre os monitores, a largura das molduras ou se as telas estão alinhadas de verdade.

**Em Per-Monitor V2, tudo é pixel físico.** O app vê os pixels brutos de cada monitor [F1]. **Dedução:** os retângulos da tela virtual também chegam em pixels físicos. Exemplo: 4K principal em (0, 0)–(3840, 2160) e Full HD à direita a partir de x = 3840. Isso é uma dedução de [F1] e [F17]; não encontrei frase da Microsoft que diga isso com essas palavras.

**Em modos não Per-Monitor, a tela virtual é virtualizada.** Como a tela virtual fica arrumada em pixels lógicos quando os monitores têm escalas diferentes (se sobram buracos ou se as áreas se sobrepõem) é **não confirmado**. A Microsoft só diz que os valores são convertidos para o espaço do app [F1][F39].

**O problema das unidades misturadas.** Numa linha que começa no 4K e termina no Full HD, cada pixel físico tem um tamanho real diferente de cada lado da borda (cerca de 0,156 mm e 0,277 mm no exemplo abaixo). Somar pixels dos dois lados dá um número que não corresponde nem a um tamanho real nem a um tamanho lógico. A seção 6 mostra os números.

## 6. Exemplos numéricos

**Monitores do exemplo (hipotéticos, para fazer a conta):**

| Monitor | Resolução | Área ativa (cálculo pela diagonal 16:9) | Tamanho de 1 pixel físico | Densidade real |
| --- | --- | --- | --- | --- |
| 4K de 27" | 3840 × 2160 | 597,7 × 336,2 mm | 0,1557 mm | 163,2 pixels por polegada |
| Full HD de 24" | 1920 × 1080 | 531,3 × 298,9 mm | 0,2767 mm | 91,8 pixels por polegada |

**Cenários de escala:**

- Cenário 1: 4K a 150% (144 DPI) como monitor principal e Full HD a 100% (96 DPI).
- Cenário 2: 4K a 175% (168 DPI) como monitor principal e Full HD a 125% (120 DPI).

Os valores do modo System aware usam como DPI do sistema o do 4K, que é o monitor principal [F1].

### 6.1 O mesmo botão nos dois monitores

Um botão desenhado com **100 × 40 pixels lógicos**, num app que ajusta a escala corretamente. Contas feitas com as fórmulas de [F10] e a regra deduzida em 3.2.

| Onde o botão está | Pixels físicos na tela | Tamanho real | Um app Unaware mede | Um app System aware mede | Um app Per-Monitor V2 mede |
| --- | --- | --- | --- | --- | --- |
| 4K a 150% | 150 × 60 | 23,35 × 9,34 mm | 100 × 40 | 150 × 60 | 150 × 60 |
| 4K a 175% | 175 × 70 | 27,24 × 10,90 mm | 100 × 40 | 175 × 70 | 175 × 70 |
| Full HD a 100% | 100 × 40 | 27,67 × 11,07 mm | 100 × 40 | 150 × 60 (cenário 1) | 100 × 40 |
| Full HD a 125% | 125 × 50 | 34,59 × 13,84 mm | 100 × 40 | 175 × 70 (cenário 2) | 125 × 50 |

O que a tabela mostra:

- Em pixels lógicos, o botão tem sempre 100 × 40, em qualquer monitor e escala. Mas, a 150%, isso dá 23 mm no 4K e 28 mm no Full HD: o mesmo número lógico corresponde a tamanhos reais diferentes.
- Em pixels físicos, o número muda de monitor para monitor (150, 175, 100, 125). É o único valor que corresponde aos pixels de uma captura de tela.
- Um app System aware, no cenário 1, diz que o botão tem 150 de largura no Full HD, onde ele ocupa só 100 pixels reais: os números são reescalados como em [F5].
- "Uma polegada lógica" (96 pixels lógicos) mede de verdade 22,41 mm no 4K a 150%, 26,15 mm no 4K a 175%, 26,57 mm no Full HD a 100% e 33,21 mm no Full HD a 125%. Uma polegada real tem 25,4 mm.

### 6.2 Tamanho da tela em pixels lógicos (e a escala de 175%)

| Monitor e escala | Pixels físicos | Pixels lógicos (físicos ÷ escala) |
| --- | --- | --- |
| 4K a 150% | 3840 × 2160 | 2560 × 1440 |
| 4K a 175% | 3840 × 2160 | 2194,29 × 1234,29 |
| Full HD a 100% | 1920 × 1080 | 1920 × 1080 |
| Full HD a 125% | 1920 × 1080 | 1536 × 864 |

Com 125% e 175%, a maioria das conversões de físico para lógico dá número quebrado. Um traço de 1 pixel físico vale 0,67 pixel lógico a 150% e 0,57 a 175%. Como o Windows arredonda as coordenadas inteiras que entrega a apps virtualizados é **não confirmado**. O exemplo de código da Microsoft usa `MulDiv`, uma divisão com arredondamento [F1][F9].

O menor passo que uma medida consegue ter é 1 pixel físico: cerca de 0,156 mm no 4K e 0,277 mm no Full HD. Em pixels lógicos a 150%, um passo de 1 pixel lógico no 4K já vale 0,233 mm.

### 6.3 Uma linha que atravessa a borda

Uma linha horizontal com 200 pixels físicos no 4K e mais 200 pixels físicos no Full HD:

| Unidade | Cenário 1 (150% / 100%) | Cenário 2 (175% / 125%) |
| --- | --- | --- |
| Pixels físicos somados | 400 | 400 |
| Pixels lógicos (cada lado na sua escala) | 133,33 + 200 = 333,33 | 114,29 + 160 = 274,29 |
| Milímetros (cada lado com o seu tamanho de pixel) | 31,1 + 55,3 = 86,5 mm | 31,1 + 55,3 = 86,5 mm |

A mesma linha dá três números diferentes. O valor em milímetros não muda com a escala, mas exige conhecer o tamanho físico dos dois monitores. Mesmo assim, ele ignora a moldura e o espaço entre as telas, que o Windows não conhece (seção 5).

### 6.4 Precisão do tamanho físico vindo do EDID

| Monitor | Tamanho real (hipotético) | Bloco principal (cm, arredondado) | Erro | Campo em mm | Erro |
| --- | --- | --- | --- | --- | --- |
| 4K de 27" | 597,7 × 336,2 mm | 60 × 34 cm | +0,38% na largura, +1,12% na altura | 598 × 336 mm | cerca de 0,05% |
| Full HD de 24" | 531,3 × 298,9 mm | 53 × 30 cm | −0,25% na largura, +0,38% na altura | 531 × 299 mm | cerca de 0,06% |

Na prática: um objeto de exatamente 100 mm na tela do 4K seria medido como 100,38 mm na horizontal e 101,12 mm na vertical, se o app usasse o tamanho em centímetros do bloco principal. Com o campo em milímetros, o erro de arredondamento cai para menos de 0,1 mm. Isso só vale se o fabricante tiver gravado valores corretos (seção 4.2).

## 7. Opções que os fatos permitem

Esta seção não decide nada; lista o que é possível e as consequências de cada caminho, para o ticket de decisão.

**Fato que vale para qualquer opção.** Sem o modo Per-Monitor V2, o app recebe coordenadas reescaladas e a Microsoft admite não documentar quais funções fazem isso [F1]. As coordenadas que o app conhece e os pixels reais da tela podem não bater (seção 3).

### 7.1 Unidade de medida

| Opção | O que ela mede | A favor | Contra |
| --- | --- | --- | --- |
| A. Pixel físico | Pontos reais da tela | Exata; corresponde 1 para 1 a uma captura de tela; não depende de EDID | O mesmo elemento dá números diferentes em monitores com escalas diferentes (150 × 60 contra 100 × 40) |
| B. Pixel lógico (físico ÷ escala do monitor) | A unidade que a interface do Windows usa para dimensionar elementos [F10] | O mesmo botão dá o mesmo número em qualquer monitor (100 × 40) | Número quebrado com 125% e 175%; não corresponde a um tamanho real |
| C. Centímetro, milímetro ou polegada | Tamanho real na tela | É a unidade de uma régua de verdade | Depende do EDID: aproximado no bloco principal, pode faltar (0 ou nulo) ou estar errado [F25][F26][F31][F38]. Precisaria de calibração manual como plano B (por exemplo, o usuário encosta uma régua ou um cartão de tamanho conhecido na tela). Muda se a resolução não for a nativa |
| D. Mais de uma ao mesmo tempo | Por exemplo, físico como principal e lógico ou mm ao lado | Cada usuário lê a unidade que precisa | Mais informação na tela; é preciso escolher qual é a principal |

### 7.2 Medidas que atravessam a borda entre monitores

| Opção | Como funciona | Consequência |
| --- | --- | --- |
| 1. Bloquear ou avisar | A medida para na borda, ou mostra um aviso quando a cruza | Simples e honesto; limita quem trabalha com janelas entre dois monitores |
| 2. Dividir por monitor | Mostra um valor para cada trecho, cada um na unidade do seu monitor | Nenhum número enganoso; a leitura fica mais complexa |
| 3. Somar pixels físicos da tela virtual | Mostra os 400 pixels da seção 6.3 | Fácil de calcular; mistura pixels de tamanhos reais diferentes |
| 4. Converter cada trecho e somar | Converte cada trecho para pixel lógico ou mm e soma (333,33 ou 86,5 mm na seção 6.3) | Coerente dentro da unidade escolhida. Em mm, exige o tamanho físico dos dois monitores e ignora molduras e o espaço entre eles |

## 8. O que ficou não confirmado

- A lista exata de escalas que Configurações oferece no Windows 10 e no 11, e se 175% sempre aparece. Só há a enumeração `DEVICE_SCALE_FACTOR` [F34] e a diretriz de hardware que não cita 175% [F31].
- A fórmula com que o Windows escolhe a escala "(Recomendado)" a partir do EDID [F31].
- Como o Windows arredonda coordenadas virtualizadas quando a escala gera números quebrados (125%, 175%).
- O que `GetDC`/`BitBlt` captura para um app Unaware ou System aware num monitor com escala.
- A unidade de `GraphicsCaptureItem.Size` e se ela depende do modo de DPI do app [F22].
- A arrumação da tela virtual em pixels lógicos com escalas diferentes (buracos ou sobreposições) para apps não Per-Monitor.
- Que `rcMonitor` venha em pixels físicos no modo Per-Monitor V2: é dedução de [F1] e [F17], sem frase explícita da Microsoft.
- O significado real de `DisplayMonitor.PhysicalSizeInInches` (diagonal ou largura × altura) [F29].
- Se a leitura `MDT_RAW_DPI` também é reescalada para apps Unaware [F6][F7].
- Qual API Win32 liga um `HMONITOR` a um `DisplayMonitor` [F28].
- O quanto o EDID dos monitores reais erra na prática.
- Diferenças entre Windows 10 e 11 nesse modelo: não encontrei nenhuma além do DisplayID 2.0 [F31] e de um ajuste de escala de cursores no Windows 11 [F1].

## Fontes

Todas consultadas em 2026-10-26.

- **F1** — High DPI Desktop Application Development on Windows (atualizada em 2026-09-10): <https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows>
- **F2** — DPI_AWARENESS_CONTEXT handle: <https://learn.microsoft.com/en-us/windows/win32/hidpi/dpi-awareness-context>
- **F3** — Setting the default DPI awareness for a process: <https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process>
- **F4** — Mixed-Mode DPI Scaling and DPI-aware APIs: <https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-improvements-for-desktop-applications>
- **F5** — PROCESS_DPI_AWARENESS: <https://learn.microsoft.com/en-us/windows/win32/api/shellscalingapi/ne-shellscalingapi-process_dpi_awareness>
- **F6** — GetDpiForMonitor: <https://learn.microsoft.com/en-us/windows/win32/api/shellscalingapi/nf-shellscalingapi-getdpiformonitor>
- **F7** — MONITOR_DPI_TYPE: <https://learn.microsoft.com/en-us/windows/win32/api/shellscalingapi/ne-shellscalingapi-monitor_dpi_type>
- **F8** — GetDpiForWindow: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforwindow>
- **F9** — WM_DPICHANGED: <https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged>
- **F10** — DPI and device-independent pixels: <https://learn.microsoft.com/en-us/windows/win32/learnwin32/dpi-and-device-independent-pixels>
- **F11** — Understanding Screen Scaling Issues (UI Automation): <https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-screenscaling>
- **F12** — PhysicalToLogicalPoint: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-physicaltologicalpoint>
- **F13** — PhysicalToLogicalPointForPerMonitorDPI: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-physicaltologicalpointforpermonitordpi>
- **F14** — GetPhysicalCursorPos: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getphysicalcursorpos>
- **F15** — GetCursorPos: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getcursorpos>
- **F16** — The Virtual Screen: <https://learn.microsoft.com/en-us/windows/win32/gdi/the-virtual-screen>
- **F17** — MONITORINFO: <https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-monitorinfo>
- **F18** — GetDeviceCaps: <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-getdevicecaps>
- **F19** — DXGI_OUTPUT_DESC: <https://learn.microsoft.com/en-us/windows/win32/api/dxgi/ns-dxgi-dxgi_output_desc>
- **F20** — DXGI_OUTDUPL_DESC: <https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_outdupl_desc>
- **F21** — Screen capture (Windows.Graphics.Capture): <https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture>
- **F22** — GraphicsCaptureItem.Size: <https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscaptureitem.size>
- **F23** — IGraphicsCaptureItemInterop::CreateForMonitor: <https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createformonitor>
- **F24** — DisplayInformation: <https://learn.microsoft.com/en-us/uwp/api/windows.graphics.display.displayinformation>
- **F25** — DisplayInformation.RawDpiX: <https://learn.microsoft.com/en-us/uwp/api/windows.graphics.display.displayinformation.rawdpix>
- **F26** — DisplayInformation.DiagonalSizeInInches: <https://learn.microsoft.com/en-us/uwp/api/windows.graphics.display.displayinformation.diagonalsizeininches>
- **F27** — DisplayInformation.RawPixelsPerViewPixel: <https://learn.microsoft.com/en-us/uwp/api/windows.graphics.display.displayinformation.rawpixelsperviewpixel>
- **F28** — DisplayMonitor: <https://learn.microsoft.com/en-us/uwp/api/windows.devices.display.displaymonitor>
- **F29** — DisplayMonitor.PhysicalSizeInInches: <https://learn.microsoft.com/en-us/uwp/api/windows.devices.display.displaymonitor.physicalsizeininches>
- **F30** — DisplayMonitor.GetDescriptor: <https://learn.microsoft.com/en-us/uwp/api/windows.devices.display.displaymonitor.getdescriptor>
- **F31** — Display (diretrizes de hardware do Windows: escala e descritores EDID/DisplayID): <https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/display>
- **F32** — Using an INF File to Override EDIDs: <https://learn.microsoft.com/en-us/windows-hardware/drivers/display/overriding-monitor-edids>
- **F33** — DPI-related APIs and registry settings (conteúdo do Windows 8.1, usado só como histórico): <https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/dpi-related-apis-and-registry-settings>
- **F34** — DEVICE_SCALE_FACTOR: <https://learn.microsoft.com/en-us/windows/win32/api/shtypes/ne-shtypes-device_scale_factor>
- **F35** — GetScaleFactorForMonitor: <https://learn.microsoft.com/en-us/windows/win32/api/shellscalingapi/nf-shellscalingapi-getscalefactorformonitor>
- **F36** — Suporte Microsoft, "Change your screen resolution and layout in Windows": <https://support.microsoft.com/en-us/windows/hardware/display-graphics/change-your-screen-resolution-and-layout-in-windows>
- **F37** — Suporte Microsoft, "How to use multiple monitors in Windows": <https://support.microsoft.com/en-us/windows/hardware/display-graphics/how-to-use-multiple-monitors-in-windows>
- **F38** — VESA Enhanced Extended Display Identification Data Standard, Release A, Revision 2 (25/09/2006), seção 3.6.2 e notas da tabela 3.21. A VESA lista essa norma entre as gratuitas em <https://vesa.org/vesa-standards/> (o download oficial passa por um formulário). O texto lido foi uma cópia não oficial do mesmo documento: <https://glenwing.github.io/docs/VESA-EEDID-A2.pdf>
- **F39** — Windows Developer Blog (blog oficial da Microsoft), Peter Felts, "High DPI Scaling Improvements for Desktop Applications and 'Mixed Mode' DPI Scaling in the Windows 10 Anniversary Update (1607)", 24/10/2016: <https://blogs.windows.com/windowsdeveloper/2016/10/24/high-dpi-scaling-improvements-for-desktop-applications-and-mixed-mode-dpi-scaling-in-the-windows-10-anniversary-update/>
