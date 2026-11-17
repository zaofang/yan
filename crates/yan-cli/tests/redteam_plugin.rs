//! The red-team demo (`examples/redteam-plugin/attacks.toml`): each attack is refused, or is a
//! recorded gap that pins today's behaviour.

use std::path::{Path, PathBuf};
use std::process::Command;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(entry.path()).unwrap(), dest).unwrap();
        } else if kind.is_dir() {
            copy(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/redteam-plugin")
}

#[test]
#[cfg(unix)]
fn every_attack_is_refused_or_a_recorded_gap() {
    let manifest: toml::Table = std::fs::read_to_string(demo().join("attacks.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let scratch = std::env::temp_dir().join(format!("yan-redteam-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    for attack in manifest["attack"].as_array().unwrap() {
        let id = attack["id"].as_str().unwrap();
        let dir = attack["dir"].as_str().unwrap();
        if dir.is_empty() {
            assert_eq!(attack["status"].as_str(), Some("gap"), "attack {id}");
            continue;
        }
        let work = scratch.join(id);
        copy(&demo().join(dir), &work);
        let args: Vec<&str> = attack["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        let out = Command::new(env!("CARGO_BIN_EXE_yan"))
            .args(&args)
            .current_dir(&work)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let both = format!("{stdout}{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(
            i64::from(out.status.code().unwrap()),
            attack["exit"].as_integer().unwrap(),
            "attack {id}: {both}"
        );
        if let Some(want) = attack.get("stdout") {
            assert_eq!(stdout, want.as_str().unwrap(), "attack {id}");
        }
        for want in attack
            .get("contains")
            .and_then(|c| c.as_array())
            .into_iter()
            .flatten()
        {
            let want = want.as_str().unwrap();
            assert!(
                both.contains(want),
                "attack {id}: missing `{want}` in {both}"
            );
        }
    }
    std::fs::remove_dir_all(&scratch).unwrap();
}

#[test]
fn the_host_runs_with_the_attacks_disabled() {
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(["run", "src/main.yag"])
        .current_dir(demo().join("host"))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\n\n");
}
