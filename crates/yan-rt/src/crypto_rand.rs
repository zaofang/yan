//! The OS CSPRNG behind the `crypto_rand` capability (7.1). Unlike `rand`, it has no seed and no
//! stream: every call asks the operating system.

/// Fills `buf` from the OS generator; `false` when the OS refuses.
pub fn fill_bytes(buf: &mut [u8]) -> bool {
    getrandom::fill(buf).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(n: usize) -> Vec<u8> {
        let mut b = vec![0u8; n];
        assert!(fill_bytes(&mut b));
        b
    }

    #[test]
    fn draws_do_not_repeat() {
        let draws: std::collections::HashSet<_> = (0..1000).map(|_| draw(32)).collect();
        assert_eq!(draws.len(), 1000);
    }

    #[test]
    fn fills_the_whole_buffer() {
        assert_ne!(draw(64), [0u8; 64]);
    }
}
