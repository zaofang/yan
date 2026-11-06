//! `yan build` (T11c): every `run` case it can compile gives the interpreter's stdout and exit
//! code, and frees every runtime block.

use std::path::{Path, PathBuf};
use std::process::Command;

const YAN: &str = env!("CARGO_BIN_EXE_yan");

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

#[test]
fn built_programs_match_the_interpreter() {
    let lib = Path::new(YAN).parent().unwrap().join("libyan_rt.a");
    if !lib.exists() {
        let mut build = Command::new(env!("CARGO"));
        build.args(["build", "-q", "-p", "yan-rt"]);
        if lib.parent().unwrap().file_name().unwrap() == "release" {
            build.arg("--release");
        }
        assert!(build.status().unwrap().success());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/yag/run");
    let mut files = Vec::new();
    cases(&root, &mut files);
    let dir = std::env::temp_dir().join(format!("yan-build-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let (mut built, mut skipped) = (0, 0);
    for file in files {
        let name = file.file_stem().unwrap().to_string_lossy().into_owned();
        let exe = dir.join(&name);
        let build = Command::new(YAN)
            .arg("build")
            .arg(&file)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        if !build.status.success() {
            skipped += 1;
            continue;
        }
        let expect = Command::new(YAN).arg("run").arg(&file).output().unwrap();
        let run = Command::new(&exe)
            .env("YAN_REPORT_LIVE", "1")
            .output()
            .unwrap();
        assert_eq!(run.stdout, expect.stdout, "{name}: stdout");
        assert_eq!(run.status.code(), expect.status.code(), "{name}: exit code");
        let stderr = String::from_utf8_lossy(&run.stderr);
        if run.status.success() {
            assert!(stderr.contains("live: 0\n"), "{name}: leaked\n{stderr}");
        }
        built += 1;
    }
    let _ = std::fs::remove_dir_all(dir);
    eprintln!("built {built}, not yet native {skipped}");
    assert!(built >= 20, "only {built} run cases build");
}

#[test]
fn a_matched_result_payload_is_freed_once() {
    let dir = std::env::temp_dir().join(format!("yan-result-match-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("r.yag");
    std::fs::write(
        &src,
        "class Bad:\n    code: int\n\n\
         def mk(x: int) -> str fail Bad:\n    if x < 0:\n        fail Bad(code=x)\n    return f\"v{x}\"\n\n\
         pub def main() uses io:\n    a = mk(5) else err -> \"e\"\n    io.print(a)\n\
         \x20   b = mk(-1) else err -> \"e\"\n    io.print(b)\n",
    )
    .unwrap();
    let exe = dir.join("r");
    let build = Command::new(YAN)
        .arg("build")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe)
        .env("YAN_REPORT_LIVE", "1")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(run.status.success(), "{:?}", run.status);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "v5\ne\n");
    assert!(String::from_utf8_lossy(&run.stderr).contains("live: 0\n"));
}

#[test]
fn a_field_read_out_of_a_dropped_owner_is_not_freed() {
    let dir = std::env::temp_dir().join(format!("yan-field-move-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("r.yag");
    std::fs::write(
        &src,
        "@derive(Show)\ntype R = A(m: str) | B(x: str, y: str) | C()\n\n\
         @derive(Show)\nclass E:\n    line: int\n    why: R\n\n\
         class P:\n    name: str\n\n\
         def mk(bad: bool) -> P fail E:\n    if bad:\n        fail E(line=1, why=B(x=\"p\" + \"q\", y=\"r\" + \"s\"))\n    return P(name=\"ok\")\n\n\
         def f(bad: bool) -> str:\n    p = mk(bad) else err -> P(name=f\"{err}\")\n    return p.name\n\n\
         pub def main() uses io:\n    io.print(f(True))\n    io.print(f(False))\n",
    )
    .unwrap();
    let exe = dir.join("r");
    let build = Command::new(YAN)
        .arg("build")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe)
        .env("YAN_REPORT_LIVE", "1")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(run.status.success(), "{:?}", run.status);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "E(line=1, why=B(x='pq', y='rs'))\nok\n"
    );
    assert!(String::from_utf8_lossy(&run.stderr).contains("live: 0\n"));
}
