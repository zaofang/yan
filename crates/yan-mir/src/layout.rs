//! Default layouts (spec 5.2): size, alignment and field offsets per type. The compiler reorders
//! fields by descending alignment to minimize padding; `Option` uses a niche when its payload
//! has an invalid bit pattern (5.1). Layout attributes (`@layout(..)`) are not modelled yet.

use crate::lower::Shape;
use crate::{IntTy, Ty};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: u64,
    pub align: u64,
    /// Byte offset of each field, in declaration order (records and tuples); empty otherwise.
    pub offsets: Vec<u64>,
    pub sum: Option<SumLayout>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SumLayout {
    /// A tag of `tag_size` bytes at offset 0, then each variant's fields at these offsets.
    Tagged {
        tag_size: u64,
        variants: Vec<Vec<u64>>,
    },
    /// `Option` over a payload with a null niche: `None` is the all-zero payload.
    Niche,
}

fn scalar(t: Ty) -> (u64, u64) {
    match t {
        Ty::Int(IntTy { bits, .. }) => (u64::from(bits) / 8, u64::from(bits) / 8),
        Ty::F64 | Ty::Ptr => (8, 8),
        Ty::Bool => (1, 1),
        Ty::Unit => (0, 1),
    }
}

fn round_up(n: u64, a: u64) -> u64 {
    n.div_ceil(a) * a
}

/// Places `fields` after `start` bytes, largest alignment first; returns offsets in declaration
/// order and the end of the last field.
fn place(start: u64, fields: &[Layout]) -> (Vec<u64>, u64) {
    let mut order: Vec<usize> = (0..fields.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(fields[i].align));
    let mut offsets = vec![0; fields.len()];
    let mut at = start;
    for i in order {
        at = round_up(at, fields[i].align);
        offsets[i] = at;
        at += fields[i].size;
    }
    (offsets, at)
}

fn aggregate(fields: &[Layout]) -> Layout {
    let align = fields.iter().map(|f| f.align).max().unwrap_or(1);
    let (offsets, end) = place(0, fields);
    Layout {
        size: round_up(end, align),
        align,
        offsets,
        sum: None,
    }
}

fn plain(size: u64, align: u64) -> Layout {
    Layout {
        size,
        align,
        offsets: Vec::new(),
        sum: None,
    }
}

/// Whether the type has an invalid all-zero bit pattern that a sum can use as a tag.
fn has_null_niche(s: &Shape) -> bool {
    matches!(
        s,
        Shape::Str
            | Shape::Bytes
            | Shape::ByteArray
            | Shape::BigInt
            | Shape::Path
            | Shape::WinPath
            | Shape::List(_)
    )
}

pub fn layout(s: &Shape) -> Layout {
    match s {
        Shape::Scalar(t) => {
            let (size, align) = scalar(*t);
            plain(size, align)
        }
        Shape::Char => plain(8, 8),
        Shape::Str
        | Shape::Bytes
        | Shape::ByteArray
        | Shape::BigInt
        | Shape::Path
        | Shape::WinPath
        | Shape::List(_)
        | Shape::Map(..)
        | Shape::Heap(_)
        | Shape::Deque(_)
        | Shape::Chan(..)
        | Shape::Actor(_)
        | Shape::Set(_) => plain(8, 8),
        Shape::Fn(..) | Shape::Ptr => plain(16, 8),
        Shape::Tuple(ss) => aggregate(&ss.iter().map(layout).collect::<Vec<_>>()),
        Shape::Rec(_, fs) => aggregate(&fs.iter().map(|(_, s)| layout(s)).collect::<Vec<_>>()),
        Shape::Sum(name, vs) => {
            if name == "Option"
                && let [(_, some), (_, none)] = vs.as_slice()
                && let [(_, t)] = some.as_slice()
                && none.is_empty()
                && has_null_niche(t)
            {
                return Layout {
                    sum: Some(SumLayout::Niche),
                    ..layout(t)
                };
            }
            let tag_size = match vs.len() {
                0..=256 => 1,
                257..=65536 => 2,
                _ => 4,
            };
            let mut align = tag_size;
            let mut end = tag_size;
            let mut variants = Vec::new();
            for (_, fs) in vs {
                let fields: Vec<Layout> = fs.iter().map(|(_, s)| layout(s)).collect();
                let (offsets, e) = place(tag_size, &fields);
                align = align.max(fields.iter().map(|f| f.align).max().unwrap_or(1));
                end = end.max(e);
                variants.push(offsets);
            }
            Layout {
                size: round_up(end, align),
                align,
                offsets: Vec::new(),
                sum: Some(SumLayout::Tagged { tag_size, variants }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn int(bits: u8) -> Shape {
        Shape::Scalar(Ty::Int(IntTy { bits, signed: true }))
    }
    const BOOL: Shape = Shape::Scalar(Ty::Bool);
    const F64: Shape = Shape::Scalar(Ty::F64);

    fn rec(fs: Vec<Shape>) -> Shape {
        Shape::Rec(
            "R".into(),
            fs.into_iter()
                .enumerate()
                .map(|(i, s)| (format!("f{i}"), s))
                .collect(),
        )
    }

    fn sum(name: &str, vs: Vec<Vec<Shape>>) -> Shape {
        Shape::Sum(
            name.into(),
            vs.into_iter()
                .enumerate()
                .map(|(i, fs)| {
                    let fs = fs.into_iter().map(|s| ("x".to_owned(), s)).collect();
                    (format!("V{i}"), fs)
                })
                .collect(),
        )
    }

    fn sa(s: &Shape) -> (u64, u64) {
        let l = layout(s);
        (l.size, l.align)
    }

    #[test]
    fn scalars() {
        assert_eq!(sa(&int(8)), (1, 1));
        assert_eq!(sa(&int(32)), (4, 4));
        assert_eq!(sa(&int(64)), (8, 8));
        assert_eq!(sa(&F64), (8, 8));
        assert_eq!(sa(&BOOL), (1, 1));
        assert_eq!(sa(&Shape::Scalar(Ty::Unit)), (0, 1));
    }

    #[test]
    fn heap_values_are_one_pointer() {
        assert_eq!(sa(&Shape::Str), (8, 8));
        assert_eq!(sa(&Shape::List(Box::new(int(64)))), (8, 8));
        assert_eq!(sa(&Shape::Fn(vec![], Box::new(int(64)))), (16, 8));
    }

    #[test]
    fn record_reorders_fields_to_remove_padding() {
        let r = rec(vec![BOOL, int(64), BOOL, int(32)]);
        let l = layout(&r);
        assert_eq!((l.size, l.align), (16, 8));
        assert_eq!(l.offsets, [12, 0, 13, 8]);
    }

    #[test]
    fn tuple_pads_the_tail_to_its_alignment() {
        let l = layout(&Shape::Tuple(vec![int(32), BOOL]));
        assert_eq!((l.size, l.align), (8, 4));
        assert_eq!(l.offsets, [0, 4]);
    }

    #[test]
    fn empty_record_and_unit_fields_take_no_space() {
        assert_eq!(sa(&rec(vec![])), (0, 1));
        assert_eq!(sa(&rec(vec![Shape::Scalar(Ty::Unit), int(16)])), (2, 2));
    }

    #[test]
    fn nested_record_uses_the_inner_alignment() {
        let inner = rec(vec![int(64), BOOL]);
        assert_eq!(sa(&inner), (16, 8));
        assert_eq!(sa(&rec(vec![BOOL, inner])), (24, 8));
    }

    #[test]
    fn enum_without_fields_is_one_byte() {
        assert_eq!(sa(&sum("E", vec![vec![], vec![], vec![]])), (1, 1));
    }

    #[test]
    fn sum_places_a_tag_then_the_widest_variant() {
        let s = sum("S", vec![vec![int(64)], vec![BOOL, BOOL], vec![]]);
        let l = layout(&s);
        assert_eq!((l.size, l.align), (16, 8));
        assert_eq!(
            l.sum,
            Some(SumLayout::Tagged {
                tag_size: 1,
                variants: vec![vec![8], vec![1, 2], vec![]]
            })
        );
    }

    #[test]
    fn small_payload_packs_beside_the_tag() {
        let l = layout(&sum("S", vec![vec![int(16)], vec![]]));
        assert_eq!((l.size, l.align), (4, 2));
    }

    #[test]
    fn option_of_a_pointer_costs_nothing() {
        let o = sum("Option", vec![vec![Shape::Str], vec![]]);
        let l = layout(&o);
        assert_eq!((l.size, l.align), (8, 8));
        assert_eq!(l.sum, Some(SumLayout::Niche));
    }

    #[test]
    fn option_of_an_integer_needs_a_tag() {
        let o = sum("Option", vec![vec![int(64)], vec![]]);
        assert_eq!(sa(&o), (16, 8));
    }

    #[test]
    fn a_user_sum_named_like_option_with_other_shape_is_tagged() {
        let o = sum("Option", vec![vec![Shape::Str], vec![Shape::Str]]);
        assert_eq!(sa(&o), (16, 8));
    }
}
