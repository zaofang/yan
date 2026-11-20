//! `yan view --as mir --check` (RFC 0097 D7): valid programs pass, a violation
//! reports its I-code and fails.

use std::path::Path;
use std::process::{Command, Output};

fn check(example: &str) -> Output {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(example);
    check_file(&file)
}

fn check_file(file: &Path) -> Output {
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
    let file = std::env::temp_dir().join("yan_mir_check_violation.mir");
    std::fs::write(
        &file,
        "mir 1\nmain fn0\n\nfn 0 \"main\"() -> (unit) {\n  local %0: i64\nbb0:\n  %0 = copy %0\n  ret ()\n}\n",
    )
    .unwrap();
    let out = check_file(&file);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("I103: fn \"main\""));
}
