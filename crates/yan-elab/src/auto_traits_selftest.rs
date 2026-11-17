use super::*;

fn codes(src: &str) -> Vec<String> {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(d.is_empty(), "{d:?}");
    let (_, d) =
        elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("elaborates");
    d.into_iter()
        .filter(|d| d.severity == yan_diag::Severity::Error && d.code != "E323")
        .map(|d| d.code)
        .collect()
}

#[test]
fn a_mutex_of_a_non_send_value_is_not_send() {
    let src = "def f(var tx: Sender[Mutex[Rc[int]]], m: Mutex[Rc[int]]) -> None fail Cancelled:
    tx.send(m)?
";
    assert_eq!(codes(src), ["E415"]);
}

#[test]
fn a_mutex_of_a_send_value_is_send() {
    let src = "def f(var tx: Sender[Mutex[int]], m: Mutex[int]) -> None fail Cancelled:
    tx.send(m)?
";
    assert!(codes(src).is_empty());
}
