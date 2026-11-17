//! `std.sync.mutex` (6.3): the guard unlocks at its scope's exit, on every path out of it. The
//! interpreter, the machine and native code agree.

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

fn run(name: &str, backend: &str, src: &str, flags: &[&str]) -> Output {
    let dir = std::env::temp_dir().join(format!("yan-mutex-{name}-{backend}"));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("mutex.yag");
    std::fs::write(&file, src).unwrap();
    Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["run", "--backend", backend])
        .args(flags)
        .arg(&file)
        .output()
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn the_guard_unlocks_on_every_exit() {
    let out = run("ok", "interp", PROGRAM, &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), OUT);
}

#[test]
fn locking_a_held_mutex_is_a_deadlock_panic() {
    let out = run("relock", "interp", RELOCK, &[]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("deadlock"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn a_guard_held_across_a_wait_is_e323() {
    let out = run("wait", "interp", ACROSS_WAIT, &[]);
    assert!(text(&out.stdout).contains("E323") || text(&out.stderr).contains("E323"));
}

#[test]
fn a_guard_cannot_be_rebound() {
    let out = run("escape", "interp", ESCAPE, &[]);
    assert!(text(&out.stdout).contains("E414") || text(&out.stderr).contains("E414"));
}

const PAR: &str = "def add(m: Mutex[int], n: int) -> None uses nondet:
    for _ in range(n):
        g = m.lock()
        g.set(g.get() + 1)

def push(m: Mutex[list[int]], x: int) -> None uses nondet:
    g = m.lock()
    xs = g.replace([])
    xs.append(x)
    g.set(xs)

def total(m: Mutex[int]) -> int:
    g = m.lock()
    return g.get()

def count(m: Mutex[list[int]]) -> int:
    g = m.lock()
    return len(g.replace([]))

def main() uses io, nondet:
    m = Mutex[int](0)
    par:
        add(m, 100)
        add(m, 100)
    print(total(m))
    l = Mutex[list[int]]([])
    par:
        push(l, 1)
        push(l, 2)
    print(count(l))
";

#[test]
fn every_backend_agrees_and_frees_what_it_allocated() {
    for (name, src, want) in [("seq", PROGRAM, OUT), ("par", PAR, "200\n2\n")] {
        for b in ["interp", "machine", "cranelift"] {
            let out = run(name, b, src, &["--report-live"]);
            assert!(out.status.success(), "{b}: {}", text(&out.stderr));
            assert_eq!(text(&out.stdout), want, "{name} {b}");
            if b == "cranelift" {
                assert!(text(&out.stderr).contains("live: 0"), "{name}");
            }
        }
    }
}

#[test]
fn the_race_detector_sees_no_race_between_critical_sections() {
    let out = run("ub", "machine", PAR, &["--ub"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "200\n2\n");
}

#[test]
fn every_backend_panics_on_a_relock() {
    for b in ["interp", "machine", "cranelift"] {
        let out = run("relock", b, RELOCK, &[]);
        assert_eq!(out.status.code(), Some(101), "{b}");
        assert!(text(&out.stderr).contains("deadlock"), "{b}");
    }
}

const OWNED: &str = "def main() uses io:
    m = Mutex[int](1)
    m.get_mut() += 2
    print(m.get_mut())
    n = Mutex[list[int]]([1])
    n.get_mut().append(2)
    print(n.into_inner())
    s = Mutex[str](\"a\")
    s.get_mut() = \"b\"
    print(s.into_inner())
";

#[test]
fn get_mut_and_into_inner_agree_on_every_backend_and_free_the_block() {
    for b in ["interp", "machine", "cranelift"] {
        let out = run("owned", b, OWNED, &["--report-live"]);
        assert!(out.status.success(), "{b}: {}", text(&out.stderr));
        assert_eq!(text(&out.stdout), "3\n[1, 2]\nb\n", "{b}");
        if b == "cranelift" {
            assert!(text(&out.stderr).contains("live: 0"));
        }
    }
    let out = run("owned-ub", "machine", OWNED, &["--ub"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
}
