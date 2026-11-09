//! Yan Core to Yan Core optimization (22.3, R771). Every pass reads and writes Yan Core; the
//! code generators lower its output through `yan-mir`.

mod drops;
mod mono;
mod owned;

use yan_core::ast::Module;
use yan_diag::Diagnostic;

pub use drops::elaborate as elaborate_drops;
pub use mono::{DEPTH, monomorphize};

#[derive(Debug)]
pub enum Error {
    /// What the pass cannot specialize yet.
    Unsupported(String),
    Diag(Box<Diagnostic>),
}

/// The passes native builds always run (R773): monomorphization, then drop elaboration.
pub fn optimize(file: &str, m: &Module) -> Result<Module, Error> {
    let out = elaborate_drops(&monomorphize(file, m)?)?;
    match yan_check::check_optimized(file, &out).into_iter().next() {
        None => Ok(out),
        Some(d) => Err(Error::Diag(Box::new(d))),
    }
}
