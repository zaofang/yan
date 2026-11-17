//! The closure check at resolution and for a locked build (RFC 0079 D3); the check itself is
//! `yan_db::closure`, shared with the compiler's graph diagnostics.

use yan_db::closure::{Node, violations};
pub(crate) use yan_db::closure::{deps_of, entry, from_package, reach};
use yan_diag::Diagnostic;

use crate::lock::fail;

pub(crate) fn check(nodes: &[Node]) -> Result<(), Box<Diagnostic>> {
    match violations(nodes).into_iter().next() {
        Some((_, message)) => Err(fail("E314", message)),
        None => Ok(()),
    }
}
