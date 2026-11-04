//! std.rc, std.sync, std.chan, std.actor and std.lattice: checked by the elaborator and the Yan Core checker;
//! the interpreter runs none of them yet, and stops with "not supported" when it reaches one.

use yan_std::registry::{Id, id::*};

use super::bind::Bind;
use super::unsupported;

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_ACTOR_SHARE,
        Bind::Plain(|_| unsupported("`std.actor.share` in the interpreter")),
    ),
    (
        STD_CHAN_CHANNEL,
        Bind::Plain(|_| unsupported("`std.chan.Channel` in the interpreter")),
    ),
    (
        STD_CHAN_RECEIVER_RECV,
        Bind::Plain(|_| unsupported("`std.chan.receiver.recv` in the interpreter")),
    ),
    (
        STD_CHAN_RECEIVER_SHARE,
        Bind::Plain(|_| unsupported("`std.chan.receiver.share` in the interpreter")),
    ),
    (
        STD_CHAN_SENDER_SEND,
        Bind::Plain(|_| unsupported("`std.chan.sender.send` in the interpreter")),
    ),
    (
        STD_CHAN_SENDER_SHARE,
        Bind::Plain(|_| unsupported("`std.chan.sender.share` in the interpreter")),
    ),
    (
        STD_RC_SHARE,
        Bind::Plain(|_| unsupported("`std.rc.share` in the interpreter")),
    ),
    (
        STD_RC_STRONG_COUNT,
        Bind::Plain(|_| unsupported("`std.rc.strong_count` in the interpreter")),
    ),
    (
        STD_SYNC_ARC_SHARE,
        Bind::Plain(|_| unsupported("`std.sync.arc.share` in the interpreter")),
    ),
    (
        STD_SYNC_ARC_STRONG_COUNT,
        Bind::Plain(|_| unsupported("`std.sync.arc.strong_count` in the interpreter")),
    ),
    (
        STD_SYNC_ATOMIC_LOAD,
        Bind::Plain(|_| unsupported("`std.sync.atomic.load` in the interpreter")),
    ),
    (
        STD_SYNC_ATOMIC_STORE,
        Bind::Plain(|_| unsupported("`std.sync.atomic.store` in the interpreter")),
    ),
    (
        STD_SYNC_CELL_GET,
        Bind::Plain(|_| unsupported("`std.sync.cell.get` in the interpreter")),
    ),
    (
        STD_SYNC_CELL_SET,
        Bind::Plain(|_| unsupported("`std.sync.cell.set` in the interpreter")),
    ),
    (
        STD_LATTICE_GET,
        Bind::Plain(|_| unsupported("`std.lattice.get` in the interpreter")),
    ),
    (
        STD_LATTICE_MAX_VALUE,
        Bind::Plain(|_| unsupported("`std.lattice.max.value` in the interpreter")),
    ),
    (
        STD_LATTICE_MAX_OF,
        Bind::Plain(|_| unsupported("`std.lattice.max_of` in the interpreter")),
    ),
    (
        STD_LATTICE_PUT,
        Bind::Plain(|_| unsupported("`std.lattice.put` in the interpreter")),
    ),
];
