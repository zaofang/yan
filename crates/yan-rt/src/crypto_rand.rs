//! The OS CSPRNG behind the `crypto_rand` capability (7.1). Unlike `rand`, it has no seed and no
//! stream: every call asks the operating system.

/// `n` bytes from the OS generator; `None` when the OS refuses.
pub fn fill_bytes(n: usize) -> Option<Vec<u8>> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).ok()?;
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_do_not_repeat() {
        let draws: std::collections::HashSet<_> =
            (0..1000).map(|_| fill_bytes(32).unwrap()).collect();
        assert_eq!(draws.len(), 1000);
    }

    #[test]
    fn length_is_exact() {
        assert_eq!(fill_bytes(0).unwrap().len(), 0);
        assert_eq!(fill_bytes(7).unwrap().len(), 7);
    }
}
