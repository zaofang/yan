//! A closure argument that captures a capability must be covered by the callee's `uses` (R1025).

use std::process::Command;

fn check(src: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "yan-closure-caps-{}-{}",
        std::process::id(),
        src.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m.yag"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["check", "m.yag"])
        .current_dir(&dir)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

const MAIN: &str =
    "pub def main() uses io, fs:\n    io.print(apply(lambda: fs.read(\"x\") else \"x\"))\n";

#[test]
fn uses_none_callee_cannot_run_a_capturing_closure() {
    let out = check(&format!(
        "def apply(f: Callable[[], str]) -> str uses none:\n    return f()\n\n{MAIN}"
    ));
    assert!(out.contains("E301"), "{out}");
}

#[test]
fn callee_that_declares_the_capability_is_fine() {
    let out = check(&format!(
        "def apply(f: Callable[[], str]) -> str uses fs:\n    return f()\n\n{MAIN}"
    ));
    assert!(!out.contains("E301"), "{out}");
}

#[test]
fn pure_closure_is_fine_for_uses_none() {
    let out = check(
        "def apply(f: Callable[[], str]) -> str uses none:\n    return f()\n\npub def main() uses io:\n    io.print(apply(lambda: \"y\"))\n",
    );
    assert!(!out.contains("E301"), "{out}");
}
