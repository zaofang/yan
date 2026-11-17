//! `yan metadata --json` and dependency root names (RFC 0084 D4, D13).

use std::path::Path;
use std::process::Command;

fn write(dir: &Path, rel: &str, text: &str) {
    let file = dir.join(rel);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

fn workspace(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("yan-metadata-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn yan(dir: &Path, args: &[&str]) -> (String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_yan"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn manifest(name: &str, extra: &str) -> String {
    format!(
        "package: Package = Package(name=\"{name}\", version=\"1.2.0\", yan=\"1.x\", platform=\"cli\"{extra})\n"
    )
}

#[test]
fn metadata_lists_packages_local_names_and_what_they_resolve_to() {
    let d = workspace("graph");
    write(
        &d,
        "package.yag",
        &manifest(
            "@t/root",
            ", members=[\"http\"], dependencies={\"web\": \"@t/http 1.2\"}, dev_dependencies=[\"@t/http 1.2\"]",
        ),
    );
    write(&d, "src/main.yag", "fn main() -> int:\n    0\n");
    write(&d, "http/package.yag", &manifest("@t/http", ""));
    write(&d, "http/src/lib.yag", "pub fn get() -> int:\n    1\n");
    write(&d, "http/src/client.yag", "pub fn open() -> int:\n    2\n");
    let (out, _) = yan(&d, &["metadata", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).expect(&out);
    assert_eq!(v["root"], "@t/root");
    let root = &v["packages"][0];
    assert_eq!(root["dependencies"][0]["local"], "web");
    assert_eq!(root["dependencies"][0]["resolved"]["name"], "@t/http");
    assert_eq!(root["dependencies"][1]["local"], "http");
    assert_eq!(root["dependencies"][1]["kind"], "dev_dependencies");
    let member = v["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["role"] == "member")
        .unwrap();
    let modules: Vec<_> = member["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["path"].as_str().unwrap())
        .collect();
    assert_eq!(modules, ["client", "lib"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_dependency_root_that_collides_with_a_module_or_a_reserved_name_is_e024() {
    let d = workspace("collide");
    write(
        &d,
        "package.yag",
        &manifest(
            "@t/root",
            ", dependencies={\"http\": \"@t/http 1.2\", \"std\": \"@t/std 1.0\"}",
        ),
    );
    write(&d, "src/main.yag", "fn main() -> int:\n    0\n");
    write(&d, "src/http.yag", "pub fn get() -> int:\n    1\n");
    let (out, err) = yan(&d, &["check", "src/main.yag"]);
    let text = out + &err;
    assert_eq!(text.matches("E024").count(), 2, "{text}");
    assert!(text.contains("top-level module `http`"), "{text}");
    assert!(text.contains("`std` is a reserved root name"), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}
