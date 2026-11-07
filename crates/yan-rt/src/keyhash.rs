//! The canonical encoding of a dict key behind the `key_hash` panic operand (R876, R884).
//!
//! The interpreter and native code both build keys with [`Enc`], so their hashes agree.

/// A key's canonical encoding: a type tag then the value, little endian, lengths as `u64`.
#[derive(Debug, Default, Clone)]
pub struct Enc(Vec<u8>);

impl Enc {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn unit(&mut self) {
        self.0.push(0);
    }

    pub fn bool(&mut self, b: bool) {
        self.0.extend([1, u8::from(b)]);
    }

    pub fn int(&mut self, n: i128) {
        self.0.push(2);
        self.0.extend(n.to_le_bytes());
    }

    /// `-0.0` encodes as `0.0`, so keys that compare equal hash equal.
    pub fn float(&mut self, x: f64) {
        self.0.push(3);
        let x = if x == 0.0 { 0.0 } else { x };
        self.0.extend(x.to_bits().to_le_bytes());
    }

    pub fn char(&mut self, c: char) {
        self.0.push(4);
        self.0.extend(u32::from(c).to_le_bytes());
    }

    pub fn text(&mut self, s: &str) {
        self.0.push(5);
        self.0.extend((s.len() as u64).to_le_bytes());
        self.0.extend(s.as_bytes());
    }

    pub fn bytes(&mut self, b: &[u8]) {
        self.0.push(6);
        self.0.extend((b.len() as u64).to_le_bytes());
        self.0.extend(b);
    }

    /// Nanoseconds of a duration, instant or monotonic reading.
    pub fn nanos(&mut self, n: i128) {
        self.0.push(7);
        self.0.extend(n.to_le_bytes());
    }

    /// A tuple or list of `len` elements; the caller encodes the elements next.
    pub fn seq(&mut self, len: usize) {
        self.0.push(8);
        self.0.extend((len as u64).to_le_bytes());
    }

    /// A record named `name`; the caller encodes its field values next, in declaration order.
    pub fn data(&mut self, name: &str) {
        self.0.push(9);
        self.0.extend((name.len() as u64).to_le_bytes());
        self.0.extend(name.as_bytes());
    }

    /// A key type with no stable encoding; its type name still separates it.
    pub fn opaque(&mut self) {
        self.0.push(255);
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The first 64 bits of BLAKE3 over the encoding, as an unsigned integer.
    pub fn hash(&self) -> u64 {
        hash(&self.0)
    }
}

/// The `key_hash` of an encoded key.
pub fn hash(encoded: &[u8]) -> u64 {
    let h = blake3::hash(encoded);
    u64::from_le_bytes(h.as_bytes()[..8].try_into().expect("8 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_zero_hashes_as_zero() {
        let (mut a, mut b) = (Enc::new(), Enc::new());
        a.float(-0.0);
        b.float(0.0);
        assert_eq!(a.hash(), b.hash());
    }

    #[test]
    fn tag_separates_equal_payloads() {
        let (mut a, mut b) = (Enc::new(), Enc::new());
        a.int(1);
        b.nanos(1);
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn record_encoding_is_fixed() {
        let mut e = Enc::new();
        e.data("P");
        e.int(1);
        e.text("x");
        let mut want = vec![9];
        want.extend(1u64.to_le_bytes());
        want.push(b'P');
        want.push(2);
        want.extend(1i128.to_le_bytes());
        want.push(5);
        want.extend(1u64.to_le_bytes());
        want.push(b'x');
        assert_eq!(e.as_bytes(), want);
    }
}
