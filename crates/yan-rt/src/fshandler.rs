//! `fs` handlers for native code: handler 0 is the real file system; `fake_fs` registers a file
//! map and returns its index (R686).

use crate::abi::{
    List, entries_list, set_fs_reason, str_block, yan_rt_fs_list_dir, yan_rt_fs_read,
};

use crate::map::Map;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static FAKES: RefCell<Vec<HashMap<String, String>>> = const { RefCell::new(Vec::new()) };
}

/// `fake_fs(files)`: the handler of a `dict[str, str]`, which the caller still owns.
///
/// # Safety
/// `files` is a `dict[str, str]` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_fs_fake(files: *const Map) -> u64 {
    // SAFETY: the caller's contract.
    let slots = unsafe { &*files }.slots();
    let map = slots
        .chunks(2)
        // SAFETY: keys and values are `Str` blocks.
        .map(|e| unsafe {
            (
                (*(e[0] as *const String)).clone(),
                (*(e[1] as *const String)).clone(),
            )
        })
        .collect();
    FAKES.with(|f| {
        let mut f = f.borrow_mut();
        f.push(map);
        f.len() as u64
    })
}

/// `fs.read` through handler `h`: a null pointer on failure, with the reason recorded.
///
/// # Safety
/// `path` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_fs_read_with(h: u64, path: *const String) -> *mut String {
    if h == 0 {
        // SAFETY: the caller's contract.
        return unsafe { yan_rt_fs_read(path) };
    }
    // SAFETY: the caller's contract.
    let path = unsafe { &*path };
    match FAKES.with(|f| fake_read(&f.borrow()[h as usize - 1], path)) {
        Ok(text) => str_block(&text),
        Err(reason) => {
            set_fs_reason(reason);
            std::ptr::null_mut()
        }
    }
}

/// `fs.read` on a fake file system.
pub fn fake_read(files: &HashMap<String, String>, path: &str) -> Result<String, String> {
    files
        .get(path)
        .cloned()
        .ok_or_else(|| "not found".to_owned())
}

/// `fs.list_dir` through handler `h`: a null pointer on failure, with the reason recorded.
///
/// # Safety
/// `path` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_fs_list_dir_with(h: u64, path: *const String) -> *mut List {
    if h == 0 {
        // SAFETY: the caller's contract.
        return unsafe { yan_rt_fs_list_dir(path) };
    }
    // SAFETY: the caller's contract.
    let path = unsafe { &*path };
    let names =
        FAKES.with(|f| fake_list(f.borrow()[h as usize - 1].keys().map(String::as_str), path));
    entries_list(path, names)
}

/// `fs.list_dir` on a fake file system, whose directories are the prefixes of its file names: the
/// first component after `dir` of every file beneath it.
pub fn fake_list<'a>(
    files: impl Iterator<Item = &'a str>,
    dir: &str,
) -> Result<Vec<String>, String> {
    let prefix = format!("{}/", dir.strip_suffix('/').unwrap_or(dir));
    let (mut names, mut is_file) = (Vec::new(), false);
    for f in files {
        is_file |= f == dir;
        if let Some(rest) = f.strip_prefix(&prefix) {
            let name = rest.split('/').next().unwrap_or(rest);
            if !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        }
    }
    match (names.is_empty(), is_file) {
        (false, _) => Ok(names),
        (true, true) => Err("not a directory".to_owned()),
        (true, false) => Err("not found".to_owned()),
    }
}
