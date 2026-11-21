//! RFC 0094: the channel-constructor names models write instead of `Channel[T](capacity)`.

pub(crate) const CHANNEL_HINT: &str = "channels are built with `Channel[T](capacity)`, which \
                                       returns `(Sender[T], Receiver[T])`";

pub(crate) const HABIT_NAMES: [&str; 7] = [
    "Chan",
    "chan",
    "channel",
    "make_chan",
    "Queue",
    "sync.Channel",
    "nondet.channel",
];

pub(crate) const HABIT_PATHS: [&str; 3] = ["queue.Queue", "asyncio.Queue", "queue.SimpleQueue"];

pub(crate) fn is_habit(n: &str) -> bool {
    HABIT_NAMES.contains(&n) || HABIT_PATHS.contains(&n)
}
