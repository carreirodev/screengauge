//! What the built ScreenGauge.exe must be (v1 spec WPC-296, Platform): x64, Per-Monitor V2 from its embedded manifest,
//! and importing only DLLs that come with Windows, so users install nothing but ScreenGauge.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{
    FindResourceW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW,
    LoadResource, LockResource, SizeofResource,
};
use windows::Win32::UI::WindowsAndMessaging::RT_MANIFEST;
use windows::core::{HSTRING, PCWSTR};

const EXE: &str = env!("CARGO_BIN_EXE_ScreenGauge");

/// Runtimes that a non-static build would import and that users would have to install.
const REDISTRIBUTABLE_PREFIXES: [&str; 6] = [
    "vcruntime",
    "msvcp",
    "ucrtbase",
    "api-ms-win-crt-",
    "concrt",
    "vccorlib",
];

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

fn read_exe() -> Vec<u8> {
    std::fs::read(EXE).unwrap_or_else(|e| panic!("reading {EXE}: {e}"))
}

/// Reads the RT_MANIFEST resource with ID 1, the one Windows applies when it starts the process.
fn embedded_manifest() -> String {
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
    // SAFETY: MAKEINTRESOURCE(1), the process manifest's ID, in a module that stays loaded until FreeLibrary.
    let manifest = unsafe {
        let resource = FindResourceW(
            Some(module),
            PCWSTR(std::ptr::without_provenance(1)),
            RT_MANIFEST,
        );
        assert!(!resource.is_invalid(), "no RT_MANIFEST resource with ID 1");
        let size = widen(SizeofResource(Some(module), resource));
        let data = LockResource(LoadResource(Some(module), resource).expect("LoadResource"));
        assert!(!data.is_null(), "LockResource returned null");
        String::from_utf8_lossy(std::slice::from_raw_parts(data.cast::<u8>(), size)).into_owned()
    };
    // SAFETY: the module came from LoadLibraryExW above, and the manifest was copied out of it.
    unsafe { FreeLibrary(module) }.expect("FreeLibrary");
    manifest
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
