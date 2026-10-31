//! What the built ScreenGauge.exe must be (v1 spec WPC-296, Platform): x64, Per-Monitor V2 from its embedded manifest,
//! and importing only DLLs that come with Windows, so users install nothing but ScreenGauge. The release build must
//! also fit the 5 MB download limit. It must carry the app icon from `assets/ScreenGauge.ico`, sharp at every size the
//! tray asks for, and a version resource that names it ScreenGauge (WPC-304).

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_RESOURCE_TYPE_NOT_FOUND, FreeLibrary, HMODULE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows::Win32::System::LibraryLoader::{
    EnumResourceNamesW, FindResourceW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
    LoadLibraryExW, LoadResource, LockResource, SizeofResource,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetIconInfo, HICON, ICONINFO, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
    RT_GROUP_ICON, RT_ICON, RT_MANIFEST,
};
use windows::core::{BOOL, Error, HRESULT, HSTRING, PCWSTR};

const EXE: &str = env!("CARGO_BIN_EXE_ScreenGauge");

/// The icon file the build embeds, relative to this crate (see build.rs).
const ICO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/ScreenGauge.ico");

/// The sizes the notification area draws at 100%, 125%, 150%, 175% and 200% scale (SM_CXSMICON at 96 to 192 DPI).
const TRAY_SIZES: [i32; 5] = [16, 20, 24, 28, 32];

/// The resource ID of the app icon group, and so the one `LoadImageW` is asked for.
const APP_ICON_ID: u16 = 1;

/// Runtimes that a non-static build would import and that users would have to install.
const REDISTRIBUTABLE_PREFIXES: [&str; 6] = [
    "vcruntime",
    "msvcp",
    "ucrtbase",
    "api-ms-win-crt-",
    "concrt",
    "vccorlib",
];

/// Each download is at most 5 MB (v1 spec WPC-296), read as decimal megabytes, the stricter reading.
const SIZE_LIMIT_BYTES: u64 = 5_000_000;

#[test]
fn exe_is_x64() {
    const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
    let image = read_exe();
    assert_eq!(
        u16_at(&image, pe_header(&image) + 4),
        IMAGE_FILE_MACHINE_AMD64
    );
}

#[test]
fn manifest_declares_per_monitor_v2() {
    let manifest = embedded_manifest();
    let awareness = manifest
        .split_once("<dpiAwareness")
        .and_then(|(_, rest)| rest.split_once('>'))
        .and_then(|(_, rest)| rest.split_once("</dpiAwareness>"))
        .map(|(value, _)| value.trim())
        .unwrap_or_else(|| panic!("no dpiAwareness element in the manifest:\n{manifest}"));
    // The first value Windows understands wins; Windows 11 understands PerMonitorV2.
    assert_eq!(
        awareness.split(',').next().map(str::trim),
        Some("PerMonitorV2"),
        "{manifest}"
    );
}

#[test]
fn imports_only_windows_dlls() {
    let system32 =
        PathBuf::from(std::env::var("SystemRoot").expect("SystemRoot is set on Windows"))
            .join("System32");
    let dlls = imported_dlls(&read_exe());
    assert!(!dlls.is_empty(), "no imports found; the parser is broken");
    for dll in &dlls {
        let name = dll.to_ascii_lowercase();
        assert!(
            !REDISTRIBUTABLE_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix)),
            "{dll} is a redistributable runtime; is crt-static off? imports: {dlls:?}"
        );
        let api_set = name.starts_with("api-ms-win-") || name.starts_with("ext-ms-win-");
        assert!(
            api_set || system32.join(dll).is_file(),
            "{dll} doesn't come with Windows; imports: {dlls:?}"
        );
    }
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "the limit applies to the release build; run cargo test --release"
)]
fn release_exe_fits_the_download_limit() {
    let size = std::fs::metadata(EXE)
        .unwrap_or_else(|e| panic!("reading {EXE}: {e}"))
        .len();
    assert!(
        size <= SIZE_LIMIT_BYTES,
        "{EXE} is {size} bytes, over the {SIZE_LIMIT_BYTES}-byte limit"
    );
}

/// The app icon is the exe's only icon group, and the icon images in the exe are the .ico file's, byte for byte:
/// nothing was converted or resized on the way in.
#[test]
fn exe_embeds_the_ico_file_as_its_only_icon_group() {
    let frames = read_ico();
    with_exe_module(|module| {
        assert_eq!(
            icon_group_names(module),
            [ResourceName::Id(APP_ICON_ID)],
            "the exe must have exactly one icon group, with ID {APP_ICON_ID}"
        );
        let entries = group_entries(&resource_bytes(module, RT_GROUP_ICON, APP_ICON_ID));
        let in_exe: Vec<_> = entries
            .iter()
            .map(|e| (e.width, e.height, e.bit_count, e.size))
            .collect();
        let in_file: Vec<_> = frames
            .iter()
            .map(|f| (f.width, f.height, f.bit_count, f.bytes.len()))
            .collect();
        assert_eq!(
            in_exe, in_file,
            "the icon group differs from {ICO}: (width, height, bits, bytes) per image"
        );
        for (entry, frame) in entries.iter().zip(&frames) {
            assert!(
                resource_bytes(module, RT_ICON, entry.id) == frame.bytes,
                "RT_ICON {} differs from the {}-pixel image in {ICO}",
                entry.id,
                frame.width
            );
        }
    });
}

/// The notification area draws an icon without scaling when the image of its size is there, so every tray size must
/// load as the .ico's own image of that size, pixel for pixel. This is what keeps the tray icon sharp.
#[test]
fn tray_sizes_load_the_icos_own_image_pixel_for_pixel() {
    let frames = read_ico();
    with_exe_module(|module| {
        for size in TRAY_SIZES {
            let frame = frames
                .iter()
                .find(|f| i32::from(f.width) == size && i32::from(f.height) == size)
                .unwrap_or_else(|| panic!("{ICO} has no {size}-pixel image"));
            let icon = load_icon(module, size);
            let (width, height, drawn) = into_color_bitmap(icon);
            assert_eq!((width, height), (size, size), "size of the {size} icon");
            assert_same_pixels(size, &drawn, &top_down_pixels(frame, size));
        }
    });
}

#[test]
fn version_resource_names_the_app() {
    let version = VersionInfo::of_exe();
    let expected = [
        ("CompanyName", "WPC Solutions"),
        ("FileDescription", "ScreenGauge"),
        ("FileVersion", env!("CARGO_PKG_VERSION")),
        ("InternalName", "ScreenGauge"),
        ("OriginalFilename", "ScreenGauge.exe"),
        ("ProductName", "ScreenGauge"),
        ("ProductVersion", env!("CARGO_PKG_VERSION")),
    ];
    for (name, value) in expected {
        assert_eq!(version.string(name), value, "{name}");
    }
}

#[test]
fn version_resource_has_the_packages_numeric_version() {
    let part = |value: &str| {
        value
            .parse::<u32>()
            .expect("a number in the package version")
    };
    let (major, minor, patch) = (
        part(env!("CARGO_PKG_VERSION_MAJOR")),
        part(env!("CARGO_PKG_VERSION_MINOR")),
        part(env!("CARGO_PKG_VERSION_PATCH")),
    );
    let fixed = VersionInfo::of_exe().fixed_info();
    assert_eq!(fixed.dwSignature, 0xFEEF_04BD, "VS_FIXEDFILEINFO signature");
    for (name, ms, ls) in [
        ("file", fixed.dwFileVersionMS, fixed.dwFileVersionLS),
        (
            "product",
            fixed.dwProductVersionMS,
            fixed.dwProductVersionLS,
        ),
    ] {
        assert_eq!(
            (ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF),
            (major, minor, patch, 0),
            "{name} version as major.minor.patch.0"
        );
    }
    assert_eq!(fixed.dwFileType, 1, "VFT_APP");
    assert_eq!(fixed.dwFileOS.0, 0x0004_0004, "VOS_NT_WINDOWS32");
    assert_eq!(fixed.dwFileFlags.0 & fixed.dwFileFlagsMask, 0, "no flags");
}

#[test]
fn version_resource_is_in_english_unicode() {
    // 0x0409 is US English, 1200 is Unicode: the `040904B0` of the string table's name.
    assert_eq!(VersionInfo::of_exe().translations(), [(0x0409, 1200)]);
}

fn read_exe() -> Vec<u8> {
    std::fs::read(EXE).unwrap_or_else(|e| panic!("reading {EXE}: {e}"))
}

/// Reads the RT_MANIFEST resource with ID 1, the one Windows applies when it starts the process.
fn embedded_manifest() -> String {
    with_exe_module(|module| {
        String::from_utf8_lossy(&resource_bytes(module, RT_MANIFEST, 1)).into_owned()
    })
}

/// Loads the exe as data only, so none of its code runs, gives the module to `read`, and frees it.
fn with_exe_module<T>(read: impl FnOnce(HMODULE) -> T) -> T {
    let path = HSTRING::from(Path::new(EXE).as_os_str());
    // SAFETY: loads the exe as data only, so none of its code runs; freed below.
    let module = unsafe {
        LoadLibraryExW(
            &path,
            None,
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    }
    .expect("LoadLibraryExW");
    let result = read(module);
    // SAFETY: the module came from LoadLibraryExW above, and `read` returns only what it copied out of it.
    unsafe { FreeLibrary(module) }.expect("FreeLibrary");
    result
}

/// A copy of the resource of type `kind` with the numeric ID `id`.
fn resource_bytes(module: HMODULE, kind: PCWSTR, id: u16) -> Vec<u8> {
    // SAFETY: MAKEINTRESOURCE(id) in a module that stays loaded while this runs.
    unsafe {
        let resource = FindResourceW(
            Some(module),
            PCWSTR(std::ptr::without_provenance(usize::from(id))),
            kind,
        );
        assert!(
            !resource.is_invalid(),
            "no resource of type {kind:?} with ID {id}"
        );
        let size = widen(SizeofResource(Some(module), resource));
        let data = LockResource(LoadResource(Some(module), resource).expect("LoadResource"));
        assert!(!data.is_null(), "LockResource returned null");
        std::slice::from_raw_parts(data.cast::<u8>(), size).to_vec()
    }
}

/// How a resource is named: by number, the way every resource ScreenGauge embeds is, or by text.
#[derive(Debug, PartialEq)]
enum ResourceName {
    Id(u16),
    Text(String),
}

/// The names of all the exe's icon groups.
fn icon_group_names(module: HMODULE) -> Vec<ResourceName> {
    let mut names: Vec<ResourceName> = Vec::new();
    // SAFETY: the callback only reads `names`, which outlives this call, through the pointer passed as lparam.
    let found = unsafe {
        EnumResourceNamesW(
            Some(module),
            RT_GROUP_ICON,
            Some(collect_resource_name),
            std::ptr::from_mut(&mut names)
                .expose_provenance()
                .cast_signed(),
        )
    };
    if !found.as_bool() {
        // With no resource of the type at all, the call fails instead of listing nothing.
        let error = Error::from_thread();
        assert_eq!(
            error.code(),
            HRESULT::from_win32(ERROR_RESOURCE_TYPE_NOT_FOUND.0),
            "EnumResourceNamesW: {error}"
        );
    }
    names
}

unsafe extern "system" fn collect_resource_name(
    _module: HMODULE,
    _kind: PCWSTR,
    name: PCWSTR,
    names: isize,
) -> BOOL {
    // SAFETY: `names` is the pointer `icon_group_names` passed, to a Vec that outlives the enumeration.
    let names = unsafe {
        &mut *std::ptr::with_exposed_provenance_mut::<Vec<ResourceName>>(names.cast_unsigned())
    };
    // IS_INTRESOURCE: Windows passes a number as a pointer value that fits in 16 bits.
    names.push(match u16::try_from(name.0.addr()) {
        Ok(id) => ResourceName::Id(id),
        // SAFETY: any other name is a null-terminated string that lives as long as the module.
        Err(_) => ResourceName::Text(
            unsafe { name.to_string() }
                .unwrap_or_else(|e| format!("<a name that isn't UTF-16: {e}>")),
        ),
    });
    true.into()
}

/// One image of the .ico file: its directory entry, and its bytes as the file stores them.
struct IcoFrame {
    /// 0 stands for 256, as in the file.
    width: u8,
    height: u8,
    bit_count: u16,
    bytes: Vec<u8>,
}

/// The images of `assets/ScreenGauge.ico`, in the file's order.
fn read_ico() -> Vec<IcoFrame> {
    let file = std::fs::read(ICO).unwrap_or_else(|e| panic!("reading {ICO}: {e}"));
    assert_eq!(u16_at(&file, 0), 0, "ICONDIR reserved");
    assert_eq!(u16_at(&file, 2), 1, "ICONDIR type: 1 is an icon");
    (0..usize::from(u16_at(&file, 4)))
        .map(|i| {
            // ICONDIRENTRY, 16 bytes: width, height, colors, reserved, planes, bits, size, offset.
            let entry = 6 + i * 16;
            let start = widen(u32_at(&file, entry + 12));
            let size = widen(u32_at(&file, entry + 8));
            IcoFrame {
                width: file[entry],
                height: file[entry + 1],
                bit_count: u16_at(&file, entry + 6),
                bytes: file[start..start + size].to_vec(),
            }
        })
        .collect()
}

/// One entry of an RT_GROUP_ICON resource, which points at an RT_ICON image by ID.
struct GroupEntry {
    width: u8,
    height: u8,
    bit_count: u16,
    size: usize,
    id: u16,
}

/// The entries of an RT_GROUP_ICON resource: GRPICONDIR, then 14-byte GRPICONDIRENTRYs.
fn group_entries(group: &[u8]) -> Vec<GroupEntry> {
    assert_eq!(u16_at(group, 0), 0, "GRPICONDIR reserved");
    assert_eq!(u16_at(group, 2), 1, "GRPICONDIR type: 1 is an icon");
    (0..usize::from(u16_at(group, 4)))
        .map(|i| {
            // Width, height, colors, reserved, planes, bits, size, ID.
            let entry = 6 + i * 14;
            GroupEntry {
                width: group[entry],
                height: group[entry + 1],
                bit_count: u16_at(group, entry + 6),
                size: widen(u32_at(group, entry + 8)),
                id: u16_at(group, entry + 12),
            }
        })
        .collect()
}

/// The pixels of a 32-bit BMP image of the .ico, as top-down rows of BGRA: the file stores the rows bottom-up.
fn top_down_pixels(frame: &IcoFrame, size: i32) -> Vec<u8> {
    let side = usize::try_from(size).expect("a positive size");
    let bytes = &frame.bytes;
    let header = widen(u32_at(bytes, 0));
    assert_eq!(
        header, 40,
        "BITMAPINFOHEADER size of the {size}-pixel image"
    );
    assert_eq!(u32_at(bytes, 4), u32::try_from(size).expect("positive"));
    // The height of an icon image counts its AND mask too.
    assert_eq!(u32_at(bytes, 8), u32::try_from(2 * size).expect("positive"));
    assert_eq!(u16_at(bytes, 14), 32, "bits per pixel");
    assert_eq!(u32_at(bytes, 16), BI_RGB.0, "compression");
    let row = side * 4;
    bytes[header..header + row * side]
        .chunks_exact(row)
        .rev()
        .flatten()
        .copied()
        .collect()
}

/// An icon loaded from the module the way the tray loads its own: of that size, at the screen's color depth.
fn load_icon(module: HMODULE, size: i32) -> HICON {
    // SAFETY: MAKEINTRESOURCE of the app icon group in a module that stays loaded while this runs.
    let handle = unsafe {
        LoadImageW(
            Some(module.into()),
            PCWSTR(std::ptr::without_provenance(usize::from(APP_ICON_ID))),
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    }
    .expect("LoadImageW");
    HICON(handle.0)
}

/// The width, height and top-down BGRA pixels of the icon's color bitmap. Destroys the icon.
fn into_color_bitmap(icon: HICON) -> (i32, i32, Vec<u8>) {
    let mut info = ICONINFO::default();
    // SAFETY: a valid icon, and an ICONINFO to fill. The two bitmaps it creates are deleted below.
    unsafe { GetIconInfo(icon, &mut info) }.expect("GetIconInfo");
    let mut bitmap = BITMAP::default();
    // SAFETY: a valid bitmap, and a BITMAP of the size passed.
    let written = unsafe {
        GetObjectW(
            info.hbmColor.into(),
            i32::try_from(size_of::<BITMAP>()).expect("fits"),
            Some(std::ptr::from_mut(&mut bitmap).cast::<c_void>()),
        )
    };
    assert_ne!(written, 0, "GetObjectW");
    let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);

    let mut format = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).expect("fits"),
            biWidth: width,
            // Negative: the rows come top-down.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0_u8; usize::try_from(width * height * 4).expect("positive")];
    // SAFETY: a memory DC for GetDIBits; the bitmap isn't selected into any DC, and the buffer holds
    // `height` rows of `width` 32-bit pixels, as `format` says.
    let rows = unsafe {
        let dc = CreateCompatibleDC(None);
        let rows = GetDIBits(
            dc,
            info.hbmColor,
            0,
            height.cast_unsigned(),
            Some(pixels.as_mut_ptr().cast::<c_void>()),
            &mut format,
            DIB_RGB_COLORS,
        );
        assert!(DeleteDC(dc).as_bool(), "DeleteDC");
        rows
    };
    assert_eq!(rows, height, "GetDIBits copied every row");
    // SAFETY: the bitmaps GetIconInfo created, and the icon the caller handed over; none is used again.
    unsafe {
        assert!(DeleteObject(info.hbmColor.into()).as_bool(), "DeleteObject");
        assert!(DeleteObject(info.hbmMask.into()).as_bool(), "DeleteObject");
        DestroyIcon(icon).expect("DestroyIcon");
    }
    (width, height, pixels)
}

/// Asserts that two top-down BGRA images are the same, naming the first pixel that differs.
fn assert_same_pixels(size: i32, drawn: &[u8], expected: &[u8]) {
    assert_eq!(drawn.len(), expected.len(), "bytes in the {size} icon");
    if let Some(at) = drawn.iter().zip(expected).position(|(a, b)| a != b) {
        let pixel = at / 4;
        let side = usize::try_from(size).expect("positive");
        panic!(
            "the {size} icon differs from the .ico at x={} y={}: drawn BGRA {:?}, in the file {:?}",
            pixel % side,
            pixel / side,
            &drawn[pixel * 4..pixel * 4 + 4],
            &expected[pixel * 4..pixel * 4 + 4],
        );
    }
}

/// The version resource as Windows reads it from the file.
struct VersionInfo {
    /// Four-byte units, so the pointers VerQueryValueW returns into it are as aligned as Windows lays them out.
    block: Vec<u32>,
}

impl VersionInfo {
    fn of_exe() -> Self {
        let path = HSTRING::from(Path::new(EXE).as_os_str());
        // SAFETY: a valid null-terminated path.
        let size = unsafe { GetFileVersionInfoSizeW(&path, None) };
        assert_ne!(size, 0, "the exe has no version resource");
        let mut block = vec![0_u32; widen(size).div_ceil(4)];
        // SAFETY: the buffer is at least `size` bytes, as GetFileVersionInfoSizeW asked.
        unsafe { GetFileVersionInfoW(&path, None, size, block.as_mut_ptr().cast::<c_void>()) }
            .expect("GetFileVersionInfoW");
        Self { block }
    }

    /// What `VerQueryValueW` finds at `sub_block`, with its length in the value's own unit (characters for a string).
    fn query(&self, sub_block: &str) -> (*const c_void, usize) {
        let sub_block = HSTRING::from(sub_block);
        let mut value = std::ptr::null_mut::<c_void>();
        let mut length = 0;
        // SAFETY: the block came from GetFileVersionInfoW, and the two out-pointers are valid locals.
        let found = unsafe {
            VerQueryValueW(
                self.block.as_ptr().cast::<c_void>(),
                &sub_block,
                &mut value,
                &mut length,
            )
        };
        assert!(found.as_bool(), "no {sub_block} in the version resource");
        (value.cast_const(), widen(length))
    }

    /// A value of the English, Unicode string table, which is the only one.
    fn string(&self, name: &str) -> String {
        let (value, length) = self.query(&format!("\\StringFileInfo\\040904B0\\{name}"));
        // SAFETY: Windows returned `length` UTF-16 units inside the block, which `self` keeps alive.
        let units = unsafe { std::slice::from_raw_parts(value.cast::<u16>(), length) };
        String::from_utf16(units)
            .unwrap_or_else(|e| panic!("{name} isn't UTF-16: {e}"))
            .trim_end_matches('\0')
            .to_owned()
    }

    fn fixed_info(&self) -> VS_FIXEDFILEINFO {
        let (value, _) = self.query("\\");
        // SAFETY: Windows returned a VS_FIXEDFILEINFO inside the block, which `self` keeps alive.
        unsafe { value.cast::<VS_FIXEDFILEINFO>().read_unaligned() }
    }

    /// The (language, code page) pairs the file declares.
    fn translations(&self) -> Vec<(u16, u16)> {
        let (value, bytes) = self.query("\\VarFileInfo\\Translation");
        (0..bytes / 4)
            // SAFETY: Windows returned `bytes` bytes of language and code page pairs inside the block.
            .map(|i| unsafe {
                let pair = value.cast::<u16>().add(i * 2);
                (pair.read_unaligned(), pair.add(1).read_unaligned())
            })
            .collect()
    }
}

/// The DLL names in the PE import and delay-import tables, read from the file bytes.
fn imported_dlls(image: &[u8]) -> Vec<String> {
    const PE32_PLUS: u16 = 0x20B;
    let coff = pe_header(image) + 4;
    let section_count = usize::from(u16_at(image, coff + 2));
    let optional = coff + 20;
    assert_eq!(u16_at(image, optional), PE32_PLUS, "not a 64-bit image");
    let data_directories = optional + 112;
    let sections = optional + usize::from(u16_at(image, coff + 16));

    let file_offset = |rva: u32| -> usize {
        (0..section_count)
            .map(|i| sections + i * 40)
            .find_map(|section| {
                let start = u32_at(image, section + 12);
                let size = u32_at(image, section + 8).max(u32_at(image, section + 16));
                (start..start + size)
                    .contains(&rva)
                    .then(|| widen(rva - start + u32_at(image, section + 20)))
            })
            .unwrap_or_else(|| panic!("RVA {rva:#x} is in no section"))
    };
    let name_at = |rva: u32| -> String {
        let start = file_offset(rva);
        let end = start
            + image[start..]
                .iter()
                .position(|&b| b == 0)
                .expect("null-terminated DLL name");
        String::from_utf8_lossy(&image[start..end]).into_owned()
    };

    let mut dlls = Vec::new();
    // Data directory 1: imports, 20-byte descriptors with the name RVA at +12. Directory 13: delay
    // imports, 32-byte descriptors with the name RVA at +4. Both lists end with a zeroed descriptor.
    for (directory, descriptor_size, name_field) in [(1, 20, 12), (13, 32, 4)] {
        let table = u32_at(image, data_directories + directory * 8);
        if table == 0 {
            continue;
        }
        let mut descriptor = file_offset(table);
        loop {
            let name = u32_at(image, descriptor + name_field);
            if name == 0 {
                break;
            }
            dlls.push(name_at(name));
            descriptor += descriptor_size;
        }
    }
    dlls
}

/// The offset of the "PE\0\0" signature, which the DOS header points to at 0x3C.
fn pe_header(image: &[u8]) -> usize {
    let offset = widen(u32_at(image, 0x3C));
    assert_eq!(&image[offset..offset + 4], b"PE\0\0", "not a PE image");
    offset
}

/// PE fields are u32; a usize holds every u32 on x64.
fn widen(value: u32) -> usize {
    usize::try_from(value).expect("a u32 fits in a usize on x64")
}

fn u16_at(image: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([image[offset], image[offset + 1]])
}

fn u32_at(image: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(image[offset..offset + 4].try_into().expect("4 bytes"))
}
