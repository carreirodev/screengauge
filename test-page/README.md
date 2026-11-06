# Test page

`test-page.html` is a page the project made with the cases that are hard to measure. `samples/` holds the test page samples: what it looks like when Chrome draws it on the owner's monitors. The tests without a screen measure these samples, and they calibrate the tolerance (v1 spec WPC-296, Testing Decisions; ticket WPC-331, used by WPC-332).

## The page

The page is a white stage of 1200 × 720 CSS px, 16 CSS px from the window's top-left corner, on a pure magenta (`#FF00FF`) background. Each case is an element with an `id`, and its style gives its place and size on the stage in CSS px, so the HTML is the list of cases:

- **Cards of known size:** `card-200x120`, two cards with a 24 px white strip between them (`card-gap-24-*`), `card-75x45-offgrid` (its place and size fall between physical pixels at 125% and 175%), `card-rounded`, `card-outlined` (1 px gray border), and `card-light-gray` (`#F3F3F3` on white).
- **Text with soft edges:** Segoe UI at 10, 12, 14, 16, 20, 28, 40 and 64 px; white on dark, red, bold, and gray on light gray; and text with a shadow.
- **Shadows:** `shadow-small`, `shadow-large`, and `shadow-blue`, a blue card with a blue glow.
- **Gradients:** `gradient-subtle`, which fades into the white at its right edge, plus `gradient-strong`, `gradient-radial` and `gradient-vertical`, and `card-on-gradient` on top of `gradient-under-card`.
- **1-pixel lines:** lines and boxes of 1 CSS px (`*-css-*`) and of 1 physical pixel (`*-device-*`, sized by `devicePixelRatio`). Also a line placed off the pixel grid, a light divider (`#E5E5E5`), and two lines with 3 px between them.

Nothing on the stage is magenta, and the stage's edge pixels are white.

**Finding a case in a sample:** multiply its CSS place and size by the scale. That is exact for the stage's multiples of 4 CSS px. A case at odd values, such as `card-75x45-offgrid`, lands within a pixel of it, because Chrome snaps its edges to whole physical pixels. At 125%, for example, its 93.75 px width comes out as 94. The exact answer for each case comes from measuring the sample, and the owner approves it once (WPC-332). The page uses no web font, image, animation or hover style, and needs no network, so Chrome draws it the same way each time. When the 125% and 175% samples were taken again in later runs, the program reported the same pixels as the files already there.

## How the samples were taken

The **program that takes them** is an ignored test in ScreenGauge itself, `crates/screengauge/src/capture/test_page.rs`. Run it on the owner's computer, with the monitors at the scales you want samples of:

```
cargo test -p screengauge --release --bin ScreenGauge take_the_test_page_samples -- --ignored --nocapture
```

The program does these steps, in order:

1. It starts a **separate Chrome** from `C:\Program Files\Google\Chrome\Application\chrome.exe`, with a new, empty profile in the temporary folder. The flags are `--user-data-dir=<that folder> --no-first-run --no-default-browser-check --force-color-profile=srgb --app=file:///…/test-page/test-page.html`. The empty profile means none of the owner's tabs, extensions, account or settings can show. App mode means there is no address bar. Zoom is Chrome's default, **100%**. The page asks Chrome not to offer a translation, because its bubble would cover the page.
2. On every monitor in turn, it **maximizes the window on top of every other window**. It then photographs the monitor through **ScreenGauge's own capture** (DXGI, or GDI where DXGI fails), so a sample holds exactly the pixels ScreenGauge measures. It refuses a photo whose colors are approximate (HDR on, with its brightening not undone).
3. It **crops the page alone**. The page must be the one rectangle of 1200 × 720 CSS px × the monitor's scale (for example 2100 × 1260 at 175%) that has magenta all around it and none inside. Otherwise the sample is refused, for example when a window or bubble covers the page's edge, when the zoom isn't 100%, or when the scale isn't the one Windows reports. It then checks what only the page draws there: every edge pixel white, and `card-200x120` exactly in place and exactly blue. That catches a window, tooltip or notification inside the ring that reaches the page's edge or that card. The program keeps the crop only once two photos in a row agree, so Chrome has finished drawing. When it refuses, it saves the monitor's photo in the temporary folder, never in the repository, so you can see why.
4. It writes `samples/<monitor resolution>-<scale>.png`. If that file already exists, it says whether the pixels are the same.

**Format:** PNG, 24-bit color, opaque, lossless, written by the same encoder as the Snip, so each pixel is exactly the photo's.

**Where they are:** `test-page/samples/`. The tests in `test_page.rs` check, on every run and in CI, that:

- the folder holds exactly the three files below;
- each file is the page's size at its scale, in 24-bit color;
- each file has white edges and `card-200x120` in place, as the program checked;
- no file holds a single magenta pixel from outside the page.

## The samples

Taken on 2026-11-03, on Windows 11 26H2 (build 26300), with Chrome **156.0.8078.80**. ClearType was on (`FontSmoothingType` 2) and HDR was off on both monitors.

| File | Monitor | Scale | Page in physical pixels |
| --- | --- | --- | --- |
| `3840x2160-175.png` | AOC U27B3A, 4K (NVIDIA GeForce RTX 3050, per the spec) | 175% | 2100 × 1260 |
| `1920x1080-125.png` | ARZOPA, Full HD (AMD Radeon 760M, per the spec) | 125% | 1500 × 900 |
| `1920x1080-100.png` | ARZOPA, Full HD | 100% | 1200 × 720 |

Measured in the samples, Chrome put every card on whole physical pixels, with sharp edges and the page's exact colors (`#1E90FF` came out `#1E90FF`). For example, `card-200x120` is 250 × 150 at 125% and 350 × 210 at 175%. A 1 CSS px line is 1 physical pixel at 125% and 2 at 175%. A `*-device-*` line is 1 physical pixel at every scale.

## The approved answers

The samples calibrated the tolerance: the rule, its limits, its steps and its default, 6. ADR `docs/adr/0005-tolerance-is-the-largest-rgb-difference.md` records the numbers and the reasons (WPC-332).

`crates/screengauge/src/capture/test_page/answers.rs` holds each case's answer at the default tolerance, on each sample, as the owner approved it on 2026-11-03: 19 Dimensions cases and 37 Bounds cases, 168 answers in all. The tests run every case through the session core, as the tools measure the real screen, and check that each Snip drag takes the sample's pixels in the rectangle its Bounds drag measures. 75 of the answers are sizes the page itself sets, its CSS times the scale with each edge rounded to the nearest pixel, and match them exactly. The other 93 only a sample can answer: text, shadows, gradients, 1-pixel borders and lines of 1 physical pixel.

When a change of rule or of the default tolerance changes an answer, the test fails and lists each changed answer next to the approved one. The new answers go back to the owner, and only the owner's approval writes them into the table.

Only this page is ever captured here. Pictures of other programs or websites never enter the repository, because it is public and such a picture could carry something personal or another company's.
