//! Local actors on the interpreter (RFC 0056 slice 1: R956, R957, R959).

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

def inc(var self: Counter, by: int) -> int:
    self.n += by
    return self.n

def checked(var self: Counter, by: int) -> int fail str:
    if by < 0:
        fail \"negative\"
    self.n += by
    return self.n
";

#[test]
fn a_counter_actor_answers_main_and_two_tasks() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    c = actor.spawn(Counter(n=0))
    print(c.inc(1))
    par concurrent:
        print(c.inc(10))
        print(c.inc(100))
    print(c.inc(0))
"
    );
    assert_eq!(run(&src).expect("runs"), "1\n11\n111\n111\n");
}

#[test]
fn a_failing_handler_keeps_its_actor() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    c = actor.spawn(Counter(n=0), capacity=4)
    print(c.checked(-1) else -1)
    print(c.checked(5) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "-1\n5\n");
}

#[test]
fn a_call_back_into_a_busy_actor_deadlocks() {
    let src = "\
class A:
    n: int

def ping(var self: A, other: Actor[A]) -> int:
    return other.ping(other)

def main() uses io:
    a = actor.spawn(A(n=0))
    print(a.ping(a))
";
    assert!(matches!(run(src), Err(yan_interp::Stop::Panic(..))));
}

#[test]
fn spawn_needs_a_positive_capacity() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    c = actor.spawn(Counter(n=0), capacity=0)
    print(c.inc(1))
"
    );
    assert!(matches!(run(&src), Err(yan_interp::Stop::Panic(..))));
}
