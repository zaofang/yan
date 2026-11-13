//! Local actors on the interpreter (RFC 0056 slices 1 and 2: R956, R957, R959, R813; RFC 0059:
//! R974 to R977).

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
fn a_failing_handler_reaches_its_caller() {
    let src = format!(
        "{COUNTER}
def main() uses io:
    c = actor.spawn(Counter(n=0), capacity=4)
    print(c.checked(5) else -1)
    print(c.checked(-1) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "5\n-1\n");
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
    // The failed `ping` stops `a` (RFC 0060 D3).
    assert_eq!(run(src).expect("runs"), "Deadlock()\n-1\n");
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
    actor.send(a).add(1) else e -> None
    actor.send(a).add(2) else e -> None
    print(a.total() else -1)
    actor.send(a).add(4) else e -> None
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
    actor.send(a).add(1) else e -> None
    actor.send(a).add(2) else e -> None
    actor.send(a).add(4) else e -> None
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

const REENTRANT: &str = "\
class A:
    n: int

class Neg:
    pass

def bump(var self: A, k: int) -> int:
    self.n = self.n + k
    return self.n

def take_away(var self: A, k: int) -> int fail Neg:
    if self.n < k:
        fail Neg()
    self.n = self.n - k
    return self.n

@reentrant
def wait_for(var self: A, var rx: Receiver[int]) -> int fail ActorError:
    self.n = self.n + 1
    v = rx.recv()? else 0
    self.n = self.n + v
    return self.n

def show(r: Result[int, ActorError]) -> str:
    match r:
        case Ok(v):
            return f\"{v}\"
        case Err(e):
            return f\"{e}\"
";

#[test]
fn a_reentrant_handler_calls_back_into_its_own_actor() {
    let src = format!(
        "{REENTRANT}
@reentrant
def relay(var self: A, me: Actor[A]) -> int fail ActorError:
    self.n = self.n + 1
    v = me.bump(10)?
    self.n = self.n + 100
    return self.n * 1000 + v

def main() uses io:
    a = actor.spawn(A(n=0))
    print(show(a.relay(a)))
    print(a.bump(0) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "111011\n111\n");
}

#[test]
fn another_handler_runs_while_a_reentrant_one_waits() {
    let src = format!(
        "{REENTRANT}
def poke(a: Actor[A], var tx: Sender[int]) -> int:
    b = a.bump(10) else -1
    tx.send(100) else e -> None
    return b

def main() uses io:
    a = actor.spawn(A(n=0))
    tx, rx = Channel[int](1)
    par concurrent:
        print(show(a.wait_for(&rx)))
        print(poke(a, &tx))
    print(a.bump(0) else -1)
"
    );
    assert_eq!(run(&src).expect("runs"), "11\n111\n111\n");
}

const POKE_FAILING: &str = "
def poke(a: Actor[A], var tx: Sender[int]) -> int:
    r = a.take_away(5) else e -> -1
    tx.send(100) else e -> None
    return r
";

#[test]
fn a_reentrant_handler_whose_actor_crashed_while_it_waited_fails_stopped() {
    let src = format!(
        "{REENTRANT}{POKE_FAILING}
def main() uses io:
    a = actor.spawn(A(n=0))
    tx, rx = Channel[int](1)
    par concurrent:
        print(show(a.wait_for(&rx)))
        print(poke(a, &tx))
"
    );
    assert_eq!(
        run(&src).expect("runs"),
        "-1\nStopped(reason=Crashed(error='Neg()'))\n"
    );
}

#[test]
fn a_reentrant_handler_does_not_retake_a_restarted_actor() {
    let src = format!(
        "{REENTRANT}{POKE_FAILING}
def fresh(n: int) -> A:
    return A(n=n)

def main() uses io:
    tx, rx = Channel[int](2)
    with supervisor(actor.one_for_one(intensity=3)):
        a = actor.start(ChildSpec(fresh, (0,), restart=permanent))
        par concurrent:
            print(show(a.wait_for(&rx)))
            print(poke(a, &tx))
        tx.send(1) else e -> None
        print(show(a.wait_for(&rx)))
"
    );
    assert_eq!(
        run(&src).expect("runs"),
        "-1\nStopped(reason=Crashed(error='Neg()'))\n2\n"
    );
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
    // The failed `signal_then` stops `b`, which `a` then finds stopped (RFC 0060 D3).
    assert_eq!(
        run(src).expect("runs"),
        "Deadlock()\nStopped(reason=Crashed(error='Deadlock()'))\n"
    );
}

#[test]
fn a_call_to_a_busy_actor_times_out_at_its_deadline() {
    let src = "\
class A:
    n: int

def get(var self: A) -> int:
    return self.n

def hold(var self: A, var rx: Receiver[int]) -> int fail Cancelled:
    return rx.recv()? else 0

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
    actor.send(a).ping(tx, 5) else e -> None
    par concurrent:
        print(rx.recv() else e -> -1)
        print(1)
";
    assert_eq!(run(src).expect("runs"), "1\n10\n");
}

#[test]
fn each_overflow_policy_handles_a_full_mailbox() {
    let src = "\
class Log:
    n: int

def add(var self: Log, v: int):
    self.n += v

def total(var self: Log) -> int:
    return self.n

def main() uses io, nondet:
    a: Actor[Log, Reject] = actor.spawn(Log(n=0), capacity=1)
    actor.send(a).add(1) else e -> print(f\"{e}\")
    actor.send(a).add(2) else e -> print(f\"{e}\")
    print(a.total() else -1)
    d: Actor[Log, DropOldest] = actor.spawn(Log(n=0), capacity=1)
    actor.send(d).add(1) else e -> print(f\"{e}\")
    actor.send(d).add(2) else e -> print(f\"{e}\")
    print(d.total() else -1)
    n: Actor[Log, DropNew] = actor.spawn(Log(n=0), capacity=1)
    actor.send(n).add(1) else e -> print(f\"{e}\")
    actor.send(n).add(2) else e -> print(f\"{e}\")
    print(n.total() else -1)
    b = actor.spawn(Log(n=0), capacity=1)
    actor.send(b).add(1) else e -> print(f\"{e}\")
    actor.send(b).add(2) else e -> print(f\"{e}\")
    print(b.total() else -1)
    s = actor.send_only(b)
    actor.send(s).add(10) else e -> print(f\"{e}\")
    print(b.total() else -1)
";
    assert_eq!(
        run(src).expect("runs"),
        "Undelivered(reason=Full(), args=(2,))\n1\n2\n1\n3\n13\n"
    );
}

#[test]
fn stopped_actors_say_why_and_failed_sends_return_their_arguments() {
    let src = "\
class C:
    n: int

def inc(var self: C, by: int) -> int fail str:
    if by < 0:
        fail \"negative\"
    self.n += by
    return self.n

def get(var self: C) -> int:
    return self.n

def add(var self: C, by: int):
    self.n += by

def show(r: Result[int, ActorError]) -> str:
    match r:
        case Ok(v):
            return f\"{v}\"
        case Err(e):
            return f\"{e}\"

def scoped() -> Actor[C] uses io:
    with supervisor(actor.one_for_one()):
        d = actor.spawn(C(n=0))
        return d

def main() uses io:
    c = actor.spawn(C(n=0))
    print(c.inc(1) else e -> -1)
    print(c.inc(-1) else e -> -1)
    print(show(c.get()))
    actor.send(c).add(5) else e -> print(f\"{e} {e.args[0]} {e.reason}\")
    d = scoped()
    print(show(d.get()))
    actor.send(d).add(5) else e -> print(f\"{e}\")
";
    assert_eq!(
        run(src).expect("runs"),
        "1\n-1\nStopped(reason=Crashed(error='negative'))\n\
         Undelivered(reason=Stopped(reason=Crashed(error='negative')), args=(5,)) 5 \
         Stopped(reason=Crashed(error='negative'))\n\
         Stopped(reason=Shutdown())\nUndelivered(reason=Stopped(reason=Shutdown()), args=(5,))\n"
    );
}

#[test]
fn policies_and_send_only_handles_are_checked() {
    let src = "\
class C:
    n: int

def add(var self: C, by: int):
    self.n += by

def get(var self: C) -> int:
    return self.n

def main() uses io:
    a: Actor[C, Reject] = actor.spawn(C(n=0))
    b: Actor[C, int] = actor.spawn(C(n=0))
    s = actor.send_only(actor.spawn(C(n=0)))
    print(s.get() else -1)
";
    assert_eq!(codes(src), ["E709", "E201", "E201"]);
}

#[test]
fn a_call_outside_every_deadline_times_out_after_5_s() {
    let src = "\
class A:
    n: int

def get(var self: A) -> int:
    return self.n

def hold(var self: A, var rx: Receiver[int]) -> int fail Cancelled:
    return rx.recv()? else 0

def show(r: Result[int, ActorError]) -> str:
    match r:
        case Ok(v):
            return f\"{v}\"
        case Err(e):
            return f\"{e}\"

def ask(a: Actor[A], var go: Receiver[int]) -> str uses time:
    go.recv() else e -> 0
    t0 = time.monotonic()
    r = show(a.get())
    return f\"{r} after {time.monotonic() - t0}\"

def main() uses io, time:
    with time = virtual(time.now()):
        a = actor.spawn(A(n=0))
        tx, rx = Channel[int](1)
        go_tx, go_rx = Channel[int](1)
        par concurrent:
            for _ in range(1):
                print(ask(a, &go_rx))
                tx.send(7) else e -> None
            print(a.hold(&rx) else e -> -1)
            go_tx.send(1) else e -> None
";
    assert_eq!(run(src).expect("runs"), "Timeout() after PT5S\n7\n");
}

#[test]
fn an_idle_actor_runs_its_mail_when_main_waits() {
    let src = "\
class Relay:
    n: int

def ping(var self: Relay, take out: Sender[int], v: int):
    tx = out
    tx.send(v * 2) else e -> None

def main() uses io:
    tx, rx = Channel[int](1)
    a = actor.spawn(Relay(n=0))
    actor.send(a).ping(tx, 5) else e -> None
    print(rx.recv() else e -> -1)
";
    assert_eq!(run(src).expect("runs"), "10\n");
}
