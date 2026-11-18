//! `yan mv from to` (RFC 0084 D11): move a module file of the package and rewrite every `use` and
//! qualified name that reaches it. Names are always qualified (D4), so the edit is mechanical.

use std::path::{Path, PathBuf};

use yan_db::{Root, path_issue, scan_layout};

fn split(path: &str) -> Vec<String> {
    path.split('.').map(str::to_owned).collect()
}

/// Move module `from` to `to` under `src/` and rewrite the package's files. Returns the files
/// written, the moved module first.
pub fn run(root: &Path, from: &str, to: &str) -> Result<Vec<PathBuf>, String> {
    if !root.join("package.yag").is_file() {
        return Err("`yan mv` needs a package.yag in the current directory".into());
    }
    let (from, to) = (split(from), split(to));
    if let Some(why) = path_issue(&to) {
        return Err(why);
    }
    let (modules, _) = scan_layout(root);
    let moved = modules
        .iter()
        .find(|m| m.root == Root::Src && m.path == from)
        .ok_or_else(|| format!("no module `{}` under src/", from.join(".")))?;
    if modules.iter().any(|m| m.root == Root::Src && m.path == to) {
        return Err(format!("module `{}` already exists", to.join(".")));
    }
    let ext = moved
        .file
        .extension()
        .expect("a module file has an extension");
    let mut target = root.join("src");
    target.extend(&to);
    let target = target.with_extension(ext);

    let mut edits = Vec::new();
    for m in &modules {
        let text =
            std::fs::read_to_string(&m.file).map_err(|e| format!("{}: {e}", m.file.display()))?;
        let own_dir = |path: &[String]| match m.root {
            Root::Src => path[..path.len() - 1].to_vec(),
            _ => Vec::new(),
        };
        let (old_dir, new_dir) = if m.file == moved.file {
            (own_dir(&from), own_dir(&to))
        } else {
            (own_dir(&m.path), own_dir(&m.path))
        };
        let out = rewrite(&text, &from, &to, &old_dir, &new_dir);
        if out != text {
            edits.push((m.file.clone(), out));
        }
    }
    let mut written = vec![target.clone()];
    std::fs::create_dir_all(target.parent().expect("a file below src/"))
        .map_err(|e| format!("{}: {e}", target.display()))?;
    std::fs::rename(&moved.file, &target).map_err(|e| format!("{}: {e}", moved.file.display()))?;
    prune(
        moved.file.parent().expect("a file below src/"),
        &root.join("src"),
    );
    for (file, text) in edits {
        let file = if file == moved.file {
            target.clone()
        } else {
            file
        };
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        if !written.contains(&file) {
            written.push(file);
        }
    }
    Ok(written)
}

/// Remove `dir` and its parents below `stop` while they are empty.
fn prune(dir: &Path, stop: &Path) {
    let mut dir = dir;
    while dir != stop && std::fs::remove_dir(dir).is_ok() {
        match dir.parent() {
            Some(p) => dir = p,
            None => break,
        }
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The end of the dotted identifier path starting at `start`.
fn path_end(c: &[char], start: usize) -> usize {
    let mut end = start;
    while end < c.len() && is_ident(c[end]) {
        end += 1;
        if end + 1 < c.len() && c[end] == '.' && (c[end + 1].is_alphabetic() || c[end + 1] == '_') {
            end += 1;
        }
    }
    end
}

/// `text` with every qualified name and `use` path that starts with `from` rewritten to start
/// with `to`, outside strings and comments. `old_dir` and `new_dir` are the file's directory
/// before and after the move, which decide what a leading-dot `use` means (D4).
fn rewrite(
    text: &str,
    from: &[String],
    to: &[String],
    old_dir: &[String],
    new_dir: &[String],
) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut line_start = true;
    while i < c.len() {
        let at_line_start = std::mem::replace(&mut line_start, c[i] == '\n');
        let triple = c[i] == '"' && c[i..].starts_with(&['"', '"', '"']);
        if triple {
            let close = (i + 3..c.len()).find(|&j| c[j..].starts_with(&['"', '"', '"']));
            let end = close.map_or(c.len(), |j| j + 3);
            out.extend(&c[i..end]);
            i = end;
        } else if c[i] == '"' {
            let mut j = i + 1;
            while j < c.len() && c[j] != '"' && c[j] != '\n' {
                j += if c[j] == '\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(c.len());
            out.extend(&c[i..end]);
            i = end;
        } else if c[i] == '#' {
            let end = (i..c.len()).find(|&j| c[j] == '\n').unwrap_or(c.len());
            out.extend(&c[i..end]);
            i = end;
        } else if (c[i].is_alphabetic() || c[i] == '_')
            && (i == 0 || !is_ident(c[i - 1]) && c[i - 1] != '.')
        {
            let end = path_end(&c, i);
            let path: String = c[i..end].iter().collect();
            if at_line_start && path == "use" {
                let mut p = end;
                while p < c.len() && c[p] == ' ' {
                    p += 1;
                }
                let dot = usize::from(c.get(p) == Some(&'.'));
                let p_end = path_end(&c, p + dot).max(p + dot);
                out.push_str(&path);
                out.extend(&c[end..p]);
                let used: String = c[p..p_end].iter().collect();
                out.push_str(&rewrite_use(&used, from, to, old_dir, new_dir));
                i = p_end;
            } else {
                let segs = split(&path);
                if segs.starts_with(from) {
                    let all: Vec<&String> = to.iter().chain(&segs[from.len()..]).collect();
                    out.push_str(&all.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("."));
                } else {
                    out.push_str(&path);
                }
                i = end;
            }
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

fn rewrite_use(
    path: &str,
    from: &[String],
    to: &[String],
    old_dir: &[String],
    new_dir: &[String],
) -> String {
    let relative = path.starts_with('.');
    let segs = split(path.trim_start_matches('.'));
    let absolute: Vec<String> = if relative {
        old_dir.iter().chain(&segs).cloned().collect()
    } else {
        segs
    };
    let absolute = if absolute.starts_with(from) {
        to.iter().chain(&absolute[from.len()..]).cloned().collect()
    } else {
        absolute
    };
    match (relative, absolute.strip_prefix(new_dir)) {
        (true, Some(rest)) if !rest.is_empty() => format!(".{}", rest.join(".")),
        _ => absolute.join("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yan-mv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src/billing")).unwrap();
        std::fs::write(d.join("package.yag"), "package: Package = Package(name=\"@t/a\", version=\"0.1.0\", edition=\"next\", yan=\"1.x\")\n").unwrap();
        d
    }

    fn dots(p: &str) -> Vec<String> {
        split(p)
    }

    #[test]
    fn rewrites_names_and_uses_but_not_strings_or_comments() {
        let text = "use shapes as sh\nuse shapes\n# shapes.x\ndef f() = shapes.area(\"shapes.x\") + other.shapes.y\n";
        let out = rewrite(text, &dots("shapes"), &dots("geo.shapes"), &[], &[]);
        assert_eq!(
            out,
            "use geo.shapes as sh\nuse geo.shapes\n# shapes.x\ndef f() = geo.shapes.area(\"shapes.x\") + other.shapes.y\n"
        );
    }

    #[test]
    fn a_relative_use_keeps_its_meaning() {
        let (from, to) = (dots("billing.invoice"), dots("finance.invoice"));
        let billing = dots("billing");
        let finance = dots("finance");
        // A sibling in billing/ now reaches the module by its full path.
        assert_eq!(
            rewrite("use .invoice\n", &from, &to, &billing, &billing),
            "use finance.invoice\n"
        );
        // The moved file's own relative use of a file it left behind becomes absolute.
        assert_eq!(
            rewrite("use .tax\n", &from, &to, &billing, &finance),
            "use billing.tax\n"
        );
        // ...and stays relative when the target moved along.
        assert_eq!(
            rewrite(
                "use .tax\n",
                &dots("billing.tax"),
                &dots("finance.tax"),
                &billing,
                &finance
            ),
            "use .tax\n"
        );
    }

    #[test]
    fn moves_the_file_and_rewrites_its_users() {
        let d = tmp("move");
        std::fs::write(
            d.join("src/billing/invoice.yag"),
            "pub def n() -> int:\n    return 1\n",
        )
        .unwrap();
        std::fs::write(
            d.join("src/main.yag"),
            "pub def m() -> int:\n    return billing.invoice.n()\n",
        )
        .unwrap();
        let written = run(&d, "billing.invoice", "finance.invoice").unwrap();
        assert_eq!(written[0], d.join("src/finance/invoice.yag"));
        assert!(!d.join("src/billing").exists());
        let main = std::fs::read_to_string(d.join("src/main.yag")).unwrap();
        assert!(main.contains("finance.invoice.n()"), "{main}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn refuses_a_missing_source_a_taken_target_and_a_bad_name() {
        let d = tmp("refuse");
        std::fs::write(d.join("src/a.yag"), "").unwrap();
        std::fs::write(d.join("src/b.yag"), "").unwrap();
        assert!(run(&d, "nope", "c").unwrap_err().contains("no module"));
        assert!(run(&d, "a", "b").unwrap_err().contains("already exists"));
        assert!(run(&d, "a", "Bad").unwrap_err().contains("cannot name"));
        assert!(run(&d, "a", "std").unwrap_err().contains("reserved"));
        assert!(d.join("src/a.yag").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
