use std::{env, fmt::Write, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=messages.toml");
    let text = fs::read_to_string("messages.toml").unwrap();
    let table: toml::Table = text.parse().unwrap();
    let mut out =
        String::from("pub fn template(key: &str) -> Option<&'static str> {\n    match key {\n");
    for (key, value) in &table {
        let value = value
            .as_str()
            .unwrap_or_else(|| panic!("{key}: template must be a string"));
        writeln!(out, "        {key:?} => Some({value:?}),").unwrap();
    }
    out.push_str("        _ => None,\n    }\n}\n");
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("messages.rs"),
        out,
    )
    .unwrap();
}
