//! Embeds into ScreenGauge.exe what is not code (WPC-304):
//!
//! - the manifest, which makes the process Per-Monitor V2 from the start;
//! - the app icon, from `assets/ScreenGauge.ico`;
//! - a version resource, which is what Windows reads to name the program, for one, in the header of its notifications.
//!
//! The icon and the version resource go in a `.res` file this script writes itself, because the owner asked for no
//! resource compiler (rc.exe, embed-resource, winresource) in the build. link.exe takes a `.res` as an input file and
//! converts it on its own. The manifest stays out of the `.res`: the linker embeds it, and a second RT_MANIFEST would
//! collide with it.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// US English, the language of every resource written here.
const LANGUAGE: u16 = 0x0409;
/// Unicode, the code page of the version resource's strings.
const CODE_PAGE: u16 = 1200;

// Resource types, from winuser.h.
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;

// Memory flags, from winbase.h. rc.exe gives icons MOVEABLE | DISCARDABLE (0x1010), icon groups that and PURE (0x1030),
// and a version resource MOVEABLE | PURE (0x0030). Windows 11 ignores them; they are kept as rc.exe writes them.
const MOVEABLE: u16 = 0x0010;
const PURE: u16 = 0x0020;
const DISCARDABLE: u16 = 0x1000;

/// The ID of the icon group, the one the tray asks for. It is the exe's only group, and so the icon Explorer shows.
const APP_ICON_GROUP_ID: u16 = 1;

fn main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = manifest_dir.join("ScreenGauge.exe.manifest");
    let icon = manifest_dir.join("../../assets/ScreenGauge.ico");

    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );

    let resources = icon_resources(&read_file(&icon), &icon.display().to_string())
        .into_iter()
        .chain([version_resource()])
        .collect::<Vec<_>>();
    let res = out_dir().join("ScreenGauge.res");
    fs::write(&res, res_file(&resources))
        .unwrap_or_else(|e| panic!("writing {}: {e}", res.display()));
    println!("cargo:rustc-link-arg-bins={}", res.display());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=ScreenGauge.exe.manifest");
    println!("cargo:rerun-if-changed={}", icon.display());
}

fn out_dir() -> PathBuf {
    PathBuf::from(var("OUT_DIR"))
}

fn var(name: &str) -> String {
    env::var(name).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn read_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------------------------------------------------
// The .res file
// ---------------------------------------------------------------------------------------------------------------------

/// One resource: a numeric type and ID, the memory flags, and the bytes.
struct Resource {
    kind: u16,
    id: u16,
    flags: u16,
    data: Vec<u8>,
}

/// A 32-bit `.res` file: a chain of entries, each a header and the data, all of it aligned to 4 bytes. The file starts
/// with an empty entry, which is how a 32-bit `.res` tells itself from a 16-bit one.
fn res_file(resources: &[Resource]) -> Vec<u8> {
    let mut file = Vec::new();
    push_entry(&mut file, 0, 0, 0, 0, &[]);
    for resource in resources {
        push_entry(
            &mut file,
            resource.kind,
            resource.id,
            resource.flags,
            LANGUAGE,
            &resource.data,
        );
    }
    file
}

/// One entry: DataSize, HeaderSize, TYPE, NAME, DataVersion, MemoryFlags, LanguageId, Version, Characteristics, and
/// then the data padded to 4 bytes. TYPE and NAME are a 0xFFFF marker and a number, the form of a numeric ID, which
/// makes the header 32 bytes. Every entry starts aligned, so the header needs no padding of its own.
fn push_entry(file: &mut Vec<u8>, kind: u16, id: u16, flags: u16, language: u16, data: &[u8]) {
    push_u32(file, checked_u32(data.len(), "a resource"));
    push_u32(file, 32);
    for word in [0xFFFF, kind, 0xFFFF, id] {
        push_u16(file, word);
    }
    push_u32(file, 0);
    push_u16(file, flags);
    push_u16(file, language);
    push_u32(file, 0);
    push_u32(file, 0);
    file.extend_from_slice(data);
    pad_to_dword(file);
}

// ---------------------------------------------------------------------------------------------------------------------
// The icon: one RT_ICON per image of the .ico file, and the RT_GROUP_ICON that lists them
// ---------------------------------------------------------------------------------------------------------------------

/// The .ico file holds a directory and, after it, the images. An exe holds the same images one by one as RT_ICON
/// resources, and a directory of its own, the RT_GROUP_ICON, that points at them by ID instead of by file offset.
/// The images are copied as they are, so Windows draws the file's own pixels. `path` is for the error messages.
fn icon_resources(ico: &[u8], path: &str) -> Vec<Resource> {
    const ICONDIR_SIZE: usize = 6;
    const ICONDIRENTRY_SIZE: usize = 16;

    if ico.len() < ICONDIR_SIZE {
        invalid_ico(path, "it is shorter than its header");
    }
    if u16_at(ico, 0) != 0 {
        invalid_ico(path, "the reserved field isn't 0");
    }
    if u16_at(ico, 2) != 1 {
        invalid_ico(path, "its type isn't 1 (icon)");
    }
    let count = usize::from(u16_at(ico, 4));
    if count == 0 {
        invalid_ico(path, "it has no images");
    }

    // GRPICONDIR: reserved, type, count. Then one 14-byte GRPICONDIRENTRY per image.
    let mut group = Vec::new();
    push_u16(&mut group, 0);
    push_u16(&mut group, 1);
    push_u16(&mut group, u16_at(ico, 4));
    let mut resources = Vec::new();
    for index in 0..count {
        let entry_start = ICONDIR_SIZE + index * ICONDIRENTRY_SIZE;
        let Some(entry) = ico.get(entry_start..entry_start + ICONDIRENTRY_SIZE) else {
            invalid_ico(
                path,
                &format!("the directory ends inside entry {}", index + 1),
            )
        };
        // ICONDIRENTRY: width, height, colors, reserved, planes, bits (together the first 8 bytes), size, offset.
        let size = widen(u32_at(entry, 8));
        let offset = widen(u32_at(entry, 12));
        let Some(image) = offset
            .checked_add(size)
            .and_then(|end| ico.get(offset..end))
            .filter(|image| !image.is_empty())
        else {
            invalid_ico(
                path,
                &format!(
                    "image {} (offset {offset}, {size} bytes) isn't inside the file",
                    index + 1
                ),
            )
        };

        // The ID is the image's number from 1. The entry is the file's, up to the offset, which the ID replaces.
        let id = checked_u16(index + 1, "the icon images");
        group.extend_from_slice(&entry[..12]);
        push_u16(&mut group, id);
        resources.push(Resource {
            kind: RT_ICON,
            id,
            flags: MOVEABLE | DISCARDABLE,
            data: image.to_vec(),
        });
    }
    resources.push(Resource {
        kind: RT_GROUP_ICON,
        id: APP_ICON_GROUP_ID,
        flags: MOVEABLE | PURE | DISCARDABLE,
        data: group,
    });
    resources
}

/// Stops the build: an icon file that is wrong should fail it loudly, not ship a broken icon.
fn invalid_ico(path: &str, why: &str) -> ! {
    panic!("{path} isn't a valid .ico file: {why}")
}

// ---------------------------------------------------------------------------------------------------------------------
// The version resource
// ---------------------------------------------------------------------------------------------------------------------

/// The numbers and texts Windows shows for the program: Task Manager, the file's Details tab and, with WPC-303, the
/// header of Windows notifications, which reads the FileDescription instead of the file name.
fn version_resource() -> Resource {
    let version = var("CARGO_PKG_VERSION");
    let part = |name: &str| {
        var(name)
            .parse::<u16>()
            .unwrap_or_else(|e| panic!("{name} isn't a 16-bit number: {e}"))
    };
    let (major, minor, patch) = (
        part("CARGO_PKG_VERSION_MAJOR"),
        part("CARGO_PKG_VERSION_MINOR"),
        part("CARGO_PKG_VERSION_PATCH"),
    );
    // No LegalCopyright: the owner hasn't decided one.
    let strings = [
        ("CompanyName", "WPC Solutions"),
        ("FileDescription", "ScreenGauge"),
        ("FileVersion", version.as_str()),
        ("InternalName", "ScreenGauge"),
        ("OriginalFilename", "ScreenGauge.exe"),
        ("ProductName", "ScreenGauge"),
        ("ProductVersion", version.as_str()),
    ];

    // The string table is named by the language and the code page as 8 hex digits.
    let string_table = version_block(
        &format!("{LANGUAGE:04X}{CODE_PAGE:04X}"),
        0,
        true,
        &[],
        &strings.map(|(name, value)| version_string(name, value)),
    );
    let string_file_info = version_block("StringFileInfo", 0, true, &[], &[string_table]);
    // The Translation says which languages the strings are in, so Windows knows which table to read.
    let translation = [LANGUAGE.to_le_bytes(), CODE_PAGE.to_le_bytes()].concat();
    let var_file_info = version_block(
        "VarFileInfo",
        0,
        true,
        &[],
        &[version_block("Translation", 4, false, &translation, &[])],
    );
    let root = version_block(
        "VS_VERSION_INFO",
        52,
        false,
        &fixed_file_info(major, minor, patch),
        &[string_file_info, var_file_info],
    );
    Resource {
        kind: RT_VERSION,
        id: 1,
        flags: MOVEABLE | PURE,
        data: root,
    }
}

/// VS_FIXEDFILEINFO, 52 bytes. The file and product versions are major.minor.patch.0, two numbers to a 32-bit word.
fn fixed_file_info(major: u16, minor: u16, patch: u16) -> Vec<u8> {
    let version_ms = (u32::from(major) << 16) | u32::from(minor);
    let version_ls = u32::from(patch) << 16;
    let mut info = Vec::new();
    for field in [
        0xFEEF_04BD, // dwSignature
        0x0001_0000, // dwStrucVersion
        version_ms,  // dwFileVersionMS
        version_ls,  // dwFileVersionLS
        version_ms,  // dwProductVersionMS
        version_ls,  // dwProductVersionLS
        0x3F,        // dwFileFlagsMask: the flags defined so far
        0,           // dwFileFlags: none (not a debug build, prerelease, patched, ...)
        0x0004_0004, // dwFileOS: VOS_NT_WINDOWS32
        1,           // dwFileType: VFT_APP
        0,           // dwFileSubtype
        0,           // dwFileDateMS
        0,           // dwFileDateLS
    ] {
        push_u32(&mut info, field);
    }
    info
}

/// A `String` block: a name and its text, the length of which counts the terminating null.
fn version_string(name: &str, value: &str) -> Vec<u8> {
    let text = utf16z(value);
    version_block(name, checked_u16(text.len() / 2, name), true, &text, &[])
}

/// A block of a version resource, and every kind of block has this shape: wLength, wValueLength, wType, the key as
/// null-terminated UTF-16, padding, the value, and then the child blocks.
///
/// - `value_length` is in the unit of the value: bytes for a binary value (`is_text` false), UTF-16 units for text, and
///   0 for a block that only holds children.
/// - wLength covers everything above, the padding between the children included, but not the padding after the block's
///   own last byte: whoever holds the block adds it.
/// - Every child starts on a 4-byte boundary. A block is only ever placed on one, so the padding can be counted from
///   the block's own start.
fn version_block(
    key: &str,
    value_length: u16,
    is_text: bool,
    value: &[u8],
    children: &[Vec<u8>],
) -> Vec<u8> {
    let mut block = Vec::new();
    push_u16(&mut block, 0);
    push_u16(&mut block, value_length);
    push_u16(&mut block, u16::from(is_text));
    block.extend_from_slice(&utf16z(key));
    pad_to_dword(&mut block);
    block.extend_from_slice(value);
    for child in children {
        pad_to_dword(&mut block);
        block.extend_from_slice(child);
    }
    let length = checked_u16(block.len(), key);
    block[..2].copy_from_slice(&length.to_le_bytes());
    block
}

// ---------------------------------------------------------------------------------------------------------------------
// Bytes and numbers
// ---------------------------------------------------------------------------------------------------------------------

/// `text` as null-terminated little-endian UTF-16.
fn utf16z(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn pad_to_dword(bytes: &mut Vec<u8>) {
    bytes.resize(bytes.len().next_multiple_of(4), 0);
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// The caller has checked that `offset + 2` is inside `bytes`.
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

/// The caller has checked that `offset + 4` is inside `bytes`.
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// A u32 always fits a usize in this x64-only build.
fn widen(value: u32) -> usize {
    usize::try_from(value).unwrap_or_else(|e| panic!("{value} doesn't fit a usize: {e}"))
}

fn checked_u16(value: usize, what: &str) -> u16 {
    u16::try_from(value).unwrap_or_else(|_| panic!("{what}: {value} doesn't fit in 16 bits"))
}

fn checked_u32(value: usize, what: &str) -> u32 {
    u32::try_from(value).unwrap_or_else(|_| panic!("{what}: {value} doesn't fit in 32 bits"))
}
