//! `std.sync.atomic` (6.2): the interpreter, the machine and native code agree.

use std::process::Command;

const PROGRAM: &str = "def work(a: Atomic[int]) -> None uses nondet:
    for _ in range(100):
        _ = a.fetch_add(1, Ordering.Relaxed())

def main() uses io, nondet:
    b = Atomic[bool](False)
    print(b.swap(True, Ordering.SeqCst()))
    print(b.fetch_and(False, Ordering.Relaxed()))
    print(f\"{b.compare_exchange(False, True, Ordering.AcqRel(), LoadOrdering.Relaxed())}\")
    u = Atomic[u64](18446744073709551615)
    print(u.fetch_max(5, Ordering.SeqCst()))
    print(u.fetch_add(2, Ordering.SeqCst()))
    c = Atomic[u8](250)
    print(c.fetch_add(10, Ordering.SeqCst()))
    print(c.load(LoadOrdering.SeqCst()))
    d = Atomic[i8](-128)
    print(d.fetch_sub(1, Ordering.SeqCst()))
    print(d.into_inner())
    n = Atomic[int](0)
    par:
        work(n)
        work(n)
    print(n.load(LoadOrdering.SeqCst()))
";

const OUT: &str = "False\nTrue\nOk(value=False)\n18446744073709551615\n18446744073709551615\n250\n4\n-128\n127\n200\n";

const FENCES: &str = "def main() uses io, nondet:
    a = Atomic[int](0)
    b = Atomic[bool](False)
    par:
        a.store(5, StoreOrdering.Relaxed())
        b.store(True, StoreOrdering.Release())
    fence(Ordering.SeqCst())
    fence(Ordering.Acquire())
    compiler_fence(Ordering.Release())
    print(a.load(LoadOrdering.Acquire()))
    print(b.load(LoadOrdering.SeqCst()))
";

fn run(backend: &str) -> String {
    run_program(backend, PROGRAM)
}

fn run_program(backend: &str, program: &str) -> String {
    let dir = std::env::temp_dir().join(format!("yan-atomic-{backend}-{}", program.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("atomic.yag");
    std::fs::write(&file, program).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["run", "--backend", backend, "--report-live"])
        .arg(&file)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{backend}: {err}");
    if backend == "cranelift" {
        assert!(err.contains("live: 0"), "{err}");
    }
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn atomics_agree_on_every_backend() {
    for b in ["interp", "machine", "cranelift"] {
        assert_eq!(run(b), OUT, "{b}");
    }
}

#[test]
fn fences_agree_on_every_backend() {
    for b in ["interp", "machine", "cranelift"] {
        assert_eq!(run_program(b, FENCES), "5\nTrue\n", "{b}");
    }
}
