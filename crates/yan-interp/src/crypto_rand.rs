//! std.crypto_rand: the OS generator and the `fake_crypto_rand` test handler (7.1). A release run
//! ignores a fake; only `yan test` honours it.

use std::rc::Rc;

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
            [h, Value::Int(n)] => fill_bytes(it.test, h, *n),
            _ => bad_args(STD_CRYPTO_RAND_FILL_BYTES),
        }),
    ),
];

fn fill_bytes(test: bool, h: &Value, n: i128) -> R<Value> {
    let Ok(n) = usize::try_from(n) else {
        return panic("value");
    };
    if test
        && let Value::Data { name, fields } = h
        && name == "FakeCryptoRand"
        && let Value::Bytes(data) = &fields[0].1
    {
        return Ok(Value::Bytes(
            data.iter().copied().cycle().take(n).collect::<Rc<[u8]>>(),
        ));
    }
    match yan_rt::crypto_rand::fill_bytes(n) {
        Some(b) => Ok(Value::Bytes(b.into())),
        None => panic("crypto_rand"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake() -> Value {
        Value::Data {
            name: "FakeCryptoRand".into(),
            fields: vec![("data".into(), Value::Bytes(Rc::from([1u8, 2, 3])))],
        }
    }

    fn draw(test: bool, h: &Value, n: i128) -> Vec<u8> {
        match fill_bytes(test, h, n) {
            Ok(Value::Bytes(b)) => b.to_vec(),
            _ => panic!("bytes"),
        }
    }

    #[test]
    fn a_test_run_honours_the_fake() {
        assert_eq!(draw(true, &fake(), 7), [1, 2, 3, 1, 2, 3, 1]);
    }

    #[test]
    fn a_release_run_ignores_the_fake() {
        let a = draw(false, &fake(), 32);
        assert_eq!(a.len(), 32);
        assert_ne!(a, draw(false, &fake(), 32));
    }

    #[test]
    fn a_negative_length_panics() {
        assert!(fill_bytes(false, &Value::Cap("crypto_rand".into()), -1).is_err());
    }
}
