
use crate::check_in;

fn codes(src: &str, release: bool) -> Vec<(String, u32)> {
    let src = format!("module t\n{src}");
    let (m, d) = yan_core::parse("t.yir", &src);
    assert!(d.is_empty());
    check_in("t.yir", &src, &m.unwrap(), release)
        .into_iter()
        .map(|d| (d.code, d.span.line))
        .collect()
}

const VIA_HELPER: &str = "fn probe(cap toolchain: Toolchain) -> i64 {\n    ret 0\n}\n\
        fn mid(cap toolchain: Toolchain) -> i64 {\n    let n: i64 = probe(cap toolchain)\n    ret n\n}\n\
        fn main(cap toolchain: Toolchain) -> i64 {\n    let n: i64 = mid(cap toolchain)\n    ret n\n}\n";

#[test]
fn release_reaches_toolchain_through_calls() {
    let found = codes(VIA_HELPER, true);
    assert_eq!(found.len(), 3);
    assert!(found.iter().all(|(c, _)| c == "E318"));
}

#[test]
fn debug_build_binds_toolchain() {
    assert_eq!(codes(VIA_HELPER, false), []);
}

#[test]
fn an_unreached_use_is_not_a_release_binding() {
    let src = "fn probe(cap toolchain: Toolchain) -> i64 {\n    ret 0\n}\nfn main() -> i64 {\n    ret 0\n}\n";
    assert_eq!(codes(src, true), []);
}
