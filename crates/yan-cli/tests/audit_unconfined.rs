use std::process::Command;

fn audit(manifest_tail: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "yan-audit-unconfined-{}-{}",
        std::process::id(),
        manifest_tail.len()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("package.yag"),
        format!(
            "package: Package = Package(name=\"@t/p\", version=\"0.1.0\", edition=\"next\", yan=\"1.x\", platform=\"cli\"{manifest_tail})\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.yag"),
        "pub def main() uses io:\n    io.print(\"x\")\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .arg("audit")
        .current_dir(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_package_without_capabilities_is_listed_as_unconfined() {
    assert!(audit("").contains("unconfined @t/p"));
    assert!(!audit(", capabilities=[\"io\"]").contains("unconfined"));
}
