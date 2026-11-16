//! `std.sync.mutex` (6.3): the guard unlocks at its scope's exit, on every path out of it. The
//! interpreter runs it; the machine and native code refuse it until the lowering lands (#1369).

use std::process::{Command, Output};

const PROGRAM: &str = "def bump(m: Mutex[int]) -> None:
    g = m.lock()
    g.set(g.get() + 1)

def first_big(m: Mutex[int], xs: list[int]) -> int:
    for x in xs:
        g = m.lock()
        if x > 1:
            g.set(x)
            return x
    return 0

def main() uses io:
    m = Mutex[int](0)
    bump(m)
    bump(m)
    print(first_big(m, [1, 2, 3]))
    print(first_big(m, [5]))
    g = m.lock()
    print(g.replace(9))
    print(g.get())
";

const OUT: &str = "2\n5\n5\n9\n";

const RELOCK: &str = "def main() uses io:
    m = Mutex[int](0)
    g = m.lock()
    h = m.lock()
    print(h.get())
";

const ACROSS_WAIT: &str = "def wait() -> None uses time:
    time.sleep(1 * ms) else return

def main() uses time:
    m = Mutex[int](0)
    g = m.lock()
    wait()
    g.set(1)
";

const ESCAPE: &str = "def main() uses io:
    m = Mutex[int](0)
    g = m.lock()
    h = g
    print(h.get())
";

fn run(name: &str, backend: &str, src: &str) -> Output {
    let dir = std::env::temp_dir().join(format!("yan-mutex-{name}-{backend}"));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("mutex.yag");
    std::fs::write(&file, src).unwrap();
    Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["run", "--backend", backend])
        .arg(&file)
        .output()
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn the_guard_unlocks_on_every_exit() {
    let out = run("ok", "interp", PROGRAM);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), OUT);
}

#[test]
fn locking_a_held_mutex_is_a_deadlock_panic() {
    let out = run("relock", "interp", RELOCK);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("deadlock"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn a_guard_held_across_a_wait_is_e323() {
    let out = run("wait", "interp", ACROSS_WAIT);
    assert!(text(&out.stdout).contains("E323") || text(&out.stderr).contains("E323"));
}

#[test]
fn a_guard_cannot_be_rebound() {
    let out = run("escape", "interp", ESCAPE);
    assert!(text(&out.stdout).contains("E414") || text(&out.stderr).contains("E414"));
}

#[test]
fn native_code_refuses_a_mutex_loudly() {
    for b in ["machine", "cranelift"] {
        let out = run("native", b, PROGRAM);
        assert!(!out.status.success(), "{b}");
        assert!(text(&out.stderr).contains("not supported yet"), "{b}");
    }
}
