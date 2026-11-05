//! Object emission (T11b): each object defines C `main`, links with `libyan_rt.a` and behaves
//! like the interpreter.

use std::path::{Path, PathBuf};
use std::process::Command;

use yan_db::{Database, Loaded};

const CASES: usize = 20;

fn static_lib() -> PathBuf {
    // The test binary lives in `target/<profile>/deps`; the archive is one level up.
    let exe = std::env::current_exe().unwrap();
    let lib = exe.parent().unwrap().parent().unwrap().join("libyan_rt.a");
    if !lib.exists() {
        let release = lib.parent().unwrap().file_name().unwrap() == "release";
        let mut build = Command::new(env!("CARGO"));
        build.args(["build", "-q", "-p", "yan-rt"]);
        if release {
            build.arg("--release");
        }
        assert!(build.status().unwrap().success());
    }
    assert!(lib.exists(), "missing {}", lib.display());
    lib
}

fn cases(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            cases(&p, out);
        } else if p.extension().is_some_and(|e| e == "yag") {
            out.push(p);
        }
    }
}

/// The checked module of `file`, or `None` if it does not load clean.
fn load(file: &Path) -> Option<yan_core::ast::Module> {
    let mut db = Database::new(false, false);
    let f = db.open(file).ok()?;
    match yan_db::load(&db, f) {
        Ok(Loaded::Ready(m, ..)) if yan_ide::diagnostics(&db, f).ok()?.is_empty() => {
            Some(m.clone())
        }
        _ => None,
    }
}

fn interpreted(m: &yan_core::ast::Module) -> Option<(Vec<u8>, i32)> {
    let mut out = Vec::new();
    let code = match yan_interp::run_with(m, &mut out, Vec::new()) {
        Ok(status) => status,
        Err(yan_interp::Stop::Panic(_)) => yan_interp::EXIT_PANIC,
        Err(_) => return None,
    };
    Some((out, code))
}

#[test]
fn objects_link_and_match_the_interpreter() {
    let lib = static_lib();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/yag/run");
    let mut files = Vec::new();
    cases(&root, &mut files);
    let dir = std::env::temp_dir().join(format!("yan-object-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let mut done = 0;
    for file in files {
        if done == CASES {
            break;
        }
        let Some(m) = load(&file) else { continue };
        let Some(m) = yan_opt::optimize(&file.display().to_string(), &m).ok() else {
            continue;
        };
        let Ok(program) = yan_mir::lower(&m) else {
            continue;
        };
        let Some(expect) = interpreted(&m) else {
            continue;
        };
        let name = file.file_stem().unwrap().to_string_lossy().into_owned();
        let object =
            yan_cg_cranelift::compile_object(&program).unwrap_or_else(|e| panic!("{name}: {e}"));
        let o = dir.join(format!("{name}.o"));
        std::fs::write(&o, object).unwrap();

        let syms = Command::new("objdump").arg("-t").arg(&o).output().unwrap();
        let syms = String::from_utf8_lossy(&syms.stdout);
        assert!(
            syms.lines()
                // Mach-O prefixes symbols with an underscore and names the section `__text`.
                .any(|l| l.contains("text") && (l.ends_with(" main") || l.ends_with(" _main"))),
            "{name}: no defined main\n{syms}"
        );
        assert!(
            syms.contains("yan_rt_exit"),
            "{name}: no runtime import\n{syms}"
        );

        let exe = dir.join(&name);
        let cc = Command::new("cc")
            .arg(&o)
            .arg(&lib)
            .args(["-lpthread", "-ldl", "-lm", "-o"])
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            cc.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&cc.stderr)
        );
        let run = Command::new(&exe).output().unwrap();
        assert_eq!(run.stdout, expect.0, "{name}: stdout");
        assert_eq!(run.status.code(), Some(expect.1), "{name}: exit code");
        done += 1;
    }
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(
        done, CASES,
        "fewer than {CASES} run cases compile to objects"
    );
}
