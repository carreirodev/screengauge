//! Layer 1 tests of the HDR conversion (v1 spec WPC-296, Cores e HDR; ticket WPC-327): a monitor's 16-bit desktop
//! image, as DXGI hands it with HDR on, divided by the monitor's SDR white level and encoded again as 8-bit sRGB by the
//! sRGB curve, with HDR content cut channel by channel to the sRGB range. The 16-bit values are made up here; whether
//! Windows hands them is the hardware's business (WPC-289 measured it).

use screengauge_core::hdr::{SdrWhiteLevel, convert};
use screengauge_core::session::Image;

/// The SDR white level as Windows gives it, for `nits`: 1000 is 80 nits.
fn white(nits: u32) -> SdrWhiteLevel {
    SdrWhiteLevel::from_windows(nits * 1000 / 80).expect("a white level above zero")
}

/// `value` as the nearest IEEE 754 half, ties to even, as the graphics card stores it. Only finite values a half holds.
fn half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = u16::try_from((bits >> 16) & 0x8000).expect("one bit");
    if value == 0.0 {
        return sign;
    }
    let exponent = i32::try_from((bits >> 23) & 0xff).expect("eight bits") - 127;
    // The 24 significant bits, the leading 1 included: the value is `significand * 2^(exponent - 23)`.
    let significand = (bits & 0x7f_ffff) | 0x80_0000;
    let magnitude = if exponent >= -14 {
        // A normal half keeps 11 significant bits; a carry out of them moves into the exponent, as it should.
        let biased = u32::try_from(exponent + 15).expect("a normal half's exponent");
        (biased << 10) + rounded_shift(significand, 13) - (1 << 10)
    } else {
        // A subnormal half counts units of 2^-24.
        rounded_shift(
            significand,
            u32::try_from(-(exponent + 1)).expect("a shift"),
        )
    };
    assert!(magnitude < 0x7c00, "{value} is too large for a half");
    sign | u16::try_from(magnitude).expect("fifteen bits")
}

/// `value >> shift`, rounded to the nearest, ties to even.
fn rounded_shift(value: u32, shift: u32) -> u32 {
    if shift >= 32 {
        return 0;
    }
    let kept = value >> shift;
    let rest = value - (kept << shift);
    let halfway = 1 << (shift - 1);
    if rest > halfway || (rest == halfway && kept & 1 == 1) {
        kept + 1
    } else {
        kept
    }
}

/// One row of pixels as DXGI lays out R16G16B16A16_FLOAT: red, green, blue and alpha halves, little-endian.
fn row(pixels: &[[f32; 3]]) -> Vec<u8> {
    pixels
        .iter()
        .flat_map(|&[red, green, blue]| [half(red), half(green), half(blue), half(1.0)])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// The red, green and blue of every pixel of `image`.
fn rgb(image: &Image) -> Vec<[u8; 3]> {
    image
        .pixels()
        .iter()
        .map(|&[blue, green, red, _]| [red, green, blue])
        .collect()
}

/// `pixels` as one row, converted at `nits`.
fn converted(pixels: &[[f32; 3]], nits: u32) -> Vec<[u8; 3]> {
    let rows = row(pixels);
    let width = u32::try_from(pixels.len()).expect("a short row");
    let image = convert(&rows, rows.len(), width, 1, white(nits)).expect("a whole row");
    rgb(&image)
}

#[test]
fn at_80_nits_sdr_white_is_white_and_half_the_light_is_188() {
    // 80 nits is scRGB's own 1.0, so nothing is divided. Half the light is 188 in sRGB, a known value of the curve.
    assert_eq!(
        converted(&[[1.0, 1.0, 1.0], [0.0, 0.0, 0.0], [0.5, 0.5, 0.5]], 80),
        [[255, 255, 255], [0, 0, 0], [188, 188, 188]]
    );
}

/// The SDR white levels WPC-289 measured on the owner's monitors: Windows' lowest, two in between and its highest.
const WHITE_LEVELS: [u32; 4] = [80, 240, 280, 480];

#[test]
fn every_sdr_white_level_is_divided_out() {
    for nits in WHITE_LEVELS {
        // SDR content's white and half its light, brightened as Windows shows them at `nits`.
        let brightening = f32::from(u16::try_from(nits).expect("a few hundred nits")) / 80.0;
        assert_eq!(
            converted(&[[brightening; 3], [0.5 * brightening; 3]], nits),
            [[255; 3], [188; 3]],
            "{nits} nits"
        );
    }
}

#[test]
fn dark_tones_follow_the_srgb_curve_not_a_2_2_gamma() {
    // A light of 0.002 is 7 on the sRGB curve's straight part near black; a 2.2 gamma would make it 15.
    assert_eq!(converted(&[[0.002; 3]], 80), [[7; 3]]);
    assert_eq!(converted(&[[0.006; 3]], 240), [[7; 3]]);
}

/// The light of sRGB byte `byte` as Windows composes SDR content at `nits`: decoded by the sRGB curve, then brightened
/// from 80 nits to the SDR white level (Microsoft Learn, "Use DirectX with Advanced Color").
fn brightened(byte: u8, nits: u32) -> f32 {
    // An f32 is far finer than the half `half` rounds this to.
    let encoded = f32::from(byte) / 255.0;
    let linear = if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    };
    linear * f32::from(u16::try_from(nits).expect("a few hundred nits")) / 80.0
}

#[test]
fn every_sdr_color_comes_back_as_the_app_drew_it() {
    for nits in WHITE_LEVELS {
        // Every byte on each channel, and dodger blue, the spec's example: #1E90FF.
        let mut pixels: Vec<[f32; 3]> = (0..=255)
            .map(|byte| {
                [
                    brightened(byte, nits),
                    brightened(0, nits),
                    brightened(255 - byte, nits),
                ]
            })
            .collect();
        pixels.push([
            brightened(0x1e, nits),
            brightened(0x90, nits),
            brightened(0xff, nits),
        ]);
        let mut expected: Vec<[u8; 3]> = (0..=255).map(|byte| [byte, 0, 255 - byte]).collect();
        expected.push([0x1e, 0x90, 0xff]);
        assert_eq!(converted(&pixels, nits), expected, "{nits} nits");
    }
}

#[test]
fn hdr_content_is_cut_channel_by_channel_to_the_srgb_range() {
    for nits in WHITE_LEVELS {
        let brightening = f32::from(u16::try_from(nits).expect("a few hundred nits")) / 80.0;
        // Twice as bright as SDR white in red, half SDR white's light in green, below black in blue (scRGB holds colors
        // outside sRGB's range as negative channels).
        assert_eq!(
            converted(
                &[[2.0 * brightening, 0.5 * brightening, -0.25], [60000.0; 3]],
                nits
            ),
            [[255, 188, 0], [255; 3]],
            "{nits} nits"
        );
    }
}

#[test]
fn a_channel_that_is_no_number_comes_out_black_and_an_infinite_one_white() {
    // Halves a graphics card shouldn't hand over, but can: NaN (0x7e00) and +infinity (0x7c00), in red, green, blue.
    let rows: Vec<u8> = [0x7e00, 0x7c00, 0xfc00, 0x3c00]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    let image = convert(&rows, rows.len(), 1, 1, white(80)).expect("one pixel");
    assert_eq!(rgb(&image), [[0, 255, 0]]);
}

#[test]
fn rows_lose_their_padding_and_every_pixel_becomes_opaque() {
    // Two rows of two pixels, with 8 bytes of padding after each, and an alpha of 0 that says nothing.
    let mut rows = Vec::new();
    for pixels in [
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [[0.0, 0.0, 1.0], [1.0, 1.0, 1.0]],
    ] {
        let mut line = row(&pixels);
        for alpha in [6, 7, 14, 15] {
            line[alpha] = 0;
        }
        rows.extend(line);
        rows.extend([0xee; 8]);
    }
    let image = convert(&rows, 24, 2, 2, white(80)).expect("two whole rows");
    assert_eq!(
        image.bgra(),
        [
            0, 0, 255, 255, 0, 255, 0, 255, //
            255, 0, 0, 255, 255, 255, 255, 255,
        ]
    );
}

#[test]
fn rows_missing_from_the_image_are_refused() {
    // Two rows promised, but the bytes stop in the middle of the second one.
    let mut rows = row(&[[1.0; 3]]);
    rows.extend(&row(&[[1.0; 3]])[..4]);
    assert_eq!(convert(&rows, 8, 1, 2, white(80)), None);
}

#[test]
fn a_white_level_of_zero_is_refused() {
    assert_eq!(SdrWhiteLevel::from_windows(0), None);
    assert!(SdrWhiteLevel::from_windows(1000).is_some());
}
