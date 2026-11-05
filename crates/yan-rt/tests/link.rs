//! `libyan_rt.a` links into a C program and its entry points work (T11a).

use std::path::PathBuf;
use std::process::Command;

fn static_lib() -> PathBuf {
    // The test binary lives in `target/<profile>/deps`; the archive is one level up.
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().join("libyan_rt.a")
}

#[test]
fn c_program_links_and_runs() {
    let lib = static_lib();
    assert!(lib.exists(), "missing {}", lib.display());
    let dir = std::env::temp_dir().join(format!("yan-rt-link-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("link");
    let src = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/link.c");
    let cc = Command::new("cc")
        .arg(src)
        .arg(&lib)
        .args(["-lpthread", "-ldl", "-lm"])
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        cc.status.success(),
        "{}",
        String::from_utf8_lossy(&cc.stderr)
    );
    let run = Command::new(&exe).output().unwrap();
    assert!(run.status.success(), "exit {:?}", run.status.code());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "1234\n");
    let _ = std::fs::remove_dir_all(dir);
}
