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
