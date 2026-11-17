//! `&place` of a heap-typed field or element (#1468): the write-back keeps the place's owner.

use std::process::Command;

const PROGRAM: &str = "class H:
    xs: list[int]

def push(var xs: list[int], v: int) -> None:
    xs.append(v)

def reset(var xs: list[int]) -> None:
    xs = [9]

pub def main() uses io:
    h = H(xs=[1, 2])
    push(&h.xs, 3)
    push(&h.xs, 4)
    io.print(f\"{h.xs}\")
    reset(&h.xs)
    io.print(f\"{h.xs}\")
    g = [[1], [2]]
    push(&g[1], 5)
    io.print(f\"{g}\")
";

const OUT: &str = "[1, 2, 3, 4]\n[9]\n[[1], [2, 5]]\n";

fn run(backend: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!("yan-inout-field-{backend}"));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("inout_field.yag");
    std::fs::write(&file, PROGRAM).unwrap();
    Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["run", "--backend", backend])
        .arg(&file)
        .output()
        .unwrap()
}

#[test]
fn inout_of_a_list_field_agrees_on_every_backend() {
    for backend in ["interp", "machine", "cranelift"] {
        let out = run(backend);
        assert!(
            out.status.success(),
            "{backend}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), OUT, "{backend}");
    }
}

#[test]
fn overwriting_an_inout_list_param_frees_the_old_value() {
    let dir = std::env::temp_dir().join("yan-inout-overwrite");
    std::fs::create_dir_all(&dir).unwrap();
    let (src, exe) = (dir.join("p.yag"), dir.join("p"));
    std::fs::write(
        &src,
        "def reset(var xs: list[int]) -> None:
    xs = [9]

pub def main() uses io:
    h = [1, 2]
    reset(&h)
    io.print(f\"{h}\")
",
    )
    .unwrap();
    let built = Command::new(env!("CARGO_BIN_EXE_yan"))
        .arg("build")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .status()
        .unwrap();
    assert!(built.success());
    let out = Command::new(&exe)
        .env("YAN_REPORT_LIVE", "1")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "[9]\n");
    assert!(String::from_utf8_lossy(&out.stderr).contains("live: 0\n"));
}
