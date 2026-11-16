//! `fs.sub` does not follow a symlink out of its directory (R674).

use std::process::Command;

#[test]
#[cfg(unix)]
fn fs_sub_refuses_symlink_out_of_the_grant() {
    let dir = std::env::temp_dir().join(format!("yan-fs-sub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("box")).unwrap();
    std::fs::write(dir.join("secret"), "SECRET").unwrap();
    std::fs::write(dir.join("box/ok"), "fine").unwrap();
    std::os::unix::fs::symlink(dir.join("secret"), dir.join("box/link")).unwrap();
    std::fs::write(
        dir.join("m.yag"),
        "pub def main() uses io, fs:\n    with fs = fs.sub(\"box\"):\n        io.print(fs.read(\"link\") else e -> e.reason)\n        io.print(fs.read(\"ok\") else e -> e.reason)\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["run", "--backend", "interp", "m.yag"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "not granted\nfine\n",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
