use super::*;

fn diags(src: &str) -> (Globals, Vec<Diagnostic>) {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(
        d.iter().all(|d| d.severity != yan_diag::Severity::Error),
        "{d:?}"
    );
    let m = m.expect("parses");
    let mut cx = Cx {
        file: "t.yag",
        src,
        strict: false,
        release: false,
        yn: false,
        ffi: false,
        untrusted: false,
        previews: Vec::new(),
        diags: Vec::new(),
        marks: Vec::new(),
        lits: Vec::new(),
        origins: Vec::new(),
    };
    let mut g = Globals::default();
    stdlib::sums(&mut g, &m).expect("std sums");
    stdlib::classes(&mut g, &m).expect("std classes");
    collect(&mut g, &m, &mut cx).expect("supported");
    (g, cx.diags)
}

fn protocol(spec: &str) -> (Option<session::Protocol>, Vec<Diagnostic>) {
    let (mut g, d) = diags(&format!("type P = session.protocol(\"{spec}\")\n"));
    (g.protocols.remove("P"), d)
}

const FETCH: &str = "client -> server: Get(str); choice server { server -> client: Found(bytes) | server -> client: Missing() }; end";

#[test]
fn a_protocol_projects_onto_each_role() {
    let (p, d) = protocol(FETCH);
    assert!(d.is_empty(), "{d:?}");
    let p = p.expect("a protocol");
    assert_eq!(p.roles, ["client", "server"]);
    use session::State::*;
    let client = &p.states[0];
    assert!(
        matches!(&client[0], Send(bs) if bs.len() == 1 && bs[0].1 == "Get" && bs[0].2 == Some(Ty::Str))
    );
    let Recv(1, bs) = &client[1] else {
        panic!("{client:?}")
    };
    assert_eq!(
        bs.iter().map(|b| b.0.as_str()).collect::<Vec<_>>(),
        ["Found", "Missing"]
    );
    assert_eq!(bs[1].1, None);
    assert_eq!(bs[0].2, bs[1].2, "both branches end in the one end state");
    assert_eq!(client[bs[0].2], End);
    let server = &p.states[1];
    assert!(matches!(&server[0], Recv(0, bs) if bs.len() == 1));
    assert!(matches!(&server[1], Send(bs) if bs.len() == 2));
}

#[test]
fn recursion_loops_back_to_its_state() {
    let (p, d) = protocol("rec X { choice a { a -> b: More(int); X | a -> b: Stop() } }");
    assert!(d.is_empty(), "{d:?}");
    let b = &p.expect("a protocol").states[1];
    let session::State::Recv(0, bs) = &b[0] else {
        panic!("{b:?}")
    };
    assert_eq!(bs[0].2, 0, "`More` returns to the start");
    assert_eq!(b[bs[1].2], session::State::End);
}

#[test]
fn a_role_outside_the_choice_merges_equal_or_receivable_branches() {
    let (_, d) = protocol("choice a { a -> b: X(); b -> c: P() | a -> b: Y(); b -> c: Q() }");
    assert!(d.is_empty(), "the choice reaches `c` through `b`: {d:?}");
    let (_, d) = protocol("choice a { a -> b: X(); a -> c: Z() | a -> b: Y(); a -> c: Z() }");
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn an_unobservable_choice_is_e728_with_both_branches() {
    let (p, d) =
        protocol("a -> b: Go(); choice a { a -> b: X(); b -> c: P() | a -> b: Y(); c -> b: Q() }");
    assert!(p.is_none());
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E728");
    assert!(d[0].message.contains("onto `c`"), "{}", d[0].message);
    let labels: Vec<&str> = d[0].labels.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(labels, ["branch 1", "branch 2"]);
    // The diagnostic sits on `choice a { ... }` inside the literal.
    assert!(d[0].span.col > 26 && d[0].span.end_col > d[0].span.col);
}

#[test]
fn a_choice_its_role_does_not_start_is_e728() {
    let (_, d) = protocol("choice a { a -> b: X() | b -> a: Y() }");
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E728"]
    );
    let (_, d) = protocol("choice a { a -> b: X() | a -> b: X() }");
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E728"]
    );
}

#[test]
fn a_malformed_protocol_is_e270_inside_the_literal() {
    for spec in [
        "a -> b Go()",
        "a -> a: Go()",
        "a -> b: Go(); end; b -> a: Back()",
        "rec X { X }",
        "a -> b: Go(); Y",
        "a -> b: Go(); choice a { a -> b: X() }",
    ] {
        let (p, d) = protocol(spec);
        assert!(p.is_none(), "{spec}");
        assert_eq!(d.len(), 1, "{spec}: {d:?}");
        assert_eq!(d[0].code, "E270", "{spec}: {d:?}");
        assert!(d[0].span.col > 26, "{spec}: {:?}", d[0].span);
    }
}

#[test]
fn equal_applications_are_one_type() {
    let src = format!(
        "type A = session.protocol(\"{FETCH}\")\ntype B = session.protocol(\"{FETCH}\")\ntype C = session.protocol(\"a -> b: Go()\")\n"
    );
    let (g, d) = diags(&src);
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(g.protocols.len(), 2, "A and B share one protocol");
    let ty = |n: &str| g.aliases[n][0].clone();
    let (j::Type::Call { args: a, .. }, j::Type::Call { args: b, .. }) = (ty("A"), ty("B")) else {
        panic!("calls")
    };
    assert_eq!(
        typefn::applied(&g, &["session".into(), "protocol".into()], &a).expect("memoized"),
        typefn::applied(&g, &["session".into(), "protocol".into()], &b).expect("memoized"),
    );
}

#[test]
fn a_protocol_type_elaborates_and_checks() {
    let src = format!(
        "type Fetch = session.protocol(\"{FETCH}\")\n\npub def main() uses io:\n    io.print(\"ok\")\n"
    );
    let (m, d) = yan_yag::parse("t.yag", &src, false);
    assert!(d.is_empty(), "{d:?}");
    let (y, d) =
        elaborate("t.yag", &src, &m.expect("parses"), Options::default()).expect("supported");
    assert!(d.is_empty(), "{d:?}");
    let checked = yan_check::check("t.yag", &src, &y);
    assert!(checked.is_empty(), "{checked:?}");
}
