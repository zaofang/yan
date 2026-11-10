//! std.sim: the PCT scheduler and trace validation against a spec (RFC 0065 D6, R999).

use std::collections::HashMap;

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::{Interp, Value, panic};

/// Probabilistic concurrency testing (Burckhardt et al., ASPLOS 2010): tasks get distinct random
/// priorities, the highest runnable one runs, and at each of `depth - 1` change points drawn from
/// the seed the task about to run drops below every initial priority.
pub(crate) struct Pct {
    state: u64,
    depth: i64,
    changes: Vec<u64>,
    step: u64,
}

impl Pct {
    fn new(seed: u64, depth: i64, steps: i64) -> Pct {
        let mut p = Pct {
            state: seed,
            depth,
            changes: Vec::new(),
            step: 0,
        };
        for _ in 1..depth {
            let at = 1 + p.next() % steps as u64;
            p.changes.push(at);
        }
        p
    }

    /// SplitMix64.
    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Distinct priorities for `n` new tasks, all above the change-point priorities.
    pub(crate) fn priorities(&mut self, n: usize) -> Vec<i64> {
        let mut p: Vec<i64> = (0..n as i64).map(|i| self.depth + i).collect();
        for i in (1..n).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            p.swap(i, j);
        }
        p
    }

    /// The index among `prios` of the task to run, `None` when nothing is runnable. A change point
    /// demotes the task chosen at that step to the priority of the change, then chooses again.
    pub(crate) fn pick(&mut self, prios: &mut [i64], runnable: &[bool]) -> Option<usize> {
        let best = |prios: &[i64]| {
            (0..prios.len())
                .filter(|i| runnable[*i])
                .max_by_key(|i| prios[*i])
        };
        let mut i = best(prios)?;
        self.step += 1;
        if let Some(k) = self.changes.iter().position(|c| *c == self.step) {
            prios[i] = k as i64;
            i = best(prios)?;
        }
        Some(i)
    }
}

pub(crate) fn pct(it: &mut Interp, a: &[Value]) -> Value {
    let [Value::Int(depth), Value::Int(seed), Value::Int(steps)] = a else {
        unreachable!("checked by the caller")
    };
    it.pct = Some(Pct::new(*seed as u64, *depth as i64, *steps as i64));
    Value::Data {
        name: "Pct".into(),
        fields: vec![
            ("depth".into(), Value::Int(*depth)),
            ("seed".into(), Value::Int(*seed)),
            ("steps".into(), Value::Int(*steps)),
        ],
    }
}

fn field<'a>(v: &'a Value, name: &str) -> &'a Value {
    match v {
        Value::Data { fields, .. } => &fields.iter().find(|(n, _)| &**n == name).expect("field").1,
        _ => unreachable!("a record"),
    }
}

fn text(v: &Value) -> &str {
    match v {
        Value::Str(s) => s,
        _ => unreachable!("a str"),
    }
}

fn list(v: &Value) -> &[Value] {
    match v {
        Value::List(l) => l,
        _ => unreachable!("a list"),
    }
}

/// The first step of `trace` that the spec's machine for its actor does not allow.
fn check(trace: &[Value], spec: &Value) -> Value {
    let mut states: HashMap<&str, &str> = HashMap::new();
    let mut edges: HashMap<(&str, &str, &str), &str> = HashMap::new();
    for m in list(field(spec, "machines")) {
        let actor = text(field(m, "actor"));
        states.insert(actor, text(field(m, "start")));
        for e in list(field(m, "edges")) {
            edges.insert(
                (actor, text(field(e, "src")), text(field(e, "event"))),
                text(field(e, "dst")),
            );
        }
    }
    for (i, step) in trace.iter().enumerate() {
        let (actor, event) = (text(field(step, "actor")), text(field(step, "event")));
        let state = states.get(actor).copied();
        match state.and_then(|s| edges.get(&(actor, s, event))) {
            Some(next) => {
                states.insert(actor, next);
            }
            None => {
                return Value::Data {
                    name: "Rejected".into(),
                    fields: vec![
                        ("index".into(), Value::Int(i as i128)),
                        ("actor".into(), Value::Str(actor.into())),
                        ("event".into(), Value::Str(event.into())),
                        ("state".into(), Value::Str(state.unwrap_or("").into())),
                    ],
                };
            }
        }
    }
    Value::Data {
        name: "Accepted".into(),
        fields: Vec::new(),
    }
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_SIM_PCT,
        Bind::Cap(|it, a| match a {
            [Value::Int(d), Value::Int(_), Value::Int(k)] if *d >= 1 && *k >= 1 => Ok(pct(it, a)),
            [Value::Int(_), Value::Int(_), Value::Int(_)] => {
                panic("precondition `depth >= 1 and steps >= 1` of `pct` failed")
            }
            _ => bad_args(STD_SIM_PCT),
        }),
    ),
    (
        STD_SIM_CHECK_TRACE,
        Bind::Plain(|a| match a {
            [Value::List(trace), spec] => Ok(check(trace, spec)),
            _ => bad_args(STD_SIM_CHECK_TRACE),
        }),
    ),
];

#[cfg(test)]
mod tests {
    use super::Pct;

    fn run(seed: u64) -> Vec<Option<usize>> {
        let mut p = Pct::new(seed, 3, 8);
        let mut prios = p.priorities(3);
        (0..8).map(|_| p.pick(&mut prios, &[true; 3])).collect()
    }

    #[test]
    fn replays_from_the_seed() {
        assert_eq!(run(5), run(5));
        assert!((0..16).any(|s| run(s) != run(5)));
    }

    #[test]
    fn priorities_are_distinct_and_above_changes() {
        let mut p = Pct::new(1, 4, 10);
        let mut ps = p.priorities(5);
        ps.sort();
        assert_eq!(ps, vec![4, 5, 6, 7, 8]);
        assert_eq!(p.changes.len(), 3);
    }

    #[test]
    fn nothing_runnable_picks_nothing() {
        let mut p = Pct::new(1, 2, 10);
        assert_eq!(p.pick(&mut [2, 3], &[false, false]), None);
    }
}
