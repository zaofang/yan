//! std.crypto_rand: the OS generator and the `fake_crypto_rand` test handler (7.1). A release run
//! ignores a fake; only `yan test` honours it.

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::{R, Value, panic};

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_TEST_FAKE_CRYPTO_RAND,
        Bind::Plain(|a| match a {
            [data @ Value::Bytes(_)] => Ok(Value::Data {
                name: "FakeCryptoRand".into(),
                fields: vec![("data".into(), data.clone())],
            }),
            _ => bad_args(STD_TEST_FAKE_CRYPTO_RAND),
        }),
    ),
    (
        STD_CRYPTO_RAND_FILL_BYTES,
        Bind::Cap(|it, a| match a {
            [h, Value::ByteArray(buf)] => fill_bytes(it.test, h, buf),
            _ => bad_args(STD_CRYPTO_RAND_FILL_BYTES),
        }),
    ),
    (
        STD_CRYPTO_RAND_RANDOM_BYTES,
        Bind::Cap(|it, a| match a {
            [h, Value::Int(n)] => random_bytes(it.test, h, *n),
            _ => bad_args(STD_CRYPTO_RAND_RANDOM_BYTES),
        }),
    ),
];

/// `fill_bytes` on a fresh buffer of `n` bytes.
fn random_bytes(test: bool, h: &Value, n: i128) -> R<Value> {
    let Ok(n) = usize::try_from(n) else {
        return panic("value");
    };
    let mut buf = vec![0u8; n];
    fill_bytes(test, h, &mut buf)?;
    Ok(Value::Bytes(buf.into()))
}

fn fill_bytes(test: bool, h: &Value, buf: &mut [u8]) -> R<Value> {
    if test
        && let Value::Data { name, fields } = h
        && name == "FakeCryptoRand"
        && let Value::Bytes(data) = &fields[0].1
    {
        for (d, s) in buf.iter_mut().zip(data.iter().cycle()) {
            *d = *s;
        }
        return Ok(Value::Unit);
    }
    if yan_rt::crypto_rand::fill_bytes(buf) {
        Ok(Value::Unit)
    } else {
        panic("crypto_rand")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn fake() -> Value {
        Value::Data {
            name: "FakeCryptoRand".into(),
            fields: vec![("data".into(), Value::Bytes(Rc::from([1u8, 2, 3])))],
        }
    }

    fn draw(test: bool, h: &Value, n: usize) -> Vec<u8> {
        let mut buf = vec![0u8; n];
        fill_bytes(test, h, &mut buf).unwrap();
        buf
    }

    #[test]
    fn a_negative_length_panics() {
        assert!(random_bytes(false, &Value::Cap("crypto_rand".into()), -1).is_err());
    }

    #[test]
    fn random_bytes_has_the_asked_length() {
        let Ok(Value::Bytes(b)) = random_bytes(true, &fake(), 5) else {
            panic!("bytes")
        };
        assert_eq!(&*b, [1, 2, 3, 1, 2]);
    }

    #[test]
    fn a_test_run_honours_the_fake() {
        assert_eq!(draw(true, &fake(), 7), [1, 2, 3, 1, 2, 3, 1]);
    }

    #[test]
    fn a_release_run_ignores_the_fake() {
        let a = draw(false, &fake(), 32);
        assert_ne!(a, draw(false, &fake(), 32));
    }
}
