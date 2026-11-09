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

const LOUD: &str = "\
class Loud:
    n: int

def add(var self: Loud, by: int):
    self.n += by

def shout(var self: Loud, k: int) uses io:
    print(k)

def total(var self: Loud) -> int:
    return self.n
";

/// Whether elaboration reports `src` as not supported yet (E017), never silently accepts it.
fn unsupported(src: &str) -> bool {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(d.is_empty(), "{d:?}");
    let (_, d) =
        elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("elaborates");
    d.iter().any(|d| d.code == "E017")
}

#[test]
fn a_call_runs_the_messages_sent_before_it_in_order() {
    let src = format!(
        "{LOUD}
def main() uses io:
    a = actor.spawn(Loud(n=0))
    actor.send(a).add(1)
    actor.send(a).add(2)
    print(a.total())
    actor.send(a).add(4)
    print(a.total())
"
    );
    assert_eq!(run(&src).expect("runs"), "3\n7\n");
}

#[test]
fn a_full_mailbox_runs_its_oldest_message_before_admitting() {
    let src = format!(
        "{LOUD}
def main() uses io:
    a = actor.spawn(Loud(n=0), capacity=1)
    actor.send(a).add(1)
    actor.send(a).add(2)
    actor.send(a).add(4)
    print(a.total())
"
    );
    assert_eq!(run(&src).expect("runs"), "7\n");
}

#[test]
fn a_handler_with_uses_cannot_be_sent_yet() {
    let src = format!(
        "{LOUD}
def main() uses io:
    a = actor.spawn(Loud(n=0))
    actor.send(a).shout(1)
"
    );
    assert!(unsupported(&src));
}

#[test]
fn only_unit_handlers_can_be_sent() {
    let src = format!(
        "{LOUD}
def main() uses io:
    a = actor.spawn(Loud(n=0))
    actor.send(a).total()
"
    );
    assert!(unsupported(&src));
}

#[test]
fn a_reentrant_handler_call_is_unsupported() {
    let src = "\
class A:
    n: int

@reentrant
def get(var self: A) -> int:
    return self.n

def main() uses io:
    a = actor.spawn(A(n=0))
    print(a.get())
";
    assert!(unsupported(src));
}
