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
