//! Embed the Windows icon and version metadata into `nori.exe`.
//!
//! The resource is compiled by the Windows SDK's `rc.exe`, so it only runs
//! when the build itself is on Windows. `cargo check` and non-Windows builds
//! are unaffected.
//!
//! The application manifest is not here: GPUI already embeds one (its
//! `windows-manifest` feature is on by default), and a second MANIFEST
//! resource makes `cvtres` fail the link with CVT1100.

fn main() {
    #[cfg(windows)]
    {
        let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        if target_os == "windows" {
            embed_resources();
        }
    }
}

#[cfg(windows)]
fn embed_resources() {
    use std::path::PathBuf;

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let rc_path = out_dir.join("nori.rc");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let (major, minor, patch, build) = version_parts(&version);

    // Resource paths are relative to the crate root, which is the working
    // directory Cargo runs build scripts from.
    let body = format!(
        r#"1 ICON "assets/windows/nori.ico"

VS_VERSION_INFO VERSIONINFO
 FILEVERSION {major},{minor},{patch},{build}
 PRODUCTVERSION {major},{minor},{patch},{build}
 FILEFLAGSMASK 0x3fL
 FILEFLAGS 0x0L
 FILEOS 0x40004L
 FILETYPE 0x1L
 FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Nori"
            VALUE "FileDescription", "Nori torrent finder"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "nori"
            VALUE "OriginalFilename", "nori.exe"
            VALUE "ProductName", "Nori"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );

    std::fs::write(&rc_path, body).expect("write the generated Windows resource script");
    println!("cargo:rerun-if-changed=assets/windows/nori.ico");

    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_required()
        .expect("embed the Windows icon and version info");
}

#[cfg(windows)]
fn version_parts(version: &str) -> (u16, u16, u16, u16) {
    let mut parts = version
        .split(['.', '-', '+'])
        .filter_map(|part| part.parse::<u16>().ok());
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}
