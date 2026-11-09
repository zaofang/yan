//! `yan build`: compile to an object and link it with `libyan_rt.a` through the system `cc` (M1b).
//! `--verify` builds twice in separate roots and refuses any byte difference (19.5, E010).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use yan_db::Database;
use yan_diag::{Diagnostic, Span};

use crate::{checked, lower_native, print_diags, tool_failure};

pub fn run(file: &Path, output: &Path, verify: bool, mut db: Database) -> ExitCode {
    let module = match checked(&mut db, file) {
        Ok(m) => m,
        Err(code) => return code,
    };
    let program = match lower_native(&file.display().to_string(), module) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let source = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let Some(rt) = runtime_library() else {
        return tool_failure("libyan_rt.a not found beside the yan executable");
    };
    let name = output.file_name().unwrap_or(output.as_os_str());
    let build = |root: &Path| -> Result<Vec<u8>, String> {
        let object = yan_cg_cranelift::compile_object_debug(&program, &source)
            .map_err(|what| format!("not supported yet: {what}"))?;
        let exe = root.join(name);
        link(&object, &rt, root, &exe)?;
        std::fs::read(&exe).map_err(|e| format!("{}: {e}", exe.display()))
    };
    let roots: Vec<PathBuf> = ["a", "b"]
        .iter()
        .take(if verify { 2 } else { 1 })
        .map(|r| std::env::temp_dir().join(format!("yan-build-{}-{r}", std::process::id())))
        .collect();
    let built: Result<Vec<Vec<u8>>, String> = roots
        .iter()
        .map(|root| {
            std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
            build(
                &root
                    .canonicalize()
                    .map_err(|e| format!("{}: {e}", root.display()))?,
            )
        })
        .collect();
    roots.iter().for_each(|r| drop(std::fs::remove_dir_all(r)));
    let built = match built {
        Ok(b) => b,
        Err(msg) => return tool_failure(&msg),
    };
    if let [a, b] = built.as_slice()
        && let Some(at) = first_difference(a, b)
    {
        print_diags(&[Diagnostic::error(
            "E010",
            &file.display().to_string(),
            Span::default(),
            "",
            format!(
                "build is not reproducible: two roots differ at byte {at} ({} and {} bytes)",
                a.len(),
                b.len()
            ),
        )]);
        return ExitCode::FAILURE;
    }
    if let Err(e) = write_executable(output, &built[0]) {
        return tool_failure(&format!("{}: {e}", output.display()));
    }
    ExitCode::SUCCESS
}

fn first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .or_else(|| (a.len() != b.len()).then(|| a.len().min(b.len())))
}

fn write_executable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// The runtime archive sits beside the resolved `yan` executable (`target/<profile>`, or the
/// release archive's directory behind a Homebrew symlink).
fn runtime_library() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let lib = exe.parent()?.join("libyan_rt.a");
    lib.is_file().then_some(lib)
}

/// Links inside `root` with relative input names, so the root's path never reaches the output.
/// On macOS the debug map records object paths; `-oso_prefix` strips the root from them.
fn link(object: &[u8], rt: &Path, root: &Path, exe: &Path) -> Result<(), String> {
    let o = root.join("program.o");
    std::fs::write(&o, object).map_err(|e| format!("{}: {e}", o.display()))?;
    let mut cc = Command::new("cc");
    cc.current_dir(root)
        .arg("program.o")
        .arg(rt)
        .args(["-lpthread", "-ldl", "-lm", "-o"])
        .arg(exe);
    if cfg!(target_os = "macos") {
        cc.arg(format!("-Wl,-oso_prefix,{}/", root.display()));
    }
    let linked = cc.output().map_err(|e| format!("cc: {e}"))?;
    if !linked.status.success() {
        return Err(format!(
            "cc failed\n{}",
            String::from_utf8_lossy(&linked.stderr)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::first_difference;

    #[test]
    fn first_difference_finds_content_and_length_changes() {
        assert_eq!(first_difference(b"abc", b"abc"), None);
        assert_eq!(first_difference(b"abc", b"abd"), Some(2));
        assert_eq!(first_difference(b"ab", b"abc"), Some(2));
    }
}
