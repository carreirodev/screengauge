# The Microsoft Store page

Everything ScreenGauge's page on the Microsoft Store needs for its submission, in English only (WPC-341). Nothing here
is sent: the launch (WPC-342) creates the submission in Partner Center and fills it from these files. The package
itself is built by [`packaging/msix`](../msix/README.md).

| What | Where it comes from | Where it goes in Partner Center |
| --- | --- | --- |
| Product name, short description, description, product features | [`listing.md`](listing.md) | Store listings > English > Product name, Supplemental fields > Short description, Description, Product features |
| Screenshots and their captions | the list below; captions in [`listing.md`](listing.md) | Store listings > English > Screenshots (Desktop) |
| Store icon | [`assets/store/StoreIcon.png`](../../assets/store/StoreIcon.png), 300 × 300 | Store listings > English > Store logos > 1:1 App tile icon |
| Store art | `DesignInterface/MS Store images/Store art.png`, on the owner's computer only, 3840 × 2160 | Store listings > English > 16:9 Super hero art |
| Privacy policy | [`listing.md`](listing.md): https://wpcsolutions.com.br/screengauge/privacy/ | Properties > Privacy policy URL |
| Website | [`listing.md`](listing.md): the repository, the page Settings' "Project page" opens | Properties > Website |
| Why the package declares `runFullTrust` | [`listing.md`](listing.md) | Submission options > Restricted capabilities |

The tests in `crates/screengauge/tests/store_listing.rs` hold `listing.md` to Microsoft's limits below, to the name
ScreenGauge (one word, S and G capitalized), to plain English with no links in the page's texts, to none of WPC-286's
goals (no times, sizes or shares of the processor), to the privacy policy both READMEs link to, and to the repository
the app opens. The Store icon's size is checked with its drawing, in `crates/screengauge-icons/tests/assets.rs`.

## The texts

Paste each section of `listing.md` as it is, without its heading. Partner Center takes plain text, so the texts have no
markup and no links: Microsoft asks for "no HTML, code snippets, or URLs in the description field", and gives the
privacy policy and the website fields of their own. Leave "What's new in this version" blank for the first submission.

## The images

- **Store icon:** the set square drawn as orange acrylic, with thickness and reflection (WPC-291), made by
  `screengauge-icons` with the rest of the icon files. If none is uploaded, the Store uses the package's own logo.
- **Store art:** `Store art.png`, a PNG of 3840 × 2160, one of the two sizes Microsoft takes (WPC-294). The Store icon's
  set square, large, near the center and turned 12°, in warm orange light on graphite, measured as the tools measure: a
  protractor behind it, a dimension line along its left edge, a dot on each corner, and a magnifier showing the pixels
  of its long edge. It has no text and no numbers, no monitor or other device, and nothing important in the bottom
  third, where the Store may lay a gradient. The file stays out of git, as `.gitignore` says. The page that draws it
  and the script that renders it sit beside it, in `source/`: `render.ps1 -Variants b` writes `source/hero-b.png`,
  which is copied over `Store art.png`.

## The screenshots

The launch takes five desktop screenshots, in this order, numbered as their captions in `listing.md`. They show only
ScreenGauge and the project's own [test page](../../test-page/README.md): no other program, website, window,
notification or personal file, because the repository is public and so is the Store page (v1 spec, Testing Decisions).

**Setup, the same for every shot:**

1. The AOC 4K monitor at 175%, made the main display in Settings > System > Display, so Chrome opens on it, HDR off, Windows in the light theme, and ScreenGauge's Theme on Automatic. Close every
   other window on that monitor, and turn on Do not disturb, so no notification comes in.
2. Open the test page in a separate Chrome window with an empty profile, so none of the owner's tabs, extensions or
   account can show, placed so the stage sits near the monitor's right edge. Inspect's panel opens below and to the
   right of the cursor and turns to the left only near the monitor's edge: in a full-screen page, the panel of shot 3
   would fall off the stage. In PowerShell 7, from the repository's root:

   ```
   & "C:\Program Files\Google\Chrome\Application\chrome.exe" --user-data-dir="$env:TEMP\screengauge-store-shots" --no-first-run --no-default-browser-check --force-color-profile=srgb --window-position=880,20 --window-size=1250,800 --app=$(([Uri](Resolve-Path test-page\test-page.html).Path).AbsoluteUri)
   ```

3. Each shot is the page's white stage alone: **2100 × 1260** physical pixels (the stage is 1200 × 720 CSS px at 175%),
   found as the white rectangle inside the magenta. In WPC-342 it sat at (1580, 114) on the AOC. The magenta around
   the stage never shows. Microsoft takes PNG files of 1366 × 768 or larger, up to 10 for the desktop, and recommends at
   least four.
4. Put ScreenGauge's toolbar on the stage, where it shows in the shot: in WPC-342, `toolbar_x = 0.93` and
   `toolbar_y = 0.074` in settings.ini put it over the top row's right cards. Keep what each shot is about in the top
   two thirds: the Store may lay text over the bottom third.
5. Take the shot with a delayed grab that doesn't come to the front, such as a script that waits, then copies the screen
   with GDI. The Snipping Tool and Win + Shift + S come to the front, and ScreenGauge closes its tools when another
   program does. Save it as PNG, with no logo or text added on it.

| # | Tool | What is on the stage |
| --- | --- | --- |
| 1 | Dimensions, both directions (C) | The cursor inside `card-200x120`, with the measurement on screen: 350 × 210 physical pixels, 200 × 120 logical. One more measurement pinned with a click, in the gap between `card-gap-24-left` and `card-gap-24-right`. |
| 2 | Bounds | A rectangle dragged loosely around `text-40`, "Measure 1200 px", released so it shrinks to the text and stays pinned with its size. Not `card-on-gradient`: a rectangle that starts on a gradient keeps its size, as the approved answers say (WPC-332). |
| 3 | Inspect, color format HEX and RGB | The cursor on the edge of `shadow-blue`, so the magnified view shows the card, its glow and the white, with the color under the cursor. |
| 4 | Snip | A rectangle being dragged around the text cases, `text-20` to `text-64`, before release. |
| 5 | Settings | Settings, opened from the toolbar's gear, moved onto the stage, with the default options except Snip's folder: choose a folder whose path has no personal name in it, such as `D:\Snips` (a folder at the root of C: needs an administrator), because Settings shows the path and the default one has the Windows user name. The tools close when Settings opens, so the live test page is behind it. |

## Still to fill at launch

These fields aren't page texts, so they stay with the launch (WPC-342) and are only listed here: pricing (free) and
markets, category, the company's contact details (required for a company account), and the age ratings
questionnaire.

## Sources

Microsoft Learn, read on 2026-11-04:

- Store listing fields and their limits: https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/add-and-edit-store-listing-info
- Screenshots, Store logos and the 16:9 Super hero art: https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/screenshots-and-images
- The submission's pages and required fields: https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/create-app-submission
- Restricted capabilities, `runFullTrust` and Packaged COM: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations
