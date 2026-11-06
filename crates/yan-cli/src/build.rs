//! `yan build`: compile to an object and link it with `libyan_rt.a` through the system `cc` (M1b).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use yan_db::Database;

use crate::{checked, lower_native, tool_failure};

pub fn run(file: &Path, output: &Path, mut db: Database) -> ExitCode {
    let module = match checked(&mut db, file) {
        Ok(m) => m,
        Err(code) => return code,
    };
    let program = match lower_native(&file.display().to_string(), module) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let source = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let object = match yan_cg_cranelift::compile_object_debug(&program, &source) {
        Ok(o) => o,
        Err(what) => return tool_failure(&format!("not supported yet: {what}")),
    };
    let Some(rt) = runtime_library() else {
        return tool_failure("libyan_rt.a not found beside the yan executable");
    };
    link(&object, &rt, output).unwrap_or_else(|msg| tool_failure(&msg))
}

/// The runtime archive sits in the directory of the `yan` executable (`target/<profile>`).
fn runtime_library() -> Option<PathBuf> {
    let lib = std::env::current_exe().ok()?.parent()?.join("libyan_rt.a");
    lib.is_file().then_some(lib)
}

fn link(object: &[u8], rt: &Path, output: &Path) -> Result<ExitCode, String> {
    let dir = std::env::temp_dir().join(format!("yan-build-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let o = dir.join("program.o");
    let linked = std::fs::write(&o, object)
        .map_err(|e| format!("{}: {e}", o.display()))
        .and_then(|()| {
            Command::new("cc")
                .arg(&o)
                .arg(rt)
                .args(["-lpthread", "-ldl", "-lm", "-o"])
                .arg(output)
                .output()
                .map_err(|e| format!("cc: {e}"))
        });
    let _ = std::fs::remove_dir_all(&dir);
    let linked = linked?;
    if !linked.status.success() {
        return Err(format!(
            "cc failed\n{}",
            String::from_utf8_lossy(&linked.stderr)
        ));
    }
    Ok(ExitCode::SUCCESS)
}
