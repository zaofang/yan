//! Actor supervision on the interpreter (RFC 0056 slice 3: R957, R958).

use super::*;

fn run(src: &str) -> Result<String, yan_interp::Stop> {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(d.is_empty(), "{d:?}");
    let (y, mut d) =
        elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("supported");
    d.extend(yan_check::check("t.yag", src, &y));
    assert!(
        d.iter().all(|d| d.severity != yan_diag::Severity::Error),
        "{d:?}"
    );
    let mut out = Vec::new();
    yan_interp::run(&y, &mut out)?;
    Ok(String::from_utf8(out).expect("utf-8"))
}

const COUNTER: &str = "\
class Counter:
    n: int

def counter(n: int) -> Counter:
    return Counter(n=n)

def add(var self: Counter, by: int) -> int fail str:
    if by < 0:
        fail \"negative\"
    self.n += by
    return self.n
";

#[test]
fn a_scope_binds_a_supervisor() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    with supervisor(actor.one_for_one()):
        c = actor.spawn(Counter(n=0))
        print(c.add(1) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "1\n");
}

#[test]
fn one_for_one_restarts_a_failed_child_from_its_start() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    with supervisor(actor.one_for_one()):
        c = actor.start(ChildSpec(counter, (10,), restart=permanent))
        print(c.add(1) else -1)
        print(c.add(-1) else -1)
        print(c.add(1) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "11\n-1\n11\n");
}

#[test]
fn one_for_all_restarts_every_sibling() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    with supervisor(actor.one_for_all()):
        a = actor.start(ChildSpec(counter, (0,)))
        b = actor.start(ChildSpec(counter, (100,)))
        print(b.add(5) else -1)
        print(a.add(-1) else -1)
        print(b.add(0) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "105\n-1\n100\n");
}

#[test]
fn rest_for_one_restarts_the_children_started_after() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    with supervisor(actor.rest_for_one(intensity=2)):
        a = actor.start(ChildSpec(counter, (0,)))
        b = actor.start(ChildSpec(counter, (100,)))
        print(a.add(5) else -1)
        print(b.add(5) else -1)
        print(b.add(-1) else -1)
        print(a.add(0) else -1)
        print(a.add(-1) else -1)
        print(b.add(0) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "5\n105\n-1\n5\n-1\n100\n");
}

#[test]
fn exceeding_the_intensity_escalates() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    with supervisor(actor.one_for_one(intensity=1)):
        c = actor.start(ChildSpec(counter, (0,)))
        print(c.add(-1) else -1)
        print(c.add(-1) else -1)
"
    );
    match run(&src) {
        Err(yan_interp::Stop::Panic(p)) => assert_eq!(p.kind, "supervisor"),
        r => panic!("{r:?}"),
    }
}

#[test]
fn a_spawned_child_is_temporary() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    c = actor.spawn(Counter(n=0))
    print(c.add(-1) else -1)
    print(c.add(1) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "-1\n-1\n");
}

#[test]
fn a_transient_child_restarts_after_a_failure() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    c = actor.start(ChildSpec(counter, (7,), restart=transient))
    print(c.add(-1) else -1)
    print(c.add(1) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "-1\n8\n");
}

#[test]
fn the_scope_end_stops_its_children() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    c = actor.start(ChildSpec(counter, (0,)))
    with supervisor(actor.one_for_one()):
        d = actor.start(ChildSpec(counter, (0,)))
        c = d
    print(c.add(1) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "-1\n");
}

#[test]
fn a_child_spec_names_a_top_level_function() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    c = actor.start(ChildSpec(Counter, (0,), restart=forever))
"
    );
    let (m, d) = yan_yag::parse("t.yag", &src, false);
    assert!(d.is_empty(), "{d:?}");
    let (_, d) =
        elaborate("t.yag", &src, &m.expect("parses"), Options::default()).expect("elaborates");
    assert!(d.iter().any(|d| d.code == "E201"), "{d:?}");
}
