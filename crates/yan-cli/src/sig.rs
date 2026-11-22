//! `yan sig <module> [--budget n]`: headers only, as valid Yan Agent text (14, 18.1).

use std::path::Path;
use std::process::ExitCode;

pub fn run(root: &Path, module: &str, budget: Option<usize>) -> Result<String, String> {
    let path = crate::patch::module_path(root, module)?;
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let m = crate::patch::parse(&path.display().to_string(), &src)?;
    Ok(yan_ide::sig(&m, budget))
}

pub fn command(module: &str, budget: Option<usize>) -> ExitCode {
    let src = Path::new("src");
    let root = if src.is_dir() { src } else { Path::new(".") };
    match run(root, module, budget) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(m) => crate::tool_failure(&m),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn prints_headers_only() {
        let dir = std::env::temp_dir().join(format!("yan-sig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.yag"), "pub def f() -> int:\n    return 1\n").unwrap();
        let out = super::run(&dir, "m", None).unwrap();
        assert!(out.contains("pub def f() -> int:") && !out.contains("return 1"));
        assert!(super::run(&dir, "missing", None).is_err());
    }
}
