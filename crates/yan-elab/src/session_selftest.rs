use super::*;

fn diags(src: &str) -> (Globals, Vec<Diagnostic>) {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(
        d.iter().all(|d| d.severity != yan_diag::Severity::Error),
        "{d:?}"
    );
    let mut m = m.expect("parses");
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
    let e = typefn::expand(&mut m, &mut cx).expect("supported");
    let mut g = Globals {
        applied: e.applied,
        protocols: e.protocols,
        endpoints: e.endpoints,
        broken: e.broken,
        ..Globals::default()
    };
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
        matches!(&client[0], Send(bs) if bs.len() == 1 && bs[0].1 == "Get" && bs[0].2 == Some(j::Type::Named { path: vec!["Str".into()], args: Vec::new() }))
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
    let (_, d) = protocol("choice a { a -> b: X(); b -> c: Px() | a -> b: Y(); b -> c: Qx() }");
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
    let j::Type::Call { path, args } = &g.aliases["B"][0] else {
        panic!("B stays an application")
    };
    assert_eq!(
        typefn::applied(&g, path, args).expect("memoized"),
        Ty::Data("A".into())
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

fn checked(src: &str) -> (y::Module, Vec<Diagnostic>) {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(d.is_empty(), "{d:?}");
    let (y, mut d) =
        elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("supported");
    if d.iter().all(|d| d.severity != yan_diag::Severity::Error) {
        d.extend(yan_check::check("t.yag", src, &y));
    }
    (y, d)
}

fn errors(d: &[Diagnostic]) -> Vec<&str> {
    d.iter()
        .filter(|d| d.severity == yan_diag::Severity::Error)
        .map(|d| d.code.as_str())
        .collect()
}

const FETCH_RUN: &str = "\
def client(take ep: Fetch_client_0, key: str) -> str fail SessionClosed:
    ep1 = session.send(ep, Get(key))?
    got = session.recv(ep1)?
    match got:
        case Found(value=v, next=done):
            session.close(done)
            return f\"found {v}\"
        case Missing(next=gone):
            session.close(gone)
            return \"missing\"

def server(take ep: Fetch_server_0) -> None fail SessionClosed:
    k, ep1 = session.recv(ep)?
    if k == \"a\":
        done = session.send(ep1, Found(42))?
        session.close(done)
    else:
        gone = session.send(ep1, Missing())?
        session.close(gone)

pub def main() uses io:
    eps = session.connect[Fetch]()
    c = eps.client
    s = eps.server
    par concurrent:
        r = client(c, \"a\") else \"closed\"
        server(s) else None
    io.print(r)
";

const FETCH_INT: &str = "client -> server: Get(str); choice server { server -> client: Found(int) | server -> client: Missing() }; end";

#[test]
fn endpoints_carry_a_session_to_its_end() {
    let src = format!("type Fetch = session.protocol(\"{FETCH_INT}\")\n\n{FETCH_RUN}");
    let (m, d) = checked(&src);
    assert!(errors(&d).is_empty(), "{d:?}");
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
    assert_eq!(String::from_utf8(out).expect("utf-8"), "found 42\n");
}

#[test]
fn recursive_three_party_sessions_run() {
    let src = "\
type Count = session.protocol(\"rec Loop { choice a { a -> b: Add(int); a -> c: Tick(); Loop | a -> b: Done(); a -> c: Stop(); b -> c: Total(int) } }\")

def adder(take ep: Count_a_0, i: int, n: int) -> None fail SessionClosed:
    decreases n - i
    if i < n:
        sent = session.send(ep, Add(i))?
        again = session.send(sent, Tick())?
        adder(again, i=i + 1, n=n)?
        return
    done = session.send(ep, Done())?
    last = session.send(done, Stop())?
    session.close(last)

def summer(take ep: Count_b_0, total: int, fuel: int) -> None fail SessionClosed:
    decreases fuel
    got = session.recv(ep)?
    match got:
        case Add(value=v, next=more):
            summer(more, total=total + v, fuel=fuel - 1)?
        case Done(next=fin):
            last = session.send(fin, Total(total))?
            session.close(last)

def reader(take ep: Count_c_0, ticks: int, fuel: int) -> int fail SessionClosed:
    decreases fuel
    got = session.recv(ep)?
    match got:
        case Tick(next=more):
            return reader(more, ticks=ticks + 1, fuel=fuel - 1)?
        case Stop(next=wait):
            t, fin = session.recv(wait)?
            session.close(fin)
            return t * 100 + ticks

pub def main() uses io:
    eps = session.connect[Count]()
    a = eps.a
    b = eps.b
    c = eps.c
    par concurrent:
        adder(a, i=0, n=5) else None
        summer(b, total=0, fuel=100) else None
        t = reader(c, ticks=0, fuel=100) else -1
    io.print(f\"{t}\")
";
    let (m, d) = checked(src);
    assert!(errors(&d).is_empty(), "{d:?}");
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
    assert_eq!(String::from_utf8(out).expect("utf-8"), "1005\n");
}

#[test]
fn an_operation_the_state_does_not_allow_is_e727() {
    let src = format!(
        "type Fetch = session.protocol(\"{FETCH_INT}\")\n\n\
         def wrong_label(take ep: Fetch_client_0) -> None fail SessionClosed:\n    ep1 = session.send(ep, Put(\"x\"))?\n    session.close(ep1)\n\n\
         def early_close(take ep: Fetch_client_0) -> None fail SessionClosed:\n    ep1 = session.send(ep, Get(\"x\"))?\n    session.close(ep1)\n\n\
         def recv_first(take ep: Fetch_client_0) -> None fail SessionClosed:\n    got = session.recv(ep)?\n"
    );
    let (_, d) = checked(&src);
    assert_eq!(errors(&d), ["E727", "E727", "E727"], "{d:?}");
    assert!(
        d[0].message.contains("`Put`") && d[0].message.contains("allows sending `Get`"),
        "{}",
        d[0].message
    );
    assert!(
        d[1].message.contains("`Fetch_client_1`") && d[1].message.contains("`recv` from `server`"),
        "{}",
        d[1].message
    );
}

#[test]
fn dropping_an_endpoint_before_end_is_e412() {
    let src = format!(
        "type Fetch = session.protocol(\"{FETCH_INT}\")\n\n\
         def drop(take ep: Fetch_client_0) -> None fail SessionClosed:\n    ep1 = session.send(ep, Get(\"x\"))?\n    return\n"
    );
    let (_, d) = checked(&src);
    assert_eq!(errors(&d), ["E412"], "{d:?}");
}

#[test]
fn a_label_may_share_a_variant_name() {
    let src = format!(
        "type T = Found(n: int) | Other()\n\ntype Fetch = session.protocol(\"{FETCH_INT}\")\n\n{FETCH_RUN}"
    );
    let (m, d) = checked(&src);
    assert!(errors(&d).is_empty(), "{d:?}");
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
    assert_eq!(String::from_utf8(out).expect("utf-8"), "found 42\n");
}

#[test]
fn a_role_never_taken_out_of_the_endpoints_is_e412() {
    let src = format!(
        "type Fetch = session.protocol(\"{FETCH_INT}\")\n\n\
         pub def main() -> None fail SessionClosed:\n    eps = session.connect[Fetch]()\n    c = eps.client\n    ep1 = session.send(c, Get(\"k\"))?\n    got = session.recv(ep1)?\n    match got:\n        case Found(value=v, next=d):\n            session.close(d)\n        case Missing(next=d):\n            session.close(d)\n"
    );
    let (_, d) = checked(&src);
    assert_eq!(errors(&d), ["E412"], "{d:?}");
    assert!(d.iter().any(|d| d.message.contains("`server`")), "{d:?}");
}
