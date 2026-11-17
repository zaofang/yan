//! Compile-side graph diagnostics and `yan audit` library paths (RFC 0079 D3, D5).

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const EXTERN: &str = "unsafe extern \"C\" lib c link \"c\"\n  safe getpid() -> c_int uses none\n";

fn manifest(name: &str, extra: &str) -> String {
    format!(
        "package: Package = Package(name=\"{name}\", version=\"1.0.0\", yan=\"1.x\", platform=\"cli\"{extra})\n"
    )
}

fn workspace() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yan-ffi-graph-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, rel: &str, text: &str) {
    let file = dir.join(rel);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

fn yan(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn a_member_added_without_lock_gets_e314_and_e023_on_its_package_yag() {
    let dir = workspace();
    write(
        &dir,
        "package.yag",
        &manifest("@t/root", ", members=[\"a\"]"),
    );
    write(
        &dir,
        "a/package.yag",
        &manifest("@t/a", ", capabilities=[\"io\"]"),
    );
    write(&dir, "a/ffi/z.yn", EXTERN);
    let out = yan(&dir, &["check", "a/ffi/z.yn"]);
    let line = |code: &str| {
        out.lines()
            .find(|l| l.starts_with(code) && l.contains("package.yag"))
            .map(str::to_owned)
    };
    let e314 = line("E314").unwrap_or_else(|| panic!("{out}"));
    assert!(
        e314.contains("package.yag") && e314.contains("`c`"),
        "{out}"
    );
    let e023 = line("E023").unwrap_or_else(|| panic!("{out}"));
    assert!(e023.contains("package.yag"), "{out}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_consented_package_with_its_library_in_the_ceiling_is_clean() {
    let dir = workspace();
    write(
        &dir,
        "package.yag",
        &manifest("@t/root", ", members=[\"a\"], allow_ffi=[\"@t/a\"]"),
    );
    write(
        &dir,
        "a/package.yag",
        &manifest("@t/a", ", capabilities=[\"io\", \"c\"]"),
    );
    write(&dir, "a/ffi/z.yn", EXTERN);
    let out = yan(&dir, &["check", "a/ffi/z.yn"]);
    assert!(!out.contains("E314") && !out.contains("E023"), "{out}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn audit_lists_each_foreign_library_with_the_path_that_brings_it_in() {
    let dir = workspace();
    write(
        &dir,
        "package.yag",
        &manifest("@t/root", ", dependencies={\"b\": \"@t/b 1.0\"}"),
    );
    write(&dir, "m.yn", "main() uses io =\n  io.print(1)\n");
    write(&dir, "b/package.yag", &manifest("@t/b", ""));
    write(&dir, "b/ffi/z.yn", EXTERN);
    write(
        &dir,
        "yan.lock",
        "[[package]]\nname = \"@t/b\"\nversion = \"1.0.0\"\npath = \"b\"\n",
    );
    let out = yan(&dir, &["audit"]);
    assert!(
        out.lines()
            .any(|l| l.starts_with("foreign_lib @t/b:c") && l.contains("via @t/root -> @t/b")),
        "{out}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
