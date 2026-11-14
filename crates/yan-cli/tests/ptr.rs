//! `core.ptr` (6.6): the provenance functions run on the interpreter.

use std::path::Path;
use std::process::Command;

#[test]
fn ptr_provenance_runs_on_the_interpreter() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/ptr/ffi/provenance.yn");
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .arg("run")
        .arg(file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "4104\nTrue\n1\n4104\n"
    );
}

#[test]
fn ptr_provenance_runs_natively() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/ptr/ffi/provenance.yn");
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["run", "--backend", "cranelift", "--report-live"])
        .arg(file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "4104\nTrue\n1\n4104\n"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("live: 0"));
}
