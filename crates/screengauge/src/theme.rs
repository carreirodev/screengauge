//! Light or dark: which of its two palettes ScreenGauge draws its own surfaces in: the toolbar, the notices, the pinned
//! measurements' labels and Settings, its title bar included (v1 spec WPC-296, Theme). The measurement's line and the
//! live label are cyan in both. The user chooses Automatic, Light or Dark in Settings; Automatic follows Windows' app
//! mode, and changes with it while ScreenGauge runs (WPC-321).

use screengauge_core::settings::Theme;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::{Error, w};

use crate::log;

/// The palette ScreenGauge's surfaces are drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

/// The palette for `theme`: Light and Dark as chosen, and Automatic as Windows' own "default app mode" (Settings >
/// Personalization > Colors) is right now.
pub fn mode(theme: Theme) -> Mode {
    match theme {
        Theme::Light => Mode::Light,
        Theme::Dark => Mode::Dark,
        Theme::Automatic => match apps_use_light_theme() {
            Ok(true) => Mode::Light,
            Ok(false) => Mode::Dark,
            // The palette only colors what ScreenGauge draws; the tools still open, in Windows' own default.
            Err(e) => {
                log::failure("theme: reading Windows' app mode", &e);
                Mode::Light
            }
        },
    }
}

/// Whether Windows draws apps in its light mode. A user who never changed it has no value, and Windows is then light.
fn apps_use_light_theme() -> windows::core::Result<bool> {
    let mut value = 0u32;
    let mut size = const { crate::win32::struct_size::<u32>() };
    // SAFETY: `value` and `size` outlive the call, which writes at most `size` bytes into `value`; the names are static
    // strings.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    match read {
        ERROR_SUCCESS => Ok(value != 0),
        ERROR_FILE_NOT_FOUND => Ok(true),
        error => Err(Error::from(error.to_hresult())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_are_as_chosen_whatever_windows_says() {
        assert_eq!(mode(Theme::Light), Mode::Light);
        assert_eq!(mode(Theme::Dark), Mode::Dark);
    }

    #[test]
    fn this_computer_says_which_mode_its_apps_are_in() {
        // Whichever it is, the value must be readable here, or Automatic would fall back on every call.
        apps_use_light_theme().expect("Windows' app mode is readable");
    }
}
