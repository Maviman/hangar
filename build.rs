use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/hangar.ico");
    println!("cargo:rerun-if-changed=assets/hangar.manifest");
    // Windows only: embed icon, manifest and version info (works with MSVC rc.exe and mingw windres).
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let ver = env::var("CARGO_PKG_VERSION").unwrap();
    let mut n: Vec<u32> = ver.split(['.', '-', '+']).filter_map(|p| p.parse().ok()).collect();
    n.resize(4, 0);
    let assets = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    let p = |f: &str| assets.join(f).to_string_lossy().replace('\\', "/");
    let rc = format!(
        r#"1 ICON "{ico}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},{d}
PRODUCTVERSION {a},{b},{c},{d}
FILEOS 0x40004L
FILETYPE 0x1L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "HANGAR\0"
      VALUE "ProductName", "HANGAR\0"
      VALUE "FileVersion", "{ver}\0"
      VALUE "ProductVersion", "{ver}\0"
      VALUE "OriginalFilename", "HANGAR.exe\0"
      VALUE "InternalName", "hangar\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = p("hangar.ico"),
        manifest = p("hangar.manifest"),
        a = n[0],
        b = n[1],
        c = n[2],
        d = n[3],
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("hangar.rc");
    fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
}
