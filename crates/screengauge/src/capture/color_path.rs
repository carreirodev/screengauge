//! The color path of each monitor's DXGI photo: how its desktop image becomes a photo, chosen per monitor from whether
//! HDR is on (v1 spec WPC-296, Colors and HDR), and what becomes of a photo when HDR changed while it was taken. The
//! other path of the capture, GDI in DXGI's place, is another matter (`super::gdi`).

use screengauge_core::hdr::SdrWhiteLevel;
use screengauge_core::session::Colors;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
};
use windows::core::Error;

use super::Desktop;

/// How an output's desktop image becomes a photo: the rule is per monitor (v1 spec WPC-296, Colors and HDR).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ColorPath {
    /// HDR is off: the 8-bit image, with the colors the apps drew.
    Sdr,
    /// HDR is on and its SDR white level was read: the 16-bit image converted by it, the exact HDR path.
    Hdr(SdrWhiteLevel),
    /// HDR is on but its SDR white level couldn't be read, for this reason: the 8-bit image, with Windows' brightening
    /// in it, so its colors are approximate.
    Brightened(Error),
}

impl ColorPath {
    /// The color path of an output with HDR on or off. The 16-bit image is only for HDR on, because AMD's comes wrong
    /// with HDR off (WPC-289), so `white`, which reads the SDR white level, is asked only then.
    pub(super) fn of(
        hdr: bool,
        white: impl FnOnce() -> windows::core::Result<SdrWhiteLevel>,
    ) -> ColorPath {
        if !hdr {
            return ColorPath::Sdr;
        }
        match white() {
            Ok(white) => ColorPath::Hdr(white),
            Err(e) => ColorPath::Brightened(e),
        }
    }

    pub(super) fn hdr(&self) -> bool {
        !matches!(self, ColorPath::Sdr)
    }

    /// The formats the duplication asks DXGI for, which converts the desktop to one of them when it's in none.
    /// B8G8R8A8 is always among them, as Microsoft recommends, and its image is then approximate on a monitor with HDR
    /// on.
    pub(super) fn formats(&self) -> &'static [DXGI_FORMAT] {
        match self {
            ColorPath::Hdr(_) => &[DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_B8G8R8A8_UNORM],
            ColorPath::Sdr | ColorPath::Brightened(_) => &[DXGI_FORMAT_B8G8R8A8_UNORM],
        }
    }

    /// What becomes of a desktop image DXGI handed in `format`.
    pub(super) fn reading(&self, format: DXGI_FORMAT) -> windows::core::Result<Reading> {
        match (self, format) {
            (ColorPath::Sdr, DXGI_FORMAT_B8G8R8A8_UNORM) => Ok(Reading::Copy(Colors::Exact)),
            (ColorPath::Hdr(_) | ColorPath::Brightened(_), DXGI_FORMAT_B8G8R8A8_UNORM) => {
                Ok(Reading::Copy(Colors::Approximate))
            }
            (ColorPath::Hdr(white), DXGI_FORMAT_R16G16B16A16_FLOAT) => Ok(Reading::Convert(*white)),
            _ => Err(Error::new(
                E_FAIL,
                format!("the desktop image came in format {}", format.0),
            )),
        }
    }
}

/// How a desktop image is read into a photo's image.
#[derive(Debug, PartialEq)]
pub(super) enum Reading {
    /// 8 bits, copied as they are, with these colors.
    Copy(Colors),
    /// 16 bits, converted by this SDR white level: exact colors.
    Convert(SdrWhiteLevel),
}

/// A photo taken on a color path chosen with HDR on or off (`chosen`), once HDR is read again after it (`now`), and
/// whether HDR was on for the monitor during it. HDR turned on or off between choosing the color path and the end of
/// the photo, losing the duplication or not, leaves a color path for the old mode, which would mark a brightened photo
/// exact or take AMD's wrong 16-bit one (WPC-289): the monitor goes to GDI, marked as HDR on, since it was for part of
/// the photo.
pub(super) fn settled(
    desktop: windows::core::Result<Desktop>,
    chosen: bool,
    now: windows::core::Result<bool>,
) -> (windows::core::Result<Desktop>, bool) {
    match now {
        Ok(now) if now == chosen => (desktop, chosen),
        Ok(_) => (
            Err(Error::new(
                E_FAIL,
                "HDR was turned on or off during the photo",
            )),
            true,
        ),
        // Unread, the photo's marks can't be trusted; a photo that had already failed keeps its own cause.
        Err(e) => (desktop.and(Err(e)), chosen),
    }
}

#[cfg(test)]
mod tests {
    use screengauge_core::session::Image;
    use windows::Win32::Graphics::Dxgi::DXGI_ERROR_ACCESS_LOST;

    use super::*;

    /// An SDR white level of 240 nits.
    fn white_240() -> SdrWhiteLevel {
        SdrWhiteLevel::from_windows(3000).expect("a white level above zero")
    }

    #[test]
    fn a_monitor_with_hdr_off_takes_8_bits_without_reading_its_sdr_white_level() {
        // AMD's 16-bit image comes wrong with HDR off (WPC-289).
        let color_path = ColorPath::of(false, || panic!("the SDR white level was read"));
        assert_eq!(color_path, ColorPath::Sdr);
        assert_eq!(color_path.formats(), [DXGI_FORMAT_B8G8R8A8_UNORM]);
    }

    #[test]
    fn a_monitor_with_hdr_on_takes_16_bits_converted_by_its_sdr_white_level() {
        let color_path = ColorPath::of(true, || Ok(white_240()));
        assert_eq!(color_path, ColorPath::Hdr(white_240()));
        // B8G8R8A8 stays in the list, as Microsoft recommends, should DXGI have no 16-bit image to give.
        assert_eq!(
            color_path.formats(),
            [DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_B8G8R8A8_UNORM]
        );
    }

    #[test]
    fn a_monitor_with_hdr_on_whose_sdr_white_level_is_unknown_takes_8_bits() {
        let unread = Error::new(E_FAIL, "no white level");
        let color_path = ColorPath::of(true, || Err(unread.clone()));
        assert_eq!(color_path, ColorPath::Brightened(unread));
        assert_eq!(color_path.formats(), [DXGI_FORMAT_B8G8R8A8_UNORM]);
    }

    #[test]
    fn colors_are_exact_with_hdr_off_or_when_the_16_bit_image_was_converted() {
        // The exact path mark: a 16-bit image with the SDR white level read, or HDR off.
        let brightened = ColorPath::Brightened(Error::new(E_FAIL, "no white level"));
        let readings = [
            (ColorPath::Sdr, DXGI_FORMAT_B8G8R8A8_UNORM),
            (ColorPath::Hdr(white_240()), DXGI_FORMAT_R16G16B16A16_FLOAT),
            (ColorPath::Hdr(white_240()), DXGI_FORMAT_B8G8R8A8_UNORM),
            (brightened, DXGI_FORMAT_B8G8R8A8_UNORM),
        ]
        .map(|(color_path, format)| color_path.reading(format).ok());
        assert_eq!(
            readings,
            [
                Some(Reading::Copy(Colors::Exact)),
                Some(Reading::Convert(white_240())),
                Some(Reading::Copy(Colors::Approximate)),
                Some(Reading::Copy(Colors::Approximate)),
            ]
        );
    }

    #[test]
    fn a_16_bit_image_without_an_sdr_white_level_to_convert_by_is_refused() {
        // DXGI hands only formats it was asked for, so this is DXGI going wrong, and the monitor goes to GDI.
        let reading = ColorPath::Sdr.reading(DXGI_FORMAT_R16G16B16A16_FLOAT);
        assert!(reading.is_err(), "{reading:?}");
    }

    fn desktop(colors: Colors) -> windows::core::Result<Desktop> {
        Ok(Desktop {
            image: Image::new(4, 4, vec![0; 64]).expect("a whole image"),
            colors,
        })
    }

    #[test]
    fn hdr_turned_on_during_the_photo_sends_the_monitor_to_gdi_as_hdr_on() {
        // The color path was chosen with HDR off, so its 8-bit photo of a now brightened desktop says exact.
        let (image, hdr) = settled(desktop(Colors::Exact), false, Ok(true));
        assert!(image.is_err());
        assert!(hdr);
    }

    #[test]
    fn hdr_turned_off_during_the_photo_sends_the_monitor_to_gdi_as_hdr_on() {
        // HDR was on for part of the photo, and GDI's photo can't say which part.
        let (image, hdr) = settled(desktop(Colors::Exact), true, Ok(false));
        assert!(image.is_err());
        assert!(hdr);
    }

    #[test]
    fn hdr_as_the_color_path_was_chosen_keeps_the_photo() {
        for chosen in [false, true] {
            let (image, hdr) = settled(desktop(Colors::Exact), chosen, Ok(chosen));
            assert_eq!(
                image.map(|desktop| desktop.colors).ok(),
                Some(Colors::Exact)
            );
            assert_eq!(hdr, chosen);
        }
    }

    #[test]
    fn hdr_unread_after_the_photo_sends_the_monitor_to_gdi_as_the_color_path_was_chosen() {
        let unread = || Err(Error::new(E_FAIL, "GetDesc1 failed"));
        let (image, hdr) = settled(desktop(Colors::Exact), true, unread());
        assert_eq!(
            image.map(|_| ()).err().map(|e| e.message()),
            Some(String::from("GetDesc1 failed"))
        );
        assert!(hdr);
        // A photo that had already failed keeps its own cause.
        let lost = Err(Error::new(DXGI_ERROR_ACCESS_LOST, "lost"));
        let (image, hdr) = settled(lost, false, unread());
        assert_eq!(
            image.map(|_| ()).err().map(|e| e.code()),
            Some(DXGI_ERROR_ACCESS_LOST)
        );
        assert!(!hdr);
    }
}
