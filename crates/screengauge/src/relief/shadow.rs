//! CSS box shadows with a blur, as rounded rectangles laid over each other (the toolbar's, WPC-310, and Settings').
//! Direct2D's render targets have no blur of their own, so a shadow is drawn the way the notice's is: the shape grown
//! by a little more each time, every layer faint, so that where the layers overlap their opacities add up to the
//! strength a gaussian blur would leave there. This module works the opacities out; what to fill with them is in
//! `relief`. It needs no Windows, so it is tested without a graphics card.
//!
//! What is approximated, honestly: the blur is taken as along a straight edge, or across the short side of a box, and
//! applied by distance from the shape's outline, so a corner comes out a little too dark and the end of a long box a
//! little too light, against what a true blur gives. Each layer's outline is on whole pixels, so its edge is crisp and
//! the fall-off steps down in bands, `Layers::step` pixels wide, rather than smoothly: with the bands a twentieth of
//! the blur wide, each step is a few levels of an 8-bit channel. The frozen screen's labels and panels have wider bands
//! (`COARSE`), as their layers are few enough not to round away.

use std::f64::consts::SQRT_2;

/// What the layers are fitted to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Profile {
    /// The edge of a shape much bigger than the blur: a long side of an inset shadow's hole, or of a large panel.
    Edge,
    /// A box whose short side is this many pixels, seen along the middle of that side: its two edges are not far
    /// enough apart for the blur to leave the middle at full strength.
    Strip(f64),
}

/// One layer: the shape grown by `grow` pixels on every side (or smaller, should it be negative), filled at `opacity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    pub grow: i32,
    pub opacity: f32,
}

/// The layers of one shadow, innermost first.
#[derive(Debug, Clone, PartialEq)]
pub struct Layers {
    pub layers: Vec<Layer>,
    /// How much bigger each layer is than the one inside it.
    pub step: i32,
}

impl Layers {
    /// How far past the shape the outermost layer reaches, in pixels: nothing of the shadow is drawn beyond.
    pub fn reach(&self) -> i32 {
        self.layers
            .iter()
            .map(|layer| layer.grow)
            .max()
            .unwrap_or(0)
            .max(0)
    }
}

/// The error function, to 1.5e-7 (Abramowitz and Stegun 7.1.26). Rust's standard library has none.
fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    (1.0 - poly * (-x * x).exp()).copysign(x)
}

/// The share of a standard normal distribution below `x`.
pub fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / SQRT_2))
}

/// How much of the shadow's full strength a blur of standard deviation `sigma` leaves `distance` pixels outside the
/// shape's outline (inside, should it be negative).
fn coverage(profile: Profile, sigma: f64, distance: f64) -> f64 {
    match profile {
        Profile::Edge => normal_cdf(-distance / sigma),
        Profile::Strip(short) => {
            normal_cdf((short + distance) / sigma) - normal_cdf(distance / sigma)
        }
    }
}

/// The layers for a shadow of `strength` (the color's opacity, 0 to 1) blurred with a standard deviation of `sigma`
/// pixels, which is half of CSS's blur radius.
///
/// The layers' outlines are `step` pixels apart. The opacity of each is worked out so that, in the middle of the band
/// between its outline and the next one in, the layers stacked there (a layer over another leaves `1 - (1 - a)(1 - b)`)
/// add up to what the blur leaves at that distance, reading from the outermost layer in. Beyond the outermost, which is
/// three standard deviations out, the blur leaves under a quarter of one level of an 8-bit channel.
///
/// With no blur there is one layer, at the strength, on the outline itself.
pub fn layers(sigma: f64, strength: f32, profile: Profile) -> Layers {
    layers_in_bands(sigma, strength, profile, FINE)
}

/// About twenty bands to each standard deviation of the blur, `layers`'s: the steps between them are small. The
/// toolbar's and Settings' shadows, drawn once onto a bitmap of their own, have them.
pub const FINE: f64 = 20.0;

/// Four bands to each standard deviation of the blur: the frozen screen's labels and panels, drawn each frame straight
/// onto the screen's photo (WPC-393). Each layer is fainter the finer the bands, and every layer a channel of 8 bits
/// takes in rounds by up to half a level, so the many faint layers of fine bands add up to a shadow darker than the
/// blur: measured against CSS's true blur of the notice, the tolerance indicator and the magnified view, in both
/// palettes at 100%, 150% and 175%, fine bands were 0.035 of opacity off it, root mean square, and these 0.021, the
/// nearest of the widths tried (from 2.5 to 20 to a deviation). The notice's shadow had been fitted with bands 4 pixels
/// wide, at a deviation of 14, before it came here.
pub const COARSE: f64 = 4.0;

/// As `layers`, with `bands` bands to each standard deviation of the blur, never one finer than a pixel.
pub fn layers_in_bands(sigma: f64, strength: f32, profile: Profile, bands: f64) -> Layers {
    if sigma < 1e-3 {
        return Layers {
            layers: vec![Layer {
                grow: 0,
                opacity: strength,
            }],
            step: 1,
        };
    }
    let step = ceil_to_pixels((sigma / bands).max(1.0));
    let outer = ceil_to_pixels(3.0 * sigma);
    // Inside the outline the fall-off is the same up to three deviations in. A narrow box has no room for that: it
    // keeps a core of its own, a pixel from the middle, so no layer collapses to nothing.
    let depth = match profile {
        Profile::Edge => outer,
        Profile::Strip(short) => outer.min(ceil_to_pixels((short / 2.0 - 1.0).max(0.0))),
    };
    let strength = f64::from(strength);
    let count = (depth + outer + step - 1) / step + 1;
    let at = |k: i32| -depth + k * step;
    // The target for the band whose outer outline is layer `k`'s: read at the band's middle.
    let target =
        |k: i32| strength * coverage(profile, sigma, f64::from(at(k)) - f64::from(step) / 2.0);
    // From the outermost layer in, because each layer's opacity depends on what the ones outside it add up to. A layer
    // that would not change a channel by half a level is not drawn, and what it would add goes to the next one in, so
    // the sums inside are still right.
    let mut layers = Vec::new();
    let mut carried = 0.0;
    for k in (0..count).rev() {
        let outside = if k + 1 < count { target(k + 1) } else { 0.0 };
        // What this layer must add under the layers outside it, which lay `outside` there already.
        let opacity = if outside >= 1.0 {
            0.0
        } else {
            1.0 - (1.0 - target(k)) / (1.0 - outside)
        };
        let opacity = 1.0 - (1.0 - opacity) * (1.0 - carried);
        if opacity > 1.0 / 512.0 {
            layers.push(Layer {
                grow: at(k),
                opacity: narrow(opacity.min(1.0)),
            });
            carried = 0.0;
        } else {
            carried = opacity;
        }
    }
    layers.reverse();
    Layers { layers, step }
}

/// A whole number of pixels, rounded up. Shadows reach a few hundred pixels at most, far inside `i32`.
// TODO: place the layers without going through a float, so no float comes back to `i32` here.
fn ceil_to_pixels(distance: f64) -> i32 {
    debug_assert!((0.0..1e6).contains(&distance), "{distance} pixels");
    distance.ceil() as i32
}

/// An opacity between 0 and 1, which an `f32` holds to far more digits than a channel of 8 bits has.
// TODO: work in `f32` throughout once the standard library has an error function, so nothing narrows here.
fn narrow(opacity: f64) -> f32 {
    debug_assert!((0.0..=1.0).contains(&opacity), "{opacity}");
    opacity as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_function_matches_known_values() {
        for (x, wanted) in [
            (0.0, 0.0),
            (0.5, 0.520_499_877_8),
            (1.0, 0.842_700_792_9),
            (2.0, 0.995_322_265_0),
            (-1.0, -0.842_700_792_9),
        ] {
            assert!((erf(x) - wanted).abs() < 2e-7, "erf({x}) is {}", erf(x));
        }
    }

    #[test]
    fn the_normal_distribution_is_half_at_zero_and_symmetric() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-9);
        assert!((normal_cdf(1.0) - 0.841_344_746).abs() < 2e-7);
        assert!((normal_cdf(-1.0) + normal_cdf(1.0) - 1.0).abs() < 1e-7);
        assert!(normal_cdf(-6.0) < 1e-8 && normal_cdf(6.0) > 1.0 - 1e-8);
    }

    /// What the layers add up to at `distance` outside the outline: the opacity of everything that covers it.
    fn stacked(layers: &Layers, distance: f64) -> f64 {
        1.0 - layers
            .layers
            .iter()
            .filter(|layer| f64::from(layer.grow) > distance)
            .map(|layer| 1.0 - f64::from(layer.opacity))
            .product::<f64>()
    }

    #[test]
    fn the_layers_add_up_to_the_blur_in_the_middle_of_each_band() {
        for (sigma, strength) in [(2.0, 0.55_f32), (4.0, 0.8), (10.0, 0.45), (20.0, 0.8)] {
            let layers = layers(sigma, strength, Profile::Edge);
            for pair in layers.layers.windows(2) {
                let middle = f64::from(pair[0].grow) + f64::from(layers.step) / 2.0;
                if pair[1].grow - pair[0].grow != layers.step {
                    continue;
                }
                let wanted = f64::from(strength) * coverage(Profile::Edge, sigma, middle);
                // Layers under half a level were left out, so the sum may fall that short, and the opacities are
                // single-precision.
                assert!(
                    (stacked(&layers, middle) - wanted).abs() < 2.0 / 255.0,
                    "sigma {sigma}: {} at {middle}, wanted {wanted}",
                    stacked(&layers, middle)
                );
            }
        }
    }

    #[test]
    fn coarse_bands_are_a_quarter_of_a_deviation_and_still_add_up_to_the_blur() {
        // The panels' shadow at 100% (a deviation of 14) and at 175% (24.5), and the label's at 175% (6.1).
        for sigma in [14.0, 24.5, 6.1] {
            let coarse = layers_in_bands(sigma, 0.75, Profile::Edge, COARSE);
            let fine = layers(sigma, 0.75, Profile::Edge);
            assert_eq!(coarse.step, ceil_to_pixels((sigma / 4.0).max(1.0)));
            // Fewer layers, each stronger, so fewer of them round away in an 8-bit channel.
            assert!(coarse.layers.len() < fine.layers.len(), "sigma {sigma}");
            for pair in coarse.layers.windows(2) {
                if pair[1].grow - pair[0].grow != coarse.step {
                    continue;
                }
                let middle = f64::from(pair[0].grow) + f64::from(coarse.step) / 2.0;
                let wanted = 0.75 * coverage(Profile::Edge, sigma, middle);
                assert!(
                    (stacked(&coarse, middle) - wanted).abs() < 2.0 / 255.0,
                    "sigma {sigma}: {} at {middle}, wanted {wanted}",
                    stacked(&coarse, middle)
                );
            }
        }
    }

    #[test]
    fn a_shadow_is_strongest_inside_and_fades_to_nothing_outside() {
        let layers = layers(8.0, 0.35, Profile::Edge);
        // Deep inside, the full strength; at the outline, half of it; three deviations out, almost none.
        assert!((stacked(&layers, -23.0) - 0.35).abs() < 0.01);
        assert!((stacked(&layers, 0.0) - 0.175).abs() < 0.015);
        assert!(stacked(&layers, 24.0) < 1e-9);
        // Never stronger further out.
        let mut before = 1.0;
        for distance in -24..24 {
            let now = stacked(&layers, f64::from(distance) + 0.5);
            assert!(now <= before + 1e-9, "{distance}: {now} after {before}");
            before = now;
        }
    }

    #[test]
    fn the_layers_reach_three_deviations_out_and_every_layer_is_a_real_opacity() {
        let layers = layers(20.0, 0.8, Profile::Edge);
        // The outermost layers, under half a level, are left out; none is beyond three deviations.
        let reach = layers.reach();
        assert!((50..=60).contains(&reach), "{reach}");
        // About twenty bands to a deviation, and never finer than a pixel: twice the blur, twice the step.
        assert_eq!(layers.step, 1);
        assert_eq!(super::layers(40.0, 0.8, Profile::Edge).step, 2);
        for layer in &layers.layers {
            assert!(layer.opacity > 0.0 && layer.opacity <= 1.0, "{layer:?}");
        }
        // Their grow runs from inside to outside in whole steps.
        for pair in layers.layers.windows(2) {
            assert!(pair[1].grow > pair[0].grow);
        }
    }

    #[test]
    fn a_blur_under_a_pixel_is_a_handful_of_layers_a_pixel_apart() {
        // CSS `1px` of blur: sigma a half pixel.
        let layers = layers(0.5, 0.25, Profile::Edge);
        assert_eq!(layers.step, 1);
        assert!(layers.layers.len() <= 5, "{layers:?}");
        // The row just inside the outline gets most of the strength, the row just outside little.
        assert!((stacked(&layers, -0.5) - 0.25 * 0.841).abs() < 0.01);
        assert!((stacked(&layers, 0.5) - 0.25 * 0.159).abs() < 0.01);
    }

    #[test]
    fn no_blur_is_one_layer_at_the_strength() {
        let layers = layers(0.0, 0.12, Profile::Edge);
        assert_eq!(
            layers.layers,
            [Layer {
                grow: 0,
                opacity: 0.12
            }]
        );
        assert_eq!(layers.reach(), 0);
    }

    #[test]
    fn a_narrow_box_is_not_as_dark_in_the_middle_as_a_wide_one() {
        // A box 26 pixels across under a blur of deviation 20 never gets near full strength.
        let wide = layers(20.0, 0.8, Profile::Edge);
        let narrow = layers(20.0, 0.8, Profile::Strip(26.0));
        // The strip's core is its middle, 12 pixels in; the edge profile goes deeper.
        let strongest = |layers: &Layers| {
            layers
                .layers
                .iter()
                .map(|layer| f64::from(layer.opacity))
                .sum::<f64>()
        };
        assert!(narrow.layers[0].grow >= -12);
        assert!(narrow.layers.len() < wide.layers.len());
        let middle = stacked(&narrow, f64::from(narrow.layers[0].grow) - 1.0);
        // Two edges 26 pixels apart with a deviation of 20: 2 * cdf(13 / 20) - 1.
        let wanted = 0.8 * (2.0 * normal_cdf(13.0 / 20.0) - 1.0);
        assert!((middle - wanted).abs() < 0.03, "{middle} against {wanted}");
        assert!(strongest(&narrow) > 0.0);
        assert!(stacked(&wide, -59.0) > middle);
    }

    #[test]
    fn a_strip_matches_the_blur_across_its_short_side() {
        let (sigma, short) = (14.0, 42.0);
        let layers = layers(sigma, 0.75, Profile::Strip(short));
        for distance in [-15.5, -5.5, 0.5, 8.5, 20.5, 30.5] {
            let wanted =
                0.75 * (normal_cdf((short + distance) / sigma) - normal_cdf(distance / sigma));
            let got = stacked(&layers, distance);
            assert!(
                (got - wanted).abs() < 0.03,
                "{distance}: {got} against {wanted}"
            );
        }
    }

    #[test]
    fn a_very_thin_box_still_gets_layers_with_a_core() {
        let layers = layers(4.0, 0.5, Profile::Strip(1.0));
        assert!(!layers.layers.is_empty());
        assert!(layers.layers[0].grow >= 0);
    }
}
