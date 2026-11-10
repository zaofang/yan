//! The `Wire` round trip of a call through `Remote` (R402) over native slots: the value is read
//! from its slots, encoded and decoded by [`crate::wire`] under the default limits, and the
//! decoded value is laid out again, as `yan-interp/src/cluster.rs` does over `Value`s. The slot
//! layout is `yan-mir`'s, as in [`crate::jsonslots`].

use crate::jsonslots::Slot;
use crate::wire::{Def, Limits, Schema, Schemas, Val, WireError, decode, encode};

fn fields<'a>(sc: &'a Schemas, n: &str) -> &'a Def {
    &sc.defs[n]
}

/// The slots of a value of type `t`.
fn width(sc: &Schemas, t: &Schema) -> usize {
    let fs = |fs: &[(String, Schema)]| fs.iter().map(|(_, t)| width(sc, t)).sum::<usize>();
    match t {
        Schema::Option(e) => 1 + width(sc, e),
        Schema::Tuple(ts) => ts.iter().map(|t| width(sc, t)).sum(),
        Schema::Named(n) => match fields(sc, n) {
            Def::Record(r) => fs(r),
            Def::Sum(vs) => 1 + vs.iter().map(|(_, f)| fs(f)).sum::<usize>(),
        },
        _ => 1,
    }
}

/// How `crate::map` encodes a key of type `t` (`yan-mir`'s `key_schema`).
fn key_schema(sc: &Schemas, t: &Schema, out: &mut String) {
    let fs = |fs: &[(String, Schema)], out: &mut String| {
        fs.iter().for_each(|(_, t)| key_schema(sc, t, out));
    };
    match t {
        Schema::Unit => out.push('n'),
        Schema::Bool => out.push('b'),
        Schema::Int => out.push_str("i64"),
        Schema::Float => out.push('f'),
        Schema::Str => out.push('s'),
        Schema::Tuple(ts) => {
            out.push('(');
            ts.iter().for_each(|t| key_schema(sc, t, out));
            out.push(')');
        }
        Schema::Option(e) => {
            out.push_str("<Some|");
            key_schema(sc, e, out);
            out.push_str(";None|;>");
        }
        Schema::Named(n) => match fields(sc, n) {
            Def::Record(r) => {
                out.push('{');
                out.push_str(n);
                out.push('|');
                fs(r, out);
                out.push('}');
            }
            Def::Sum(vs) => {
                out.push('<');
                for (v, f) in vs {
                    out.push_str(v);
                    out.push('|');
                    fs(f, out);
                    out.push(';');
                }
                out.push('>');
            }
        },
        _ => unreachable!("{t:?} is not a native dict key"),
    }
}

/// The codec's value of type `t` held in `w`, exactly its slots.
///
/// # Safety
/// The `Str`, `List` and `Map` slots in `w` point to live blocks.
unsafe fn read(sc: &Schemas, t: &Schema, w: &[u64]) -> Val {
    // SAFETY: each part is a value of its type, under the caller's contract.
    let parts = |ts: &mut dyn Iterator<Item = &Schema>, w: &[u64]| {
        let mut at = 0;
        ts.map(|t| {
            let n = width(sc, t);
            let v = unsafe { read(sc, t, &w[at..at + n]) };
            at += n;
            v
        })
        .collect::<Vec<_>>()
    };
    match t {
        Schema::Unit => Val::Unit,
        Schema::Bool => Val::Bool(w[0] & 0xff != 0),
        Schema::Int => Val::Int(w[0] as i64),
        Schema::Float => Val::Float(f64::from_bits(w[0])),
        // SAFETY: the caller's contract.
        Schema::Str => Val::Str(unsafe { &*(w[0] as *const String) }.clone()),
        Schema::List(e) => {
            // SAFETY: the caller's contract.
            let l = unsafe { &*(w[0] as *const crate::abi::List) };
            let n = width(sc, e);
            if n == 0 {
                return Val::List(Vec::new());
            }
            Val::List(
                l.data
                    .chunks(n)
                    .map(|x| unsafe { read(sc, e, x) })
                    .collect(),
            )
        }
        Schema::Dict(k, e) => {
            // SAFETY: the caller's contract.
            let m = unsafe { &*(w[0] as *const crate::map::Map) };
            let kw = width(sc, k);
            let vw = width(sc, e);
            Val::Dict(
                m.entries()
                    // SAFETY: an entry is a key of `k` then a value of `e`.
                    .map(|x| unsafe { (read(sc, k, &x[..kw]), read(sc, e, &x[kw..kw + vw])) })
                    .collect(),
            )
        }
        // `Some` is tag 0 and `None` tag 1, as the native `Option` sum.
        Schema::Option(_) if w[0] == 1 => Val::Option(None),
        // SAFETY: forwarded.
        Schema::Option(e) => Val::Option(Some(Box::new(unsafe { read(sc, e, &w[1..]) }))),
        Schema::Tuple(ts) => Val::Tuple(parts(&mut ts.iter(), w)),
        Schema::Named(n) => match fields(sc, n) {
            Def::Record(r) => Val::Record(parts(&mut r.iter().map(|f| &f.1), w)),
            Def::Sum(vs) => {
                let k = w[0] as usize;
                let at = 1 + vs[..k]
                    .iter()
                    .flat_map(|(_, f)| f)
                    .map(|(_, t)| width(sc, t))
                    .sum::<usize>();
                Val::Variant(k, parts(&mut vs[k].1.iter().map(|f| &f.1), &w[at..]))
            }
        },
        Schema::Char | Schema::Bytes | Schema::Set(_) => unreachable!("no native slots for {t:?}"),
    }
}

/// The slots of `v`, a value of type `t`.
fn write(sc: &Schemas, t: &Schema, v: Val, out: &mut Vec<Slot>) {
    let zeros = |n: usize, out: &mut Vec<Slot>| out.extend((0..n).map(|_| Slot::Word(0)));
    match (t, v) {
        (_, Val::Unit) => out.push(Slot::Word(0)),
        (_, Val::Bool(b)) => out.push(Slot::Word(u64::from(b))),
        (_, Val::Int(i)) => out.push(Slot::Word(i as u64)),
        (_, Val::Float(f)) => out.push(Slot::Word(f.to_bits())),
        (_, Val::Str(s)) => out.push(Slot::Str(s)),
        (Schema::List(e), Val::List(xs)) => {
            let mut slots = Vec::new();
            xs.into_iter().for_each(|x| write(sc, e, x, &mut slots));
            out.push(Slot::List(width(sc, e), slots));
        }
        (Schema::Dict(k, e), Val::Dict(xs)) => {
            let mut schema = String::new();
            key_schema(sc, k, &mut schema);
            let entries = xs
                .into_iter()
                .map(|(a, b)| {
                    let (mut ka, mut vb) = (Vec::new(), Vec::new());
                    write(sc, k, a, &mut ka);
                    write(sc, e, b, &mut vb);
                    (ka, vb)
                })
                .collect();
            out.push(Slot::Map {
                schema,
                kw: width(sc, k),
                vw: width(sc, e),
                entries,
            });
        }
        (Schema::Option(e), Val::Option(None)) => {
            out.push(Slot::Word(1));
            zeros(width(sc, e), out);
        }
        (Schema::Option(e), Val::Option(Some(x))) => {
            out.push(Slot::Word(0));
            write(sc, e, *x, out);
        }
        (Schema::Tuple(ts), Val::Tuple(xs)) => {
            ts.iter().zip(xs).for_each(|(t, x)| write(sc, t, x, out));
        }
        (Schema::Named(n), Val::Record(xs)) => {
            let Def::Record(r) = fields(sc, n) else {
                unreachable!("a record")
            };
            r.iter()
                .zip(xs)
                .for_each(|((_, t), x)| write(sc, t, x, out));
        }
        (Schema::Named(n), Val::Variant(k, xs)) => {
            let Def::Sum(vs) = fields(sc, n) else {
                unreachable!("a sum")
            };
            let span = |vs: &[(String, Vec<(String, Schema)>)]| {
                vs.iter()
                    .flat_map(|(_, f)| f)
                    .map(|(_, t)| width(sc, t))
                    .sum::<usize>()
            };
            out.push(Slot::Word(k as u64));
            zeros(span(&vs[..k]), out);
            vs[k]
                .1
                .iter()
                .zip(xs)
                .for_each(|((_, t), x)| write(sc, t, x, out));
            zeros(span(&vs[k + 1..]), out);
        }
        (t, v) => unreachable!("{v:?} does not lay out as {t:?}"),
    }
}

/// The slots of the value in `w` after the codec's round trip, and the codec's failure if
/// it rejects the value. A rejected value is laid out again unencoded, so the slots always
/// hold a value the program owns.
///
/// # Safety
/// `w` holds a value of the schema's root type; its `Str`, `List` and `Map` slots point to live
/// blocks.
pub unsafe fn roundtrip(sc: &Schemas, w: &[u64]) -> (Vec<Slot>, Option<WireError>) {
    let root = Schema::Named(sc.root.clone());
    // SAFETY: the caller's contract.
    let v = unsafe { read(sc, &root, w) };
    let (v, why) = match encode(sc, &v).and_then(|b| decode(sc, &b, Limits::default())) {
        Ok(back) => (back, None),
        Err(e) => (v, Some(e)),
    };
    let mut out = Vec::new();
    write(sc, &root, v, &mut out);
    (out, why)
}

/// The slot count of the schema's root type.
pub fn root_width(sc: &Schemas) -> usize {
    width(sc, &Schema::Named(sc.root.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schemas(json: &str) -> Schemas {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_record_with_a_sum_and_an_option_round_trips() {
        let sc = schemas(
            r#"{"root":"$","defs":{
                "$":{"Record":[["x",{"Named":"P"}]]},
                "P":{"Record":[["a","Int"],["b",{"Option":"Bool"}],["c",{"Named":"S"}]]},
                "S":{"Sum":[["A",[["n","Int"]]],["B",[["f","Float"],["g","Int"]]]]}}}"#,
        );
        let w = [7, 0, 1, 1, 0, 2.5f64.to_bits(), (-3i64) as u64];
        assert_eq!(root_width(&sc), w.len());
        // SAFETY: no pointer slots.
        let (slots, why) = unsafe { roundtrip(&sc, &w) };
        assert_eq!(why, None);
        let back: Vec<u64> = slots
            .into_iter()
            .map(|s| match s {
                Slot::Word(x) => x,
                s => panic!("{s:?}"),
            })
            .collect();
        assert_eq!(back, w);
    }

    #[test]
    fn a_str_over_the_length_limit_is_rejected_and_laid_out_again() {
        let sc = schemas(r#"{"root":"$","defs":{"$":{"Record":[["s","Str"]]}}}"#);
        let long = "x".repeat(Limits::default().len + 1);
        let s = Box::new(long.clone());
        let w = [&*s as *const String as u64];
        // SAFETY: the slot points to a live `String`.
        let (slots, why) = unsafe { roundtrip(&sc, &w) };
        assert_eq!(why, Some(WireError::Size));
        assert_eq!(slots, vec![Slot::Str(long)]);
    }

    #[test]
    fn a_dict_of_lists_is_copied_into_fresh_blocks() {
        use crate::abi::{List, leak, str_block, yan_rt_wire_roundtrip};
        let json =
            r#"{"root":"$","defs":{"$":{"Record":[["d",{"Dict":["Str",{"List":"Int"}]}]]}}}"#;
        let sc = schemas(json);
        let m = crate::map::Map::new("s", 1, 1);
        let xs = leak(List {
            words: 1,
            data: vec![1, (-2i64) as u64],
            limit: -1,
        });
        // SAFETY: the key is one live `Str` slot; the value slot is written at once.
        unsafe {
            let k = [str_block("a") as u64];
            crate::map::yan_rt_map_push(m, k.as_ptr()).write(xs as u64);
        }
        let w = [m as u64];
        let mut out = [0u64];
        let schema = str_block(json);
        // SAFETY: `w` is a live dict of the root type; `out` has its one slot.
        let st = unsafe { yan_rt_wire_roundtrip(out.as_mut_ptr(), w.as_ptr(), schema, 0) };
        assert_eq!(st, 0);
        assert_ne!(out[0], w[0]);
        let root = Schema::Named(sc.root.clone());
        // SAFETY: both hold live dicts of the root type.
        let (a, b) = unsafe { (read(&sc, &root, &w), read(&sc, &root, &out)) };
        assert_eq!(a, b);
    }
}
