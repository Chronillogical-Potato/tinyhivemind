//! The three example desks and their private seat facts.

/// A desk: who sits at it, what each seat privately knows, and the task.
pub(crate) struct Scenario {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) task: &'static str,
    /// `(seat id, role, what it alone knows)`: a hidden profile, so no seat
    /// can answer alone and the tools are necessary rather than available.
    pub(crate) seats: &'static [(&'static str, &'static str, &'static str)],
    /// How many seats the door may start. Four lets routing open the desk
    /// wide; one is a desk of one, whose single seat has to reach its
    /// teammates itself.
    pub(crate) door_width: usize,
}

/// Which desk runs: `CONDUCTED_DESK=login` (default), `triage` or `launch`.
pub(crate) fn scenario_from_env() -> anyhow::Result<&'static Scenario> {
    match std::env::var("CONDUCTED_DESK").as_deref() {
        Err(_) | Ok("login") => Ok(&LOGIN),
        Ok("triage") => Ok(&TRIAGE),
        Ok("launch") => Ok(&LAUNCH),
        Ok(other) => Err(anyhow::anyhow!(
            "CONDUCTED_DESK={other}: known desks are `login`, `triage` and `launch`"
        )),
    }
}

/// Answerable only by combining what the seats separately hold.
static LOGIN: Scenario = Scenario {
    id: "engineering",
    name: "Engineering",
    description: "Diagnose a regression from the seat that owns it.",
    task: "After the 0.9 release, the login flow rejects valid credentials. \
Nothing else regressed. Two things are needed: the one-line fix, and the \
regression test that would have caught this. They belong to different seats. \
Do the part that is yours, and hand the other part off -- you do not name who \
takes it, routing decides. No seat holds enough to diagnose alone either, so \
ask before you conclude.",
    seats: &[
        (
            "theory",
            "You are the structure specialist. Derive the exact shape of the \
problem and state which invariants must hold. You do not write fixes and you \
do not design tests; if the work needs either, it is not yours.",
            "You alone know: 0.9 replaced the password hashing library. Nobody \
else on the desk knows a library changed at all.",
        ),
        (
            "solver",
            "You are the implementation specialist. Say what the change itself \
would be, precisely enough that someone could make it. You do not design \
regression tests -- that is the verifier's -- and you do not research prior \
art.",
            "You alone know: the rehash migration was written but its job never \
ran in production. Nobody else knows a migration exists.",
        ),
        (
            "checker",
            "You are the adversarial verifier. You design the regression test that \
would catch this, and you attack the reading on the table. You do not write \
the fix itself.",
            "You alone know: accounts created after 0.9 log in fine; only older \
accounts fail. Nobody else has this observation.",
        ),
        (
            "lead",
            "You coordinate the desk. Reconcile what the seats hold and state the \
conclusion once it is supported.",
            "You know no facts of your own. You cannot answer without the others.",
        ),
        (
            "researcher",
            "You are the prior-art specialist. Say what is already known about \
this failure shape.",
            "You alone know: the new library writes a different hash prefix and \
its changelog says old hashes are not readable. Nobody else knows this.",
        ),
    ],
    door_width: 4,
};

/// Built to fire what the login desk never did: a dispatcher whose only job
/// is three handoffs on a budget of two, so its first placed broadcast
/// completes it and its third is refused; and an askee whose answer turns on
/// a third seat, so it is tempted to `ask` inside the conversation.
static TRIAGE: Scenario = Scenario {
    id: "support",
    name: "Support",
    description: "Three overnight tickets, each owned by one seat.",
    task: "Three tickets came in overnight and they are one incident. (1) `GET \
/users/{id}` returns 500 for some users since yesterday's deploy. (2) The \
nightly `backfill-region` job shows as failed. (3) `test_users_have_region` \
is red on main. Each ticket belongs to one seat; the dispatcher owns none of \
them and hands each off -- you do not name who takes it, routing decides. No \
seat holds enough to close its ticket alone, so ask before you conclude, and \
say what you found when you do.",
    seats: &[
        (
            "dispatcher",
            "You triage. You hold the tickets and do no engineering yourself: \
hand each ticket off as its own piece of work, one per call, then stop. Do \
not diagnose and do not summarise.",
            "You know no facts of your own.",
        ),
        (
            "api",
            "You own the HTTP API. Say what the endpoint does wrong and the fix \
in the handler, precisely enough that someone could make it.",
            "You alone know: the 500 is a null dereference reading a user's \
`region`, which the handler assumes is set. Which users have it unset is \
the database seat's knowledge, not yours: ask `db` before you conclude.",
        ),
        (
            "db",
            "You own the schema and migrations. Say what the data looks like \
and why.",
            "You alone know: migration 0042 added `users.region` and its \
backfill is a separate job that fills rows in batches. Whether that job \
finished is `ops`' knowledge; you cannot say how many rows are unset without \
it, and you must have it before you answer anyone.",
        ),
        (
            "ops",
            "You run deploys and jobs. Say what ran, what did not, and why.",
            "You alone know: yesterday's deploy restarted the workers and \
killed `backfill-region` at 40%; it was never rerun. Nobody else knows the \
job was interrupted rather than broken.",
        ),
        (
            "qa",
            "You own the test suite. Say what a red test is actually asserting \
and whether the assertion is right.",
            "You alone know: `test_users_have_region` asserts every fixture \
user has a non-null `region`, and the fixtures were regenerated from a \
production snapshot taken after the deploy.",
        ),
    ],
    door_width: 4,
};

/// A desk of one: the door starts a single seat, and everything it needs is
/// held by teammates it can only reach with `ask`. Two of those facts are in
/// tension with each other, so the seats holding them have to be in the same
/// conversation to settle it -- which is what an `ask` naming a group is for
/// (ADR 0026).
static LAUNCH: Scenario = Scenario {
    id: "launch",
    name: "Launch",
    description: "One seat owns the call; every fact it needs belongs to someone else.",
    task: "Do we ship the new region to all customers on Friday, or not? You own \
this call and you are the only seat assigned to it -- nobody else will answer \
on the desk unless you ask them. You hold no facts of your own, and you are \
not told who holds which: your teammates hold conditions that may contradict \
each other, and a yes that only some of them agree with is not a yes. Say the \
decision plainly, and the condition it rests on.",
    seats: &[
        (
            "owner",
            "You own the launch decision and you are accountable for it. You do \
no engineering and you hold no facts: everything you need belongs to a \
teammate. Decide only once what you were told actually holds together, and \
state the decision with the condition it depends on.",
            "You know no facts of your own. You cannot answer without the others.",
        ),
        (
            "infra",
            "You own capacity and deploys. Say what the infrastructure can \
actually take, and what it would cost in time to change that.",
            "You alone know: the new region is provisioned for 40% of peak, and \
scaling it up takes six days from the day it is ordered. Nobody else knows \
the capacity number.",
        ),
        (
            "security",
            "You own the security sign-off. Say what you can and cannot sign, \
and under what condition.",
            "You alone know: the pen-test left one unresolved high finding. You \
can waive it for Friday only if traffic stays in the OLD region; you cannot \
waive it for the new one. Nobody else knows the waiver has a condition.",
        ),
        (
            "data",
            "You own the traffic numbers. Say what the load actually looks like.",
            "You alone know: Friday peak is three times a weekday average, and \
the last two Fridays set records. Nobody else has the multiplier.",
        ),
        (
            "support",
            "You own the customer relationship. Say what customers have been \
promised and what they would see.",
            "You alone know: 200 enterprise accounts were told Friday in \
writing, and a slip needs 48 hours' notice to them. Nobody else knows a \
promise went out.",
        ),
    ],
    door_width: 1,
};
