//! Local actors on the interpreter (RFC 0056 slices 1 and 2: R956, R957, R959, R813).

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
    print(c.inc(1) else -1)
    par concurrent:
        print(c.inc(10) else -1)
        print(c.inc(100) else -1)
    print(c.inc(0) else -1)
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

def ping(var self: A, other: Actor[A]) -> int fail ActorError:
    self.n += 1
    return other.ping(other)?

def total(var self: A) -> int:
    return self.n

def show(r: Result[int, ActorError]) -> str:
    match r:
        case Ok(v):
            return f\"{v}\"
        case Err(e):
            return f\"{e}\"

def main() uses io:
    a = actor.spawn(A(n=0))
    print(show(a.ping(a)))
    print(a.total() else -1)
";
    assert_eq!(run(src).expect("runs"), "Deadlock()\n1\n");
}

#[test]
fn spawn_needs_a_positive_capacity() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    c = actor.spawn(Counter(n=0), capacity=0)
    print(c.inc(1) else -1)
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

/// The codes of the errors elaborating `src` reports.
fn codes(src: &str) -> Vec<String> {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(d.is_empty(), "{d:?}");
    let (_, d) =
        elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("elaborates");
    d.into_iter()
        .filter(|d| d.severity == yan_diag::Severity::Error)
        .map(|d| d.code)
        .collect()
}

/// Whether elaboration reports `src` as not supported yet (E017), never silently accepts it.
fn unsupported(src: &str) -> bool {
    codes(src).iter().any(|c| c == "E017")
}

#[test]
fn a_call_runs_the_messages_sent_before_it_in_order() {
    let src = format!(
        "{LOUD}
def main() uses io:
    a = actor.spawn(Loud(n=0))
    actor.send(a).add(1)
    actor.send(a).add(2)
    print(a.total() else -1)
    actor.send(a).add(4)
    print(a.total() else -1)
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
    print(a.total() else -1)
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
fn only_unit_handlers_without_failures_can_be_sent() {
    let src = format!(
        "{LOUD}
def main() uses io:
    a = actor.spawn(Loud(n=0))
    actor.send(a).total()
"
    );
    assert_eq!(codes(&src), ["E729"]);
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
    print(a.get() else -1)
";
    assert!(unsupported(src));
}

#[test]
fn the_youngest_caller_of_a_wait_cycle_fails_deadlock() {
    let src = "\
class A:
    n: int

def get(var self: A) -> int:
    return self.n

def wait_then(var self: A, other: Actor[A], var rx: Receiver[int]) -> int fail ActorError:
    rx.recv()?
    return other.get()? + 1

def signal_then(var self: A, other: Actor[A], var tx: Sender[int]) -> int fail ActorError:
    tx.send(1)?
    return other.get()? + 2

def show(r: Result[int, ActorError]) -> str:
    match r:
        case Ok(v):
            return f\"{v}\"
        case Err(e):
            return f\"{e}\"

def main() uses io:
    a = actor.spawn(A(n=10))
    b = actor.spawn(A(n=20))
    tx, rx = Channel[int](1)
    par concurrent:
        print(show(a.wait_then(b, &rx)))
        print(show(b.signal_then(a, &tx)))
";
    assert_eq!(run(src).expect("runs"), "Deadlock()\n21\n");
}

#[test]
fn a_call_to_a_busy_actor_times_out_at_its_deadline() {
    let src = "\
class A:
    n: int

def get(var self: A) -> int:
    return self.n

def hold(var self: A, var rx: Receiver[int]) -> int fail Cancelled:
    return rx.recv()?

def ask(a: Actor[A], var go: Receiver[int]) -> int fail ActorError uses time:
    go.recv()?
    with timeout(5 * ms):
        return a.get()?
    return 0

def show(r: Result[int, ActorError]) -> str:
    match r:
        case Ok(v):
            return f\"{v}\"
        case Err(e):
            return f\"{e}\"

def main() uses io, time:
    a = actor.spawn(A(n=0))
    tx, rx = Channel[int](1)
    go_tx, go_rx = Channel[int](1)
    par concurrent:
        for _ in range(1):
            print(show(ask(a, &go_rx)))
            tx.send(7) else e -> None
        print(a.hold(&rx) else e -> -1)
        go_tx.send(1) else e -> None
";
    assert_eq!(run(src).expect("runs"), "Timeout()\n7\n");
}

#[test]
fn an_idle_actor_runs_its_mail_when_every_task_waits() {
    let src = "\
class Relay:
    n: int

def ping(var self: Relay, take out: Sender[int], v: int):
    tx = out
    tx.send(v * 2) else e -> None

def main() uses io:
    tx, rx = Channel[int](1)
    a = actor.spawn(Relay(n=0))
    actor.send(a).ping(tx, 5)
    par concurrent:
        print(rx.recv() else e -> -1)
        print(1)
";
    assert_eq!(run(src).expect("runs"), "1\n10\n");
}
