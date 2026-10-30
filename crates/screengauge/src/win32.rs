//! The numeric conversions Win32 forces on us, each checked once here.

use windows::Win32::Foundation::LPARAM;

/// A struct's size for a `cbSize` field. Use it as `const { struct_size::<T>() }`, so the assert runs at
/// compile time and the cast provably loses nothing.
pub const fn struct_size<T>() -> u32 {
    let size = size_of::<T>();
    assert!(size <= u32::MAX as usize);
    size as u32
}

/// LOWORD: keeping only the low 16 bits is the point, so the truncating cast is intended.
pub fn low_word(lparam: LPARAM) -> u32 {
    u32::from(lparam.0 as u16)
}

/// The low and high 16 bits of a packed pair, each signed, as GET_X_LPARAM, GET_Y_LPARAM and GET_WHEEL_DELTA_WPARAM
/// read them: a monitor left of or above the main one has negative coordinates. Keeping 16 bits of each is the point,
/// so the truncating casts are intended.
pub fn signed_words(packed: usize) -> (i16, i16) {
    (packed as u16 as i16, (packed >> 16) as u16 as i16)
}
