//! The target description (RFC 0083 D4): one data file the compiler reads.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

const HOST: &str = include_str!("../target/host.toml");

#[derive(Debug, Deserialize)]
pub struct Target {
    pub name: String,
    pub endian: String,
    pub pointer_size: u32,
    pub pointer_align: u32,
    pub addr_bits: u32,
    /// C type name to the Yan scalar it stands for.
    pub c: BTreeMap<String, String>,
}

impl Target {
    /// Parses a description and rejects a target Yan 0.1.0 does not support.
    pub fn parse(src: &str) -> Result<Target, String> {
        let t: Target = toml::from_str(src).map_err(|e| format!("target description: {e}"))?;
        if t.endian != "little" {
            return Err(format!(
                "target `{}` is {}-endian; Yan 0.1.0 targets are little-endian",
                t.name, t.endian
            ));
        }
        Ok(t)
    }

    /// The host entry.
    pub fn host() -> &'static Target {
        static HOST_TARGET: OnceLock<Target> = OnceLock::new();
        HOST_TARGET.get_or_init(|| Target::parse(HOST).expect("host target description"))
    }

    /// The Yan scalar a C type of `core.ffi` stands for.
    pub fn c_scalar(&self, name: &str) -> Option<&str> {
        self.c.get(name).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_entry_parses() {
        let t = Target::host();
        assert_eq!((t.pointer_size, t.addr_bits), (8, 64));
        assert_eq!(t.c_scalar("c_long"), Some("int"));
        assert_eq!(t.c_scalar("nope"), None);
    }

    #[test]
    fn big_endian_is_refused() {
        let src = HOST.replace("endian = \"little\"", "endian = \"big\"");
        assert!(Target::parse(&src).unwrap_err().contains("little-endian"));
    }
}
