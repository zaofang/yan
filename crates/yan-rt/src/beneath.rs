//! Reads and listings confined beneath a grant root: no symlink on the way out of it (R674).

/// Descriptor walks from `base` (`/` or `.`): the first `trusted` components are the grant root
/// itself, followed as written; every one after them is opened from the previous directory's
/// descriptor with `O_NOFOLLOW`, so no symlink is followed and no `..` is possible. Symlinks inside
/// a grant are refused too.
#[cfg(unix)]
#[allow(unsafe_code)]
mod unix {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    pub fn denied() -> String {
        "not granted".to_owned()
    }

    pub fn errno(e: std::io::Error) -> String {
        match e.kind() {
            std::io::ErrorKind::NotFound => "not found".to_owned(),
            std::io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
            _ => e.to_string(),
        }
    }

    pub fn open(dir: &OwnedFd, name: &str, flags: i32) -> Result<OwnedFd, std::io::Error> {
        let name = CString::new(name).map_err(|_| std::io::ErrorKind::InvalidInput)?;
        // SAFETY: `dir` is an open descriptor and `name` a NUL-terminated string.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `fd` is a fresh descriptor that nothing else owns.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    fn is_symlink(dir: &OwnedFd, name: &str) -> bool {
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
    }

    /// `name` of `dir` opened with `flags`, with `O_NOFOLLOW` unless `follow`.
    pub fn step(dir: &OwnedFd, name: &str, flags: i32, follow: bool) -> Result<OwnedFd, String> {
        let nofollow = if follow { 0 } else { libc::O_NOFOLLOW };
        open(dir, name, flags | nofollow).map_err(|e| {
            if is_symlink(dir, name) {
                denied()
            } else {
                errno(e)
            }
        })
    }

    pub fn walk(base: &str, dirs: &[&str], trusted: usize) -> Result<OwnedFd, String> {
        let mut dir = OwnedFd::from(std::fs::File::open(base).map_err(errno)?);
        for (i, c) in dirs.iter().enumerate() {
            dir = step(&dir, c, libc::O_RDONLY | libc::O_DIRECTORY, i < trusted)?;
        }
        Ok(dir)
    }
}

/// The text of the file at `comps` under `base`; confined as described on [`unix`].
#[cfg(unix)]
pub fn read_confined(base: &str, comps: &[&str], trusted: usize) -> Result<String, String> {
    use std::io::Read;

    let Some((file, dirs)) = comps.split_last() else {
        return Err(unix::denied());
    };
    let dir = unix::walk(base, dirs, trusted)?;
    let fd = unix::step(&dir, file, libc::O_RDONLY, dirs.len() < trusted)?;
    let mut text = String::new();
    std::fs::File::from(fd)
        .read_to_string(&mut text)
        .map_err(unix::errno)?;
    Ok(text)
}

/// The entry names of the directory at `comps` under `base`, unsorted and without `.` and `..`;
/// a name that is not UTF-8 fails with `NotUnicode`. Confined as [`read_confined`] is.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn list_confined(base: &str, comps: &[&str], trusted: usize) -> Result<Vec<String>, String> {
    use std::ffi::CStr;
    use std::os::fd::IntoRawFd;

    let fd = unix::walk(base, comps, trusted)?.into_raw_fd();
    // SAFETY: `fd` is an open directory descriptor that `fdopendir` takes over.
    let d = unsafe { libc::fdopendir(fd) };
    if d.is_null() {
        let e = std::io::Error::last_os_error();
        // SAFETY: `fdopendir` failed, so `fd` is still ours to close.
        unsafe { libc::close(fd) };
        return Err(unix::errno(e));
    }
    let mut names = Vec::new();
    let mut result = Ok(());
    loop {
        // SAFETY: `d` is an open directory stream.
        let e = unsafe { libc::readdir(d) };
        if e.is_null() {
            break;
        }
        // SAFETY: `d_name` of a returned entry is NUL-terminated.
        let name = unsafe { CStr::from_ptr((*e).d_name.as_ptr()) };
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        match name.to_str() {
            Ok(n) => names.push(n.to_owned()),
            Err(_) => {
                result = Err("NotUnicode".to_owned());
                break;
            }
        }
    }
    // SAFETY: `d` is open and not used again.
    unsafe { libc::closedir(d) };
    result.map(|()| names)
}

#[cfg(not(unix))]
pub fn read_confined(_: &str, _: &[&str], _: usize) -> Result<String, String> {
    Err("no host fs handler".to_owned())
}

#[cfg(not(unix))]
pub fn list_confined(_: &str, _: &[&str], _: usize) -> Result<Vec<String>, String> {
    Err("no host fs handler".to_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::{list_confined, read_confined};
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
        let list = |rel: &[&str]| {
            let comps: Vec<&str> = top
                .iter()
                .copied()
                .chain(["box"])
                .chain(rel.iter().copied())
                .collect();
            list_confined("/", &comps, top.len() + 1).map(|mut n| {
                n.sort();
                n
            })
        };
        assert_eq!(
            list(&[]),
            Ok(vec![
                "dir".into(),
                "file".into(),
                "inside".into(),
                "sub".into()
            ])
        );
        assert_eq!(list(&["sub"]), Ok(vec!["ok".into()]));
        assert_eq!(list(&["dir"]), Err("not granted".to_owned()));
        assert_eq!(list(&["missing"]), Err("not found".to_owned()));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
