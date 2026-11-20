//! Embeds each Windows exe's icon and version details (description, product, company, copyright and the package
//! version), which Explorer shows in the file list and under Properties > Details. Each program gets its own resource:
//! OP2Imager the starship, OP2Timelapse the space icon.

fn main() {
    println!("cargo:rerun-if-changed=assets/Tech_Starship.ico");
    println!("cargo:rerun-if-changed=assets/Tech_Space.ico");
    #[cfg(windows)]
    {
        resource("OP2Imager", "Tech_Starship.ico");
        resource("OP2Timelapse", "Tech_Space.ico");
    }
}

/// Writes `bin`'s resource script into OUT_DIR and links it into that program only.
#[cfg(windows)]
fn resource(bin: &str, icon: &str) {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = std::env::var("OUT_DIR").unwrap();
    let icon = format!("{manifest}\\assets\\{icon}").replace('\\', "\\\\");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let numbers: Vec<u16> = version.split(['.', '-']).take(3).map(|n| n.parse().unwrap_or(0)).collect();
    let (major, minor, patch) = (numbers[0], numbers[1], numbers[2]);
    let rc = format!(
        r#"#pragma code_page(65001)
#include <winver.h>
1 ICON "{icon}"
VS_VERSION_INFO VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "OP2Imager"
      VALUE "ProductName", "OP2Imager"
      VALUE "CompanyName", "Outpost Universe"
      VALUE "LegalCopyright", "Outpost Universe © 2026"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "{bin}.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 0x04B0
  END
END
"#
    );
    let path = format!("{out}\\{bin}.rc");
    std::fs::write(&path, rc).unwrap();
    if let Err(e) = embed_resource::compile_for(&path, [bin], embed_resource::NONE).manifest_optional() {
        println!("cargo:warning={bin}: icon and version details not embedded: {e}");
    }
}
