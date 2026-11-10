//! Child-process confinement from a capability ceiling (RFC 0065 D5, R998).
//!
//! The ceiling is data: the directories a child may read and write and whether it may use the
//! network. `confine` turns it into a Landlock ruleset applied between fork and exec. Where the OS
//! cannot enforce it, `confine` reports `Unavailable` and the caller decides (R998).

use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ceiling {
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
    pub net: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Unavailable(pub &'static str);

/// Restrict `cmd`'s child to `ceiling`, or say why the OS cannot.
pub fn confine(cmd: &mut Command, ceiling: &Ceiling) -> Result<(), Unavailable> {
    imp::confine(cmd, ceiling)
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;
    pub fn confine(_: &mut Command, _: &Ceiling) -> Result<(), Unavailable> {
        Err(Unavailable("no OS confinement on this platform"))
    }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod imp {
    use super::*;
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::CommandExt;

    const CREATE_RULESET: libc::c_long = 444;
    const ADD_RULE: libc::c_long = 445;
    const RESTRICT_SELF: libc::c_long = 446;
    const VERSION_FLAG: libc::c_ulong = 1;
    const RULE_PATH_BENEATH: libc::c_int = 1;

    const EXECUTE: u64 = 1 << 0;
    const WRITE_FILE: u64 = 1 << 1;
    const READ_FILE: u64 = 1 << 2;
    const READ_DIR: u64 = 1 << 3;
    const REFER: u64 = 1 << 13;
    const TRUNCATE: u64 = 1 << 14;
    const FILE_RIGHTS: u64 = EXECUTE | WRITE_FILE | READ_FILE | TRUNCATE;
    const NET_BIND: u64 = 1 << 0;
    const NET_CONNECT: u64 = 1 << 1;

    /// Read-only locations every program needs to start: the loader, libraries and configuration.
    const SYSTEM: &[&str] = &[
        "/usr",
        "/lib",
        "/lib32",
        "/lib64",
        "/bin",
        "/sbin",
        "/etc",
        "/proc",
        "/dev/null",
        "/dev/zero",
        "/dev/urandom",
        "/dev/random",
    ];

    #[repr(C)]
    struct RulesetAttr {
        fs: u64,
        net: u64,
    }

    #[repr(C, packed)]
    struct PathBeneath {
        allowed: u64,
        parent: i32,
    }

    fn abi() -> i64 {
        // SAFETY: a version query takes no pointer.
        unsafe { libc::syscall(CREATE_RULESET, std::ptr::null::<u8>(), 0usize, VERSION_FLAG) }
    }

    fn handled_fs(abi: i64) -> u64 {
        let mut m = (1u64 << 13) - 1;
        if abi >= 2 {
            m |= REFER;
        }
        if abi >= 3 {
            m |= TRUNCATE;
        }
        m
    }

    fn allow(ruleset: &OwnedFd, path: &std::path::Path, access: u64, handled: u64) {
        let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
            return;
        };
        // SAFETY: `c` is a NUL-terminated path; the fd is closed below.
        let raw = unsafe { libc::open(c.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
        if raw < 0 {
            return;
        }
        // SAFETY: `raw` is a fresh descriptor owned here.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: `st` is writable and `fd` is open.
        let is_dir = unsafe { libc::fstat(fd.as_raw_fd(), st.as_mut_ptr()) } == 0
            && unsafe { st.assume_init() }.st_mode & libc::S_IFMT == libc::S_IFDIR;
        let mut access = access & handled;
        if !is_dir {
            access &= FILE_RIGHTS;
        }
        let rule = PathBeneath {
            allowed: access,
            parent: fd.as_raw_fd(),
        };
        // SAFETY: `rule` matches the kernel's `landlock_path_beneath_attr`.
        unsafe {
            libc::syscall(
                ADD_RULE,
                ruleset.as_raw_fd(),
                RULE_PATH_BENEATH,
                &rule as *const PathBeneath,
                0u32,
            );
        }
    }

    pub fn confine(cmd: &mut Command, ceiling: &Ceiling) -> Result<(), Unavailable> {
        let abi = abi();
        if abi < 1 {
            return Err(Unavailable("Landlock is not available (needs Linux 5.13)"));
        }
        let fs = handled_fs(abi);
        let net = if abi >= 4 && !ceiling.net {
            NET_BIND | NET_CONNECT
        } else {
            0
        };
        // Kernels before ABI 4 cannot refuse the network; the ceiling then cannot be enforced.
        if !ceiling.net && abi < 4 {
            return Err(Unavailable("Landlock net rules need Linux 6.7"));
        }
        let attr = RulesetAttr { fs, net };
        let size = if abi >= 4 { 16usize } else { 8 };
        // SAFETY: `attr` matches the kernel's `landlock_ruleset_attr` of `size` bytes.
        let raw = unsafe { libc::syscall(CREATE_RULESET, &attr as *const RulesetAttr, size, 0u32) };
        if raw < 0 {
            return Err(Unavailable("Landlock is disabled"));
        }
        // SAFETY: a fresh descriptor owned here.
        let ruleset = unsafe { OwnedFd::from_raw_fd(raw as i32) };
        let all = fs;
        let readable = READ_FILE | READ_DIR | EXECUTE;
        for p in SYSTEM {
            allow(&ruleset, std::path::Path::new(p), readable, fs);
        }
        for p in &ceiling.read {
            allow(&ruleset, p, readable, fs);
        }
        for p in &ceiling.write {
            allow(&ruleset, p, all, fs);
        }
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                    || libc::syscall(RESTRICT_SELF, ruleset.as_raw_fd(), 0u32) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn run(c: &Ceiling, script: &str) -> Option<bool> {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        confine(&mut cmd, c).ok()?;
        Some(cmd.status().ok()?.success())
    }

    #[test]
    fn child_writes_only_inside_its_ceiling() {
        let dir = std::env::temp_dir().join(format!("yan-sandbox-{}", std::process::id()));
        let (ro, rw) = (dir.join("ro"), dir.join("rw"));
        std::fs::create_dir_all(&ro).unwrap();
        std::fs::create_dir_all(&rw).unwrap();
        let c = Ceiling {
            read: vec![ro.clone()],
            write: vec![rw.clone()],
            net: true,
        };
        let w = |d: &std::path::Path| format!("echo x > {}/f", d.display());
        let (Some(a), Some(b), Some(r)) = (
            run(&c, &w(&rw)),
            run(&c, &w(&ro)),
            run(&c, &format!("ls {}", ro.display())),
        ) else {
            eprintln!("Landlock unavailable; skipped");
            return;
        };
        assert!(a, "write inside the write grant");
        assert!(!b, "write inside a read-only grant");
        assert!(r, "read inside the read grant");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
