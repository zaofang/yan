//! A foreign library is a capability: `uses none` externs still meet the ceiling (12.2, R673),
//! and only `<root>/ffi/` of a package (found by `package.yag`) is an `ffi` module.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const EXTERN: &str = "unsafe extern \"C\" lib c link \"c\"\n  safe getpid() -> c_int uses none\n\nmain() uses io =\n  io.print(getpid())\n";

fn check(caps: &str, module: &str) -> String {
    check_with(caps, "[\"@t/ffi\"]", module)
}

fn check_with(caps: &str, allow: &str, module: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "yan-ffi-gate-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join(module);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        dir.join("package.yag"),
        format!("package: Package = Package(name=\"@t/ffi\", version=\"0.1.0\", yan=\"1.x\", platform=\"cli\", capabilities=[{caps}], allow_ffi={allow})\n"),
    )
    .unwrap();
    std::fs::write(&file, EXTERN).unwrap();
    let out = run_check(&file);
    std::fs::remove_dir_all(&dir).unwrap();
    out
}

fn run_check(file: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .arg("check")
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn uses_none_extern_needs_its_library_in_the_ceiling() {
    let out = check("\"io\"", "ffi/main.yn");
    assert!(out.contains("E314"), "{out}");
    let out = check("\"io\", \"c\"", "ffi/main.yn");
    assert!(!out.contains("E314") && !out.contains("E902"), "{out}");
}

#[test]
fn nested_ffi_directory_is_not_an_ffi_module() {
    let out = check("\"io\", \"c\"", "src/ffi/main.yn");
    assert!(out.contains("E902"), "{out}");
}

#[test]
fn ffi_module_needs_its_package_in_allow_ffi() {
    let out = check_with("\"io\", \"c\"", "[]", "ffi/main.yn");
    assert!(out.contains("E023"), "{out}");
    let out = check_with("\"io\", \"c\"", "[\"@t/ffi\"]", "ffi/main.yn");
    assert!(!out.contains("E023"), "{out}");
}
