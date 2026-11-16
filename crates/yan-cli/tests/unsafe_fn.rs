//! `unsafe fn` (RFC 0076): a `pre` that fails panics as a contract on the interpreter and native
//! code and is `ub(16)` under `--ub`; misuse is a diagnostic.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/unsafe_fn/ffi/get.yn")
}

fn yan(args: &[&str], file: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(args)
        .arg(file)
        .output()
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn failed_pre_is_a_contract_panic_on_every_backend() {
    for backend in ["interp", "cranelift", "machine"] {
        let out = yan(&["run", "--backend", backend], &example());
        assert_eq!(text(&out.stdout), "30\n", "{backend}");
        assert!(text(&out.stderr).contains("panic(contract)"), "{backend}");
        assert_eq!(out.status.code(), Some(101), "{backend}");
    }
}

#[test]
fn failed_pre_is_ub_16_under_ub() {
    let out = yan(&["run", "--ub"], &example());
    assert_eq!(text(&out.stdout), "30\n");
    assert!(
        text(&out.stderr)
            .contains("ub(16): a call to `unsafe fn get` breaks its precondition, clause 0"),
        "{}",
        text(&out.stderr)
    );
}

fn diagnostics(test: &str, dir: &str, src: &str) -> String {
    let root = std::env::temp_dir().join(format!("yan-unsafe-fn-{}-{test}", std::process::id()));
    let file = root.join(dir).join("m.yn");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, src).unwrap();
    let out = yan(&["check"], &file);
    std::fs::remove_dir_all(&root).unwrap();
    text(&out.stdout) + &text(&out.stderr)
}

const FIRST: &str =
    "unsafe first(p: Ptr[int]) -> int =\n  var r = 0\n  unsafe\n    r = p.read()\n  r\n";

#[test]
fn call_outside_unsafe_is_e903() {
    let d = diagnostics(
        "call_outside_unsafe_is_e903",
        "ffi",
        &format!("{FIRST}\nf(p: Ptr[int]) -> int = first(p)\n"),
    );
    assert!(
        d.contains("E903") && d.contains("`first` is an `unsafe fn`"),
        "{d}"
    );
}

#[test]
fn body_is_not_an_unsafe_block() {
    let d = diagnostics(
        "body_is_not_an_unsafe_block",
        "ffi",
        "unsafe first(p: Ptr[int]) -> int = p.read()\n",
    );
    assert!(d.contains("E903") && d.contains("m.yn:1:36"), "{d}");
}

#[test]
fn unsafe_fn_as_value_is_e912() {
    let d = diagnostics(
        "unsafe_fn_as_value_is_e912",
        "ffi",
        &format!("{FIRST}\nf() -> int =\n  let g = first\n  0\n"),
    );
    assert!(d.contains("E912"), "{d}");
}

#[test]
fn unsafe_fn_outside_ffi_is_e901_only() {
    let d = diagnostics("unsafe_fn_outside_ffi_is_e901_only", "src", FIRST);
    assert!(d.contains("E901") && !d.contains("E017"), "{d}");
}
