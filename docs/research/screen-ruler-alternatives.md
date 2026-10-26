# O que as réguas de tela atuais oferecem (WPC-265)

> Nota (2026-10-26): depois desta pesquisa, o app foi renomeado de Pixler para Pixmeter, porque "Pixler" soava igual a Pixlr.

Data da consulta: 2026-10-26. Todas as afirmações citam a fonte primária; o que não foi possível confirmar está marcado como "não confirmado".

## Resumo

1. O PowerToys Screen Ruler é a alternativa gratuita mais completa no Windows, mas só existe dentro da suíte PowerToys, cujo instalador x64 tem cerca de 324 MB (v0.102.2933.0) e exige o WebView2 Runtime.
2. O Rooler (que inspirou o Screen Ruler do PowerToys) está parado desde junho de 2024, não publica binários (0 releases) e usa .NET Framework 4.5.2. Registros do próprio repositório indicam valores incorretos fora do monitor principal quando as escalas diferem.
3. O ShareX tem uma régua (Tools → Ruler) e seletores de cor, mas é um aplicativo de captura completo (instalador de 139 MB, GPL-3.0), não uma ferramenta de medição focada.
4. xScope (US$ 49,99) e PixelSnap (US$ 39) são só para Mac; servem de referência de funções: guias, lupa com formatos de cor, medidas fixas, exportar medidas como imagem.
5. Nenhuma alternativa gratuita no Windows reúne medição por detecção de borda, lupa com cor e captura recortada pelo conteúdo em um único programa leve e independente. Esse é o espaço do Pixmeter.
6. Candidatas à v1 além das cinco ferramentas decididas: tolerância de borda ajustável, copiar medida/cor para a área de transferência, unidades extras (DIP, polegadas, mm), formatos de cor (HEX/RGB/HSL), medida persistente (Shift), mover a medida com setas, atalho global configurável.
7. Nenhuma fonte consultada demonstra bom comportamento com monitores de escalas diferentes além de declarações de manifesto; tratar isso bem (PerMonitorV2, testado) é o principal diferencial técnico possível.

## Comparação

### PowerToys Screen Ruler

- **Medição:** modos Bounds, Spacing (cruz), Horizontal e Vertical; clicar copia a medida para a área de transferência; Shift mantém as caixas na tela; a roda do mouse ajusta a tolerância (15 unidades por passo); atalho Win+Ctrl+Shift+M; Ctrl+1/2/3/4 escolhe a ferramenta. Fonte: <https://learn.microsoft.com/en-us/windows/powertoys/screen-ruler>
- **Configurações:** captura contínua (tempo real) ou instantânea, detecção por canal de cor, tolerância 0–255, unidades extras (polegadas, cm, mm, DIP), "pés" na cruz, cor da linha, modo padrão. Mesma fonte.
- **Cor:** o Screen Ruler não inspeciona cor; isso é outro módulo, o Color Picker (Win+Shift+C): zoom com a roda, histórico de até 20 cores, formatos personalizáveis (HEX, RGB, HSL e muitos outros), editor. Limitações documentadas: sem suporte a WCG/HDR. Fonte: <https://learn.microsoft.com/en-us/windows/powertoys/color-picker>
- **Captura de imagem:** o Screen Ruler só copia o texto da medida; não há equivalente ao Snip na documentação do módulo. Mesma fonte do Screen Ruler.
- **Vários monitores e escalas:** o manifesto do componente de medição declara `PerMonitorV2` (<https://github.com/microsoft/PowerToys/blob/main/src/modules/MeasureTool/MeasureToolCore/app.manifest>); as medidas são sempre em pixels físicos e o DIP usa a escala do monitor onde se mede (<https://learn.microsoft.com/en-us/windows/powertoys/screen-ruler>). Qualidade real em configurações mistas: não confirmado. A documentação de desenvolvimento diz haver vários bugs abertos, a maioria de travamentos (<https://github.com/microsoft/PowerToys/blob/main/doc/devdocs/modules/screenruler.md>).
- **Instalação:** Windows 11 ou Windows 10 2004 (build 19041) ou mais novo, x64 ou ARM64, WebView2 Runtime instalado pelo instalador (<https://learn.microsoft.com/en-us/windows/powertoys/install>). Instaladores da v0.102.2933.0 (publicada em 2026-10-21): x64 323.641.880 bytes; por usuário x64 323.616.208 bytes (API de releases de <https://github.com/microsoft/PowerToys/releases>). Licença MIT; repositório ativo (último push em 2026-10-26). Não existe instalação só do Screen Ruler: é um módulo da suíte.

### Rooler (peteblois/rooler)

- **Funções:** Dimensions (borda mais próxima em todas as direções, tolerância ajustável, trata gradientes), Bounds (encolhe um retângulo aproximado até a borda exata), Inspect (lupa ajustável com posição do mouse, cor do pixel e medidas) e Snip ("experimental", copia o conteúdo recortado para a área de transferência). Fonte: <https://github.com/peteblois/rooler>
- **Vários monitores e escalas:** o repositório tem o issue #5 (falha com monitor principal mais baixo que os outros) e os pull requests #2 e #6. O texto do #6 afirma que o WPF usa a escala do monitor principal em todo lugar e que, nos outros monitores, os valores "nativos" ficam incorretos (<https://github.com/peteblois/rooler/pull/6>). Se esses PRs foram mesclados: não confirmado.
- **Instalação:** o README pede "o .NET Framework mais recente"; o código compila para .NET Framework 4.5.2 (`src/app.config`, `src/Rooler.csproj`). Não há releases publicados (0 na API; o issue #4 pede um). É preciso compilar com `msbuild`. Fonte: <https://github.com/peteblois/rooler>. Licença MIT; último push em 2024-06-16 (parado).
- O README diz que o Rooler foi inspirado no xScope; o PowerToys cita o Rooler como inspiração do Screen Ruler (<https://learn.microsoft.com/en-us/windows/powertoys/screen-ruler>).

### xScope (Mac)

- **Funções:** dez ferramentas: Dimensions (mede elementos e distâncias, salva capturas das áreas medidas), Rulers (duas réguas sincronizadas que rotacionam e encaixam nas bordas), Guides (salvas e carregadas), Frames (caixas de marcação que encaixam no conteúdo), Crosshair (coordenadas, origem à escolha), Loupe (lupa com valores de cor em vários formatos), Screens, Overlay, Mirror, Text. Fonte: <https://apps.apple.com/app/id889428659>
- **Vários monitores:** só Guides declara suporte; para as demais a ficha não informa. Mesma fonte.
- **Requisitos e preço:** macOS 12.3+, 28,1 MB, US$ 49,99, versão 4.7.2 (05/08/2025). Mesma fonte. O site oficial (xscopeapp.com) retornou HTTP 403 na consulta automática, por isso só a ficha da App Store foi usada.

### PixelSnap 2 (Mac)

- **Funções:** medir a distância entre elementos, fixar dimensões na tela, medir vários objetos ao mesmo tempo, copiar para a área de transferência; arrastar uma área que encaixa nos limites do objeto; guias que encaixam e mostram a distância; botão direito seleciona sem encaixar; modo de tolerância para sombras e baixo contraste; salvar uma área como imagem; exportar medidas como captura. Fonte: <https://pixelsnap.com/>
- **Vários monitores:** compatível com Retina e com vários monitores; reduz capturas Retina para 1x e mostra dimensões reais independente do zoom. Mesma fonte.
- **Cor:** a página não descreve seletor de cor.
- **Preço e requisitos:** US$ 39 por Mac; macOS 10.13+. Fonte: <https://pixelsnap.com/buy>

### ShareX (Windows)

- **Régua:** Tools → Ruler: sobreposição em tela cheia; medição de linha horizontal/vertical (teclas H, V, espaço alterna o eixo); medição de área com encaixe na borda do conteúdo; tolerância pela roda do mouse; captura estática ao abrir; setas movem a medida 1 px (10 px com Shift); Ctrl+C copia o texto da medida. Unidade: só pixels; a documentação manda ajustar o zoom a 100% e considerar a escala do Windows. Fonte: <https://getsharex.com/docs/ruler>
- **Cor e captura:** Color picker e Screen color picker em "Screen & color"; captura de região, janela, monitor, rolagem; gravação de tela (vídeo e GIF); anotações; envio e fluxos automáticos pós-captura. Fonte: <https://getsharex.com/>
- **Lupa:** a página não lista lupa independente (só "Magnify" como anotação). Mesma fonte.
- **Vários monitores e escalas:** há captura de monitor ativo e menu de monitores; o comportamento da régua com escalas diferentes: não confirmado.
- **Instalação:** instalador x64 de 139.151.207 bytes e versão portátil de 208.850.419 bytes (v21.0.0, 2026-07-03); alvo `net10.0-windows10.0.22621.0` (API de releases e `ShareX/ShareX.csproj` em <https://github.com/ShareX/ShareX>). Licença GPL-3.0 (incompatível com copiar código para um projeto MIT). Repositório ativo (push em 2026-10-26).

### Outras ferramentas de Windows

- **Screen Ruler (Bluegrams):** régua virtual móvel, sem detecção de borda. Mede em pixels, cm, polegadas, pontos e percentual; 1D e 2D; linhas de marcação; mede janelas automaticamente; ângulos; centro, terços e proporção áurea; portátil. Requer Windows 7+ e .NET Framework 4.8+. BSD-3-Clause. Último push em 2024-12-31 e último release v0.10.0 (2022-09-04): sem sinais de atividade em 2026. Fonte: <https://github.com/Bluegrams/ScreenRuler>
- **ZoomIt (Sysinternals/Microsoft):** zoom e desenho para apresentações, LiveZoom, gravação, captura de região para a área de transferência ou arquivo, OCR de região. Não mede nem informa cor. Download de 24,3 MB; página atualizada em 2026-10-15; funciona em todas as versões do Windows. Fonte: <https://learn.microsoft.com/en-us/sysinternals/downloads/zoomit>

## Tabela resumo

| Ferramenta | Medição por borda | Lupa + cor | Captura | Vários monitores/escalas | Instalação | Licença |
|---|---|---|---|---|---|---|
| PowerToys Screen Ruler | Sim | Cor em outro módulo | Não | PerMonitorV2 declarado | ~324 MB, suíte + WebView2 | MIT |
| Rooler | Sim | Sim (Inspect) | Snip (experimental) | Valores nativos incorretos fora do principal (WPF) | Compilar; .NET Fx 4.5.2 | MIT |
| ShareX Ruler | Sim | Cor em ferramentas separadas | Sim (suíte completa) | Não confirmado | 139 MB, .NET 10 | GPL-3.0 |
| Bluegrams Screen Ruler | Não (régua manual) | Não | Não | Não confirmado | .NET Fx 4.8, portátil | BSD-3 |
| ZoomIt | Não | Zoom, sem cor | Sim | Não confirmado | 24,3 MB | Não confirmado |
| xScope (Mac) | Sim | Sim | Sim | Parcial | macOS 12.3+, US$ 49,99 | Pago |
| PixelSnap (Mac) | Sim | Não descrito | Sim | Sim (Retina) | macOS 10.13+, US$ 39 | Pago |

## Funções candidatas para a v1 do Pixmeter

Já decididas: Dimensions, Bounds, Inspect, Snip, Settings.

Candidatas, da mais barata e valiosa para a menos:

1. **Tolerância de borda ajustável** (roda do mouse, valor padrão em Settings). Presente em PowerToys, Rooler, ShareX e PixelSnap; é o que permite medir sombras e gradientes.
2. **Copiar a medida para a área de transferência** com um clique (PowerToys; Ctrl+C no ShareX).
3. **Medida persistente** (Shift) e remoção individual (PowerToys, ShareX, PixelSnap).
4. **Formatos de cor na Inspect** (HEX, RGB, HSL) e cópia por atalho (PowerToys Color Picker, xScope Loupe). Formatos totalmente personalizáveis ficam para depois.
5. **Unidades extras** (DIP, polegadas, cm, mm) em Settings. Só o PowerToys tem; relevante para monitores com escalas diferentes.
6. **Mover a última medida com as setas** (1 px; 10 px com Shift), como no ShareX.
7. **Atalho global configurável** e ferramenta padrão ao ativar (PowerToys).
8. **Snip para a área de transferência ou pasta** (já decidido), com nome de arquivo configurável. O Rooler só copia para a área de transferência e o PowerToys não captura imagem.
9. **Congelar a tela ao abrir** como padrão, com modo contínuo opcional (o PowerToys oferece os dois e avisa que o contínuo consome mais recursos).
10. **Guias e marcação (Frames):** xScope e PixelSnap. Sugestão: depois da v1.
11. **Fora do escopo sugerido:** gravação de tela, envio para a internet, anotações, OCR, desenho (ShareX, ZoomIt), por contrariar "leve" e "nada sai do computador".

## O que diferenciaria o Pixmeter

- **Contra o PowerToys:** instalação independente e pequena (sem suíte nem WebView2), com medição, lupa com cor e captura no mesmo programa. O tamanho do Pixmeter só pode ser declarado depois de medido.
- **Contra o Rooler:** mantido, com releases publicados, sem depender do .NET Framework e com escala por monitor tratada de forma testada (o Rooler registra valores nativos incorretos fora do monitor principal).
- **Contra o ShareX:** foco único em medição, licença MIT em vez de GPL-3.0 e nenhuma função de envio.
- **Contra xScope e PixelSnap:** gratuito, código aberto e para Windows; ambos são pagos e só para Mac.
- **Contra o Bluegrams Screen Ruler e o ZoomIt:** mede por detecção de borda (o primeiro é régua manual, o segundo não mede).
- **Privacidade:** "nada sai do computador" pode ser uma garantia explícita, em contraste com o ShareX, cujo foco inclui envio.

## Não confirmado

- Site oficial do xScope (HTTP 403): usada a ficha da App Store.
- Comportamento real em monitores com escalas diferentes no PowerToys Screen Ruler e no ShareX Ruler (lidos apenas manifesto e documentação; nenhum teste prático).
- Se os PRs de correção de monitores do Rooler (#2, #6) foram mesclados.
- Tamanho do Screen Ruler isolado dentro do PowerToys (não é distribuído separadamente).
- Licença do ZoomIt (a página consultada não a informa).
- Lupa nativa do Windows e outras réguas pequenas (fora da amostra consultada).
