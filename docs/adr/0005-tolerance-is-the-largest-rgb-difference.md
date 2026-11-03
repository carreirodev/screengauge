# The tolerance is the largest difference of red, green or blue, from 0 to 100, at 6 by default

A pixel counts as the same area as the reference color while none of its red, green or blue differs from the reference color's by more than the tolerance. The tolerance is that difference itself, in the numbers Inspect shows (`30, 144, 255`), so a user can check a tolerance with Inspect: a light gray card of 243 on a white of 255 is 12 away, so any tolerance up to 11 finds it. It runs from **0 to 100**, starts at **6**, and a notch of the mouse wheel or a click of Settings' − / + moves it **by 1 up to 10, then by 5** (0, 1, …, 10, 15, 20, …, 100). The rule is the core's (`screengauge_core::settings::Tolerance` and `session::pixels::Reference`). Calibrated with the test page's captures and approved by the owner in WPC-332, on 2026-11-03.

## How it was calibrated

The three captures in `test-page/captures/` (the AOC at 175%, the ARZOPA at 125% and 100%) were measured with Dimensions and Bounds at every tolerance from 0 to 255, case by case: 19 Dimensions cases and 37 Bounds cases on each capture, 168 answers in all.

- **Cards, lines, gaps and text don't decide it.** Chrome draws the page's exact colors, with no noise, so their answers are the same at every tolerance from 0 to about 46.
- **The light cases set the top of the default.** The light gray card (`#F3F3F3` on white, 12 away) is lost from 12 up, the `gradient-subtle` that fades into white from 10, the `#E5E5E5` divider from 26, and the gray outline of `card-outlined` from 47.
- **The faint shadows set its bottom.** Up to 4, Bounds around "Text with a shadow" takes pixels 1 to 4 away from white far from the text: 382 × 43 instead of 238 × 26 at 100%.
- **From 5 to 9, every answer is sound at all three scales.** 6 is inside that range with room on both sides, and is what the prototype's rule gave by default (its 2, times 3).
- **Gradients need more.** The test page's blue-to-purple gradient, crossed from its middle, needs 54, the card on a gradient 66, and the radial gradient is 79 from white at its darkest. 100 holds all of them; above it, only colors far apart would merge, such as the page's blue and white, 225 apart.
- **The steps follow the two ranges.** Steps of 1 up to 10 keep the light cases' precision; steps of 5 above it cross the blue-to-purple gradient in 13 notches from the default, where steps of 1 would take 48.

At 6, the 75 answers whose size the page itself sets (its CSS times the scale, each edge rounded to the nearest pixel as Chrome draws it) match it exactly. The other 93 only a capture can answer: text, shadows, gradients, 1-pixel borders and lines of 1 physical pixel. The owner approved all 168 (the last 3, white text of 12 px on the dark band, after the review), which the tests in `crates/screengauge/src/capture/test_page/answers.rs` hold: a change of rule or of the default that changes one fails there, and the new answer goes back to the owner.

## Considered Options

- **The sum of the three differences**, and **their distance** (the square root of the sum of their squares). Compared at the same gray tolerance, they differ from the largest difference only on colored cases (the blue glow, the red text, the blue-to-purple gradient), by 1 to 6 pixels, and get no card or line right that it gets wrong. The largest difference is the fastest, since each channel's range is worked out once, and the only one Inspect's numbers check directly.
- **The prototype's number, 0 to 50, times 3 inside**: the same default, but the number matches nothing on screen.
- **Steps of 1 throughout**, the simplest, at 48 notches across the strong gradient; **steps of 5 throughout**, which would lose the precision below 10 where the light cases are decided, and have no 6.

## Consequences

- A settings file from before this calibration holds the old number: its `default_tolerance = 2` is now 2, far stricter. Only builds before v1 wrote one.
- The tolerance indicator's bar lights a segment for each 10.
- A value the settings file holds between two steps, such as 13, goes to the next step either way: 15 with +, 10 with −.
