//! DWARF line tables (T32): a debugger breakpoint on a source line stops there in a built program.
//! Skips when neither `gdb` nor `lldb` is installed.

use std::path::PathBuf;
use std::process::Command;

use yan_db::{Database, Loaded};

const SOURCE: &str = "def square(x: int) -> int:
    y = x * x
    return y

pub def main() uses io:
    a = square(3)
    b = square(a)
    print(b)
";

fn runtime() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let lib = exe.parent().unwrap().parent().unwrap().join("libyan_rt.a");
    if !lib.exists() {
        let mut build = Command::new(env!("CARGO"));
        build.args(["build", "-q", "-p", "yan-rt"]);
        if lib.parent().unwrap().file_name().unwrap() == "release" {
            build.arg("--release");
        }
        assert!(build.status().unwrap().success());
    }
    lib
}

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

#[test]
fn breakpoint_on_a_source_line_stops_there() {
    let (batch, quit): (&[&str], _) = if have("gdb") {
        (&["-batch", "-ex", "break demo.yag:7", "-ex", "run"], "gdb")
    } else if have("lldb") {
        (
            // lldb hangs on launch under CI, so it only resolves the line to code.
            &[
                "-b",
                "-o",
                "breakpoint set -f demo.yag -l 7",
                "-o",
                "breakpoint list",
            ],
            "lldb",
        )
    } else {
        eprintln!("skipped: no debugger installed");
        return;
    };

    let dir = std::env::temp_dir().join(format!("yan-debug-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("demo.yag");
    std::fs::write(&src, SOURCE).unwrap();

    let mut db = Database::new(false, false);
    let f = db.open(&src).unwrap();
    let Ok(Loaded::Ready(m, ..)) = yan_db::load(&db, f) else {
        panic!("demo does not load");
    };
    assert!(yan_ide::diagnostics(&db, f).unwrap().is_empty());
    let m = yan_opt::optimize(&src.display().to_string(), m).unwrap();
    let program = yan_mir::lower(&m).unwrap();
    let object = yan_cg_cranelift::compile_object_debug(&program, &src).unwrap();
    let o = dir.join("demo.o");
    std::fs::write(&o, object).unwrap();

    let exe = dir.join("demo");
    let cc = Command::new("cc")
        .arg(&o)
        .arg(runtime())
        .args(["-lpthread", "-ldl", "-lm", "-o"])
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        cc.status.success(),
        "{}",
        String::from_utf8_lossy(&cc.stderr)
    );

    let out = Command::new(quit)
        .args(batch)
        .arg(&exe)
        .current_dir(&dir)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("demo.yag:7") && (quit == "lldb" || text.contains("b = square(a)")),
        "{quit} did not stop on line 7:\n{text}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
