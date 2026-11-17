//! `yan publish --dry-run --info` prints the foreign fields of the version's `.info` (R1034).

use std::process::Command;

#[test]
fn publish_info_lists_ffi_and_libs() {
    let dir = std::env::temp_dir().join(format!("yan-publish-info-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ffi")).unwrap();
    std::fs::write(
        dir.join("package.yag"),
        "package: Package = Package(name=\"@t/z\", version=\"0.1.0\", yan=\"1.x\", platform=\"cli\")\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("ffi/z.yn"),
        "unsafe extern \"C\" lib zlib link \"z\"\n  safe f() -> c_int\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["publish", "--dry-run", "--info"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(r#""ffi":["ffi/z.yn"]"#)
            && stdout.contains(r#""libs":[{"link":"z","name":"zlib"}]"#),
        "{stdout}"
    );
}
