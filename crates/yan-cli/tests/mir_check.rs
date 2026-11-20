//! `yan view --as mir --check` (RFC 0097 D7): valid programs pass, a known lowering gap (#1810)
//! reports its I-code and fails.

use std::path::Path;
use std::process::{Command, Output};

fn check(example: &str) -> Output {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(example);
    Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["view", "--as", "mir", "--check"])
        .arg(file)
        .output()
        .unwrap()
}

#[test]
fn a_valid_program_passes() {
    let out = check("conformance/golden/g1_hello.yag");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("mir 1\n"));
}

#[test]
fn a_violation_is_reported_with_its_code_and_fails() {
    let out = check("conformance/yag/run/std/iter_for_loop.yag");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("I103: fn \"main\""));
}
