//! `fs` handlers for native code: handler 0 is the real file system; `fake_fs` registers a file
//! map and returns its index (R686).

use crate::abi::{set_fs_reason, str_block, yan_rt_fs_read};
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
    FAKES.with(|f| match f.borrow()[h as usize - 1].get(path) {
        Some(text) => str_block(text),
        None => {
            set_fs_reason("not found".to_owned());
            std::ptr::null_mut()
        }
    })
}
