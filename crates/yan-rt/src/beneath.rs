//! Reads confined beneath a grant root: no symlink on the way out of it (R674).

/// The text of the file at `comps` under `base` (`/` or `.`), where the first `trusted`
/// components are the grant root itself (followed as written) and every component after them is
/// opened from the previous directory's descriptor with `O_NOFOLLOW`, so no symlink is followed
/// and no `..` is possible. Symlinks inside a grant are refused too.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn read_confined(base: &str, comps: &[&str], trusted: usize) -> Result<String, String> {
    use std::ffi::CString;
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    let denied = || "not granted".to_owned();
    let errno = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => "not found".to_owned(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
        _ => e.to_string(),
    };
    let open = |dir: &OwnedFd, name: &str, flags: i32| -> Result<OwnedFd, std::io::Error> {
        let name = CString::new(name).map_err(|_| std::io::ErrorKind::InvalidInput)?;
        // SAFETY: `dir` is an open descriptor and `name` a NUL-terminated string.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `fd` is a fresh descriptor that nothing else owns.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    };
    let is_symlink = |dir: &OwnedFd, name: &str| {
        let Ok(name) = CString::new(name) else {
            return false;
        };
        // SAFETY: as in `open`; `st` is written by `fstatat` before it is read.
        unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            libc::fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                &mut st,
                libc::AT_SYMLINK_NOFOLLOW,
            ) == 0
                && st.st_mode & libc::S_IFMT == libc::S_IFLNK
        }
    };

    let mut dir = OwnedFd::from(std::fs::File::open(base).map_err(errno)?);
    let Some((file, dirs)) = comps.split_last() else {
        return Err(denied());
    };
    for (i, c) in dirs.iter().enumerate() {
        let follow = if i < trusted { 0 } else { libc::O_NOFOLLOW };
        dir = open(&dir, c, libc::O_RDONLY | libc::O_DIRECTORY | follow).map_err(|e| {
            if is_symlink(&dir, c) {
                denied()
            } else {
                errno(e)
            }
        })?;
    }
    let follow = if dirs.len() < trusted {
        0
    } else {
        libc::O_NOFOLLOW
    };
    let fd = open(&dir, file, libc::O_RDONLY | follow).map_err(|e| {
        if is_symlink(&dir, file) {
            denied()
        } else {
            errno(e)
        }
    })?;
    let mut text = String::new();
    std::fs::File::from(fd)
        .read_to_string(&mut text)
        .map_err(errno)?;
    Ok(text)
}

#[cfg(not(unix))]
pub fn read_confined(_: &str, _: &[&str], _: usize) -> Result<String, String> {
    Err("no host fs handler".to_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::read_confined;
    use std::os::unix::fs::symlink;

    #[test]
    fn refuses_symlinks_out_of_the_grant() {
        let root = std::env::temp_dir().join(format!("yan-beneath-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("box/sub")).unwrap();
        std::fs::write(root.join("secret"), "S").unwrap();
        std::fs::write(root.join("box/sub/ok"), "K").unwrap();
        symlink(root.join("secret"), root.join("box/file")).unwrap();
        symlink(&root, root.join("box/dir")).unwrap();
        symlink("sub/ok", root.join("box/inside")).unwrap();
        let top: Vec<&str> = root
            .to_str()
            .unwrap()
            .split('/')
            .filter(|c| !c.is_empty())
            .collect();
        let read = |rel: &[&str]| {
            let comps: Vec<&str> = top
                .iter()
                .copied()
                .chain(["box"])
                .chain(rel.iter().copied())
                .collect();
            read_confined("/", &comps, top.len() + 1)
        };
        assert_eq!(read(&["sub", "ok"]), Ok("K".to_owned()));
        assert_eq!(read(&["file"]), Err("not granted".to_owned()));
        assert_eq!(read(&["dir", "secret"]), Err("not granted".to_owned()));
        assert_eq!(read(&["inside"]), Err("not granted".to_owned()));
        assert_eq!(read(&["missing"]), Err("not found".to_owned()));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
