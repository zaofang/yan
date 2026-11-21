//! `fs.list_dir` on the host lists sorted entries on every backend, and `fs.sub` lists beneath
//! its grant without following symlinks out of it (R674).

use std::process::Command;

#[test]
#[cfg(unix)]
fn list_dir_agrees_on_every_backend() {
    let dir = std::env::temp_dir().join(format!("yan-fs-list-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("box/sub")).unwrap();
    std::fs::write(dir.join("box/b"), "").unwrap();
    std::fs::write(dir.join("box/a"), "").unwrap();
    std::os::unix::fs::symlink(&dir, dir.join("box/out")).unwrap();
    std::fs::write(
        dir.join("m.yag"),
        "pub def main() uses io, fs:\n    for p in fs.list_dir(\"box\") else []:\n        io.print(p.text())\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("sub.yag"),
        "pub def main() uses io, fs:\n    with fs = fs.sub(\"box\"):\n        for p in fs.list_dir(\".\") else []:\n            io.print(p.text())\n        io.print(len(fs.list_dir(\"out\") else []))\n",
    )
    .unwrap();
    let run = |backend: &str, file: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_yan"))
            .args(["run", "--backend", backend, file])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{backend}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    for backend in ["interp", "machine", "cranelift"] {
        assert_eq!(
            run(backend, "m.yag"),
            "box/a\nbox/b\nbox/out\nbox/sub\n",
            "{backend}"
        );
    }
    assert_eq!(run("interp", "sub.yag"), "./a\n./b\n./out\n./sub\n0\n");
    std::fs::remove_dir_all(&dir).unwrap();
}
