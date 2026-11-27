//! A role's looks at an actor's state, before and after the handler.

use super::*;

/// The prefix the watching roles keep the program off.
const TAG: &[u8] = b"MARK";

/// Its consent refuses only while its slot is unset, so a message can be exempt from it.
struct Watching;

impl Role for Watching {
    const NAME: &'static str = "watching";
    type Slot = AuthoritySlot;
}

/// What the watching role answers before the handler at an actor whose state starts with the
/// tag.
const MARKED: AuthorityError = AuthorityError::Role {
    krate: "lez-watching-authority",
    code: 201,
    sentence: "the actor is marked",
};

/// What it answers to a write that starts with the tag.
const WRITES_MARK: AuthorityError = AuthorityError::Role {
    krate: "lez-watching-authority",
    code: 202,
    sentence: "the write starts with the mark",
};

/// What its consent answers while its slot has no holder.
const UNSET: AuthorityError = AuthorityError::Role {
    krate: "lez-watching-authority",
    code: 203,
    sentence: "the watching role has no holder",
};

impl<C: HasSlot<Watching>> Policy<C> for Watching {}

impl<C: HasSlot<Watching>> Hooks<C> for Watching {
    const REFUSES_MESSAGES: bool = true;
    const WATCHES_STATE: bool = true;

    fn may_run(_ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        if matches!(HasSlot::<Watching>::slot(config), AuthoritySlot::Unset) {
            return Err(Refusal::of::<Watching>(UNSET));
        }
        Ok(Vec::new())
    }

    fn receive(_ctx: &Ctx, _state: &[u8], message: &[u8]) -> Result<Option<Vec<u8>>, Refusal> {
        Ok(Some(message.to_vec()))
    }

    fn may_handle(ctx: &Ctx, state: &[u8]) -> Result<(), Refusal> {
        assert!(
            ctx.caller().is_none(),
            "a handler is watched with no caller"
        );
        assert_eq!(ctx.config(), config_actor());
        if state.starts_with(TAG) {
            return Err(Refusal::of::<Watching>(MARKED));
        }
        Ok(())
    }

    fn may_write(ctx: &Ctx, _before: &[u8], after: Option<&[u8]>) -> Result<(), Refusal> {
        assert!(ctx.caller().is_none(), "a write is watched with no caller");
        assert_eq!(ctx.config(), config_actor());
        if after.is_some_and(|after| after.starts_with(TAG)) {
            return Err(Refusal::of::<Watching>(WRITES_MARK));
        }
        Ok(())
    }
}

/// A second role that watches. It refuses an actor or a write whose state starts with the tag or
/// with `PEEK`, a kept state that starts with `KEEP`, and a write over a state that starts with
/// `HOLD`.
struct Peeking;

impl Role for Peeking {
    const NAME: &'static str = "peeking";
    type Slot = AuthoritySlot;
}

const PEEKED: AuthorityError = AuthorityError::Role {
    krate: "lez-peeking-authority",
    code: 201,
    sentence: "the actor was peeked at",
};

const KEPT: AuthorityError = AuthorityError::Role {
    krate: "lez-peeking-authority",
    code: 202,
    sentence: "the handler kept the state",
};

const WRITES_PEEK: AuthorityError = AuthorityError::Role {
    krate: "lez-peeking-authority",
    code: 203,
    sentence: "the write was peeked at",
};

const OVERWRITES: AuthorityError = AuthorityError::Role {
    krate: "lez-peeking-authority",
    code: 204,
    sentence: "the write replaces a held state",
};

fn peeked(state: &[u8]) -> bool {
    state.starts_with(TAG) || state.starts_with(b"PEEK")
}

impl<C: HasSlot<Peeking>> Policy<C> for Peeking {}

impl<C: HasSlot<Peeking>> Hooks<C> for Peeking {
    const WATCHES_STATE: bool = true;

    fn may_handle(_ctx: &Ctx, state: &[u8]) -> Result<(), Refusal> {
        if peeked(state) {
            return Err(Refusal::of::<Peeking>(PEEKED));
        }
        Ok(())
    }

    fn may_write(_ctx: &Ctx, before: &[u8], after: Option<&[u8]>) -> Result<(), Refusal> {
        match after {
            None if before.starts_with(b"KEEP") => Err(Refusal::of::<Peeking>(KEPT)),
            Some(_) if before.starts_with(b"HOLD") => Err(Refusal::of::<Peeking>(OVERWRITES)),
            Some(after) if peeked(after) => Err(Refusal::of::<Peeking>(WRITES_PEEK)),
            _ => Ok(()),
        }
    }
}

/// A role whose looks would panic, which leaves the switch unset.
struct Blind;

impl Role for Blind {
    const NAME: &'static str = "blind";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<Blind>> Policy<C> for Blind {}

impl<C: HasSlot<Blind>> Hooks<C> for Blind {
    fn may_handle(_ctx: &Ctx, _state: &[u8]) -> Result<(), Refusal> {
        panic!("a role that does not watch state was asked before the handler");
    }

    fn may_write(_ctx: &Ctx, _before: &[u8], _after: Option<&[u8]>) -> Result<(), Refusal> {
        panic!("a role that does not watch state was asked after the handler");
    }
}

/// A role that sets the switch and keeps both looks as they are by default.
struct Idle;

impl Role for Idle {
    const NAME: &'static str = "idle";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<Idle>> Policy<C> for Idle {}

impl<C: HasSlot<Idle>> Hooks<C> for Idle {
    const WATCHES_STATE: bool = true;
}

/// A config whose first bytes the handler may set, so a write at the config can start with the
/// tag while every slot stays as it was.
#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct Watched {
    head: [u8; 4],
    first: AuthoritySlot,
    watching: AuthoritySlot,
    peeking: AuthoritySlot,
    blind: AuthoritySlot,
    idle: AuthoritySlot,
}

impl HasSlot<First> for Watched {
    fn slot(&self) -> &AuthoritySlot {
        &self.first
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.first
    }
}

impl HasSlot<Watching> for Watched {
    fn slot(&self) -> &AuthoritySlot {
        &self.watching
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.watching
    }
}

impl HasSlot<Peeking> for Watched {
    fn slot(&self) -> &AuthoritySlot {
        &self.peeking
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.peeking
    }
}

impl HasSlot<Blind> for Watched {
    fn slot(&self) -> &AuthoritySlot {
        &self.blind
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.blind
    }
}

impl HasSlot<Idle> for Watched {
    fn slot(&self) -> &AuthoritySlot {
        &self.idle
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.idle
    }
}

/// The config with `head` in front, the deployer holding the first role and `KEY` the
/// watching role.
fn config(head: [u8; 4]) -> Vec<u8> {
    config_with(head, held(KEY))
}

/// The same config with `watching` as the watching role's slot.
fn config_with(head: [u8; 4], watching: AuthoritySlot) -> Vec<u8> {
    let config = Watched {
        head,
        first: held(PROGRAM),
        watching,
        ..Watched::default()
    };
    borsh::to_vec(&config).unwrap()
}

#[derive(BorshSerialize, BorshDeserialize)]
enum Write {
    Keep,
    Set(Vec<u8>),
    Head([u8; 4]),
    Gated([u8; 4]),
    Exempt(Vec<u8>),
    FromConfig(Vec<u8>),
    Leak(Vec<u8>),
    Own(Vec<u8>),
    Pass(Vec<u8>),
    Panic,
}

impl Gated for Write {
    fn needs<R: Role>(&self) -> bool {
        matches!(self, Write::Gated(_))
            && std::any::TypeId::of::<R>() == std::any::TypeId::of::<Watching>()
    }

    fn only_from_config(&self) -> bool {
        matches!(self, Write::FromConfig(_))
    }

    fn exempt<R: Role>(&self) -> bool {
        matches!(self, Write::Exempt(_))
            && std::any::TypeId::of::<R>() == std::any::TypeId::of::<Watching>()
    }
}

/// Writes the bytes it is given, or the config with a new head. `Leak` also sends the config a
/// library message, `Own` sends a seat the watching role's own message, `Pass` sends a seat a
/// program message, and `Panic` panics, as a handler that cannot read a marked state would.
fn write(input: &ReceiveInput, message: Write) -> Response {
    match message {
        Write::Keep => Response::keep_state(),
        Write::Set(bytes) | Write::Exempt(bytes) | Write::FromConfig(bytes) => {
            Response::set_state(bytes)
        }
        Write::Head(head) | Write::Gated(head) => {
            let mut config: Watched = borsh::from_slice(&input.pre_state).unwrap();
            config.head = head;
            Response::set_state(borsh::to_vec(&config).unwrap())
        }
        Write::Leak(bytes) => Response::set_state(bytes).send_sync(
            config_actor(),
            &Wire::<()>::Library(LibraryMessage::Clearance(Clearance { authorized: true })),
        ),
        Write::Own(bytes) => Response::set_state(bytes).send_sync(
            seat_of(OTHER),
            &Wire::<()>::Library(LibraryMessage::Own {
                role: Watching::NAME.into(),
                key: [7; 32],
                message: Vec::new(),
            }),
        ),
        Write::Pass(bytes) => Response::set_state(bytes).send_sync(seat_of(KEY), &Write::Keep),
        Write::Panic => panic!("the handler ran"),
    }
}

type WatchSecond = (First, Watching);
type WatchFirst = (Watching, First);
type WatchAlone = (Watching,);

fn at<G: Roles<Watched, Write>>(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: &[u8],
    message: Wire<Write>,
) -> Result<Response, Refusal> {
    let input = ReceiveInput {
        receiver,
        from,
        is_authorized: authorized,
        pre_state: ActorState::from(pre.to_vec()),
        message: borsh::to_vec(&message).unwrap(),
    };
    try_guard::<G, Watched, Write>(&input, &SEED, InitGate::Deployer, message, write)
}

/// From `sender`'s seat to the config. What the config answered.
fn through_seat<G: Roles<Watched, Write>>(
    sender: AccountId,
    pre: &[u8],
    message: Wire<Write>,
) -> Result<Response, Refusal> {
    let hop = at::<G>(seat_of(sender), None, true, b"", message)?;
    let hop: Wire<Write> = borsh::from_slice(&hop.sync_messages[0].message).unwrap();
    at::<G>(config_actor(), Some(seat_of(sender)), false, pre, hop)
}

fn set(bytes: &[u8]) -> Wire<Write> {
    Wire::Program(Write::Set(bytes.to_vec()))
}

fn exempt(bytes: &[u8]) -> Wire<Write> {
    Wire::Program(Write::Exempt(bytes.to_vec()))
}

fn from_config(bytes: &[u8]) -> Wire<Write> {
    Wire::Program(Write::FromConfig(bytes.to_vec()))
}

fn watched<G: Roles<Watched, Write>>() {
    let guard = std::any::type_name::<G>();
    let marked = Some(Refusal::of::<Watching>(MARKED));
    let writes = Some(Refusal::of::<Watching>(WRITES_MARK));
    let not_holder = Some(Refusal::of::<Watching>(AuthorityError::NotHolder));
    let unset = Some(Refusal::of::<Watching>(UNSET));
    let not_from_config = Some(Refusal {
        role: G::ROOT,
        prose: G::ROOT_PROSE,
        error: AuthorityError::NotFromConfig,
    });
    let actor = seat_of(OTHER);
    let config_sent = Some(config_actor());
    let relayed = Some(seat_of(KEY));
    let foreign = Some(Actor::new(KEY, ELSEWHERE));
    let at_config =
        |pre: &[u8], message: Write| at::<G>(config_actor(), None, false, pre, message.into());
    let unwatched_config = config_with(*b"MARK", AuthoritySlot::Unset);
    let refusals = [
        (
            at::<G>(actor, config_sent, false, b"MARK1", set(b"ok")),
            &marked,
        ),
        (
            at::<G>(actor, config_sent, false, b"", set(b"MARK")),
            &writes,
        ),
        (
            at::<G>(
                actor,
                config_sent,
                false,
                b"MARK1",
                Write::Gated(*b"next").into(),
            ),
            &marked,
        ),
        (
            at::<G>(
                actor,
                config_sent,
                false,
                &config(*b"none"),
                Write::Gated(*b"MARK").into(),
            ),
            &writes,
        ),
        (
            at::<G>(actor, relayed, false, b"MARK1", set(b"ok")),
            &marked,
        ),
        (at::<G>(actor, relayed, false, b"", set(b"MARK")), &writes),
        (at::<G>(actor, None, true, b"MARK1", set(b"ok")), &marked),
        (at::<G>(actor, None, true, b"", set(b"MARK")), &writes),
        (
            at::<G>(actor, foreign, false, b"MARK1", set(b"ok")),
            &marked,
        ),
        (at::<G>(actor, foreign, false, b"", set(b"MARK")), &writes),
        (
            at::<G>(actor, relayed, false, b"MARK1", from_config(b"ok")),
            &not_from_config,
        ),
        (
            at::<G>(
                actor,
                None,
                false,
                b"",
                Write::Pass(b"MARK".to_vec()).into(),
            ),
            &writes,
        ),
        (at_config(&unwatched_config, Write::Keep), &unset),
        (
            at_config(&unwatched_config, Write::Exempt(b"ok".to_vec())),
            &marked,
        ),
        (
            at_config(&config(*b"none"), Write::Exempt(b"MARKx".to_vec())),
            &writes,
        ),
        (
            at_config(&config(*b"MARK"), Write::Exempt(b"ok".to_vec())),
            &marked,
        ),
        (at::<G>(actor, None, false, b"", set(b"MARK1")), &writes),
        (at::<G>(actor, None, false, b"MARK1", set(b"ok")), &marked),
        (
            at::<G>(actor, None, false, b"MARK1", Write::Keep.into()),
            &marked,
        ),
        (
            at::<G>(actor, None, false, b"MARK1", Write::Panic.into()),
            &marked,
        ),
        (at::<G>(actor, None, false, b"", exempt(b"MARK")), &writes),
        (
            at::<G>(actor, None, false, b"MARK1", exempt(b"ok")),
            &marked,
        ),
        (
            at::<G>(actor, config_sent, false, b"", from_config(b"MARK")),
            &writes,
        ),
        (
            at::<G>(actor, config_sent, false, b"MARK1", from_config(b"ok")),
            &marked,
        ),
        (at_config(&config(*b"none"), Write::Head(*b"MARK")), &writes),
        (
            at_config(&config(*b"none"), Write::Set(b"MARKx".to_vec())),
            &writes,
        ),
        (at_config(&config(*b"MARK"), Write::Keep), &marked),
        (at_config(&config(*b"MARK"), Write::Panic), &marked),
        (
            through_seat::<G>(KEY, &config(*b"none"), Write::Gated(*b"MARK").into()),
            &writes,
        ),
        (
            through_seat::<G>(KEY, &config(*b"MARK"), Write::Gated(*b"next").into()),
            &marked,
        ),
        (
            through_seat::<G>(OTHER, &config(*b"MARK"), Write::Gated(*b"next").into()),
            &not_holder,
        ),
    ];
    for (n, (refused, expected)) in refusals.into_iter().enumerate() {
        assert_eq!(&refused.err(), expected, "{guard}, case {n}");
    }
    let wrote = at::<G>(actor, None, false, b"old", set(b"ok")).unwrap();
    assert_eq!(
        wrote.post_state,
        Some(ActorState::from(b"ok".to_vec())),
        "{guard}"
    );
    assert!(
        at::<G>(actor, None, false, b"old", Write::Keep.into()).is_ok(),
        "{guard}"
    );
    let head = at_config(&config(*b"none"), Write::Head(*b"next"));
    assert!(head.is_ok(), "{guard}");
    let gated = through_seat::<G>(KEY, &config(*b"none"), Write::Gated(*b"next").into());
    assert!(gated.is_ok(), "{guard}");
}

#[test]
fn a_role_that_watches_state_looks_around_every_handler_at_every_actor() {
    watched::<WatchSecond>();
    watched::<WatchFirst>();
    watched::<WatchAlone>();
}

#[test]
fn a_role_that_does_not_watch_state_is_never_asked() {
    type Unwatched = (First, Blind);
    let marked = at::<Unwatched>(seat_of(OTHER), None, false, b"MARK", set(b"MARK"));
    assert!(marked.is_ok());
    let head = at::<Unwatched>(
        config_actor(),
        None,
        false,
        &config(*b"MARK"),
        Write::Head(*b"MARK").into(),
    );
    assert!(head.is_ok());
    let beside = at::<(Watching, Blind)>(seat_of(OTHER), None, false, b"old", set(b"ok"));
    assert!(beside.is_ok());
    let before = at::<(Blind, Watching)>(seat_of(OTHER), None, false, b"MARK", set(b"ok"));
    assert_eq!(before.err(), Some(Refusal::of::<Watching>(MARKED)));
    let after = at::<(Blind, Watching)>(seat_of(OTHER), None, false, b"old", set(b"MARK"));
    assert_eq!(after.err(), Some(Refusal::of::<Watching>(WRITES_MARK)));
}

#[test]
fn a_guard_watches_when_one_of_its_roles_does() {
    let watches = [
        <WatchAlone as Roles<Watched, Write>>::WATCHES_STATE,
        <WatchFirst as Roles<Watched, Write>>::WATCHES_STATE,
        <WatchSecond as Roles<Watched, Write>>::WATCHES_STATE,
        <(Blind, Watching) as Roles<Watched, Write>>::WATCHES_STATE,
        <(Blind,) as Roles<Watched, Write>>::WATCHES_STATE,
        <(First, Blind) as Roles<Watched, Write>>::WATCHES_STATE,
    ];
    assert_eq!(watches, [true, true, true, true, false, false]);
}

#[test]
fn both_looks_let_everything_through_by_default() {
    let actor = seat_of(OTHER);
    for pre in [&b""[..], b"MARK"] {
        let wrote = at::<(First, Idle)>(actor, None, false, pre, set(b"MARK"));
        assert!(wrote.is_ok(), "{pre:?}");
        let kept = at::<(First, Idle)>(actor, None, false, pre, Write::Keep.into());
        assert!(kept.is_ok(), "{pre:?}");
    }
}

fn both_states<G: Roles<Watched, Write>>() {
    let guard = std::any::type_name::<G>();
    let actor = seat_of(OTHER);
    let kept = at::<G>(actor, None, false, b"KEEP", Write::Keep.into());
    assert_eq!(kept.err(), Some(Refusal::of::<Peeking>(KEPT)), "{guard}");
    let wrote = at::<G>(actor, None, false, b"KEEP", set(b"KEEP"));
    assert!(wrote.is_ok(), "{guard}");
    let over = at::<G>(actor, None, false, b"HOLD", set(b"ok"));
    assert_eq!(
        over.err(),
        Some(Refusal::of::<Peeking>(OVERWRITES)),
        "{guard}"
    );
}

#[test]
fn the_look_after_the_handler_sees_the_state_before_and_the_write() {
    both_states::<(First, Peeking)>();
    both_states::<(Peeking, First)>();
    both_states::<(Peeking,)>();
}

#[test]
fn a_message_that_runs_no_handler_is_not_watched() {
    let marked = config(*b"MARK");
    let nominate = Wire::op::<Watching>(Op::Nominate { next: OTHER });
    let operation = through_seat::<WatchSecond>(KEY, &marked, nominate).unwrap();
    assert!(operation.post_state.is_some());
    let gated = Write::Gated(*b"none").into();
    let hop = at::<WatchSecond>(seat_of(KEY), None, true, b"MARK", gated).unwrap();
    assert_eq!(hop.post_state, None);
    let request = Wire::Library(LibraryMessage::Clearance(Clearance { authorized: true }));
    let clearance = at::<WatchSecond>(config_actor(), Some(seat_of(KEY)), false, &marked, request);
    assert!(clearance.is_ok());
    let key = [7; 32];
    let own = Wire::Library(LibraryMessage::Own {
        role: Watching::NAME.into(),
        key,
        message: b"MARK2".to_vec(),
    });
    let own_actor = own_actor::<Watching>(config_actor(), &key);
    let wrote = at::<WatchSecond>(own_actor, Some(config_actor()), false, b"MARK1", own).unwrap();
    assert_eq!(wrote.post_state, Some(ActorState::from(b"MARK2".to_vec())));
}

#[test]
fn a_message_that_keeps_the_state_and_runs_no_handler_is_not_watched_after() {
    // Peeking refuses a kept state that starts with KEEP, so a look after these would show.
    type Both = (Watching, Peeking);
    let gated = Write::Gated(*b"none").into();
    let forward = at::<Both>(seat_of(KEY), None, true, b"KEEP", gated);
    assert_eq!(forward.unwrap().post_state, None);
    let nominate = Wire::op::<Watching>(Op::Nominate { next: OTHER });
    let operation = at::<Both>(seat_of(KEY), None, true, b"KEEP", nominate);
    assert_eq!(operation.unwrap().post_state, None);
    let request = Wire::Library(LibraryMessage::Clearance(Clearance { authorized: true }));
    let clearance = at::<Both>(
        config_actor(),
        Some(seat_of(KEY)),
        false,
        &config(*b"KEEP"),
        request,
    );
    assert_eq!(clearance.unwrap().post_state, None);
}

#[test]
fn every_role_that_watches_is_asked_in_the_guard_s_order() {
    type WatchingFirst = (Watching, Peeking);
    type PeekingFirst = (Peeking, Watching);
    let actor = seat_of(OTHER);
    let marked = Some(Refusal::of::<Watching>(MARKED));
    let writes_mark = Some(Refusal::of::<Watching>(WRITES_MARK));
    let peeked = Some(Refusal::of::<Peeking>(PEEKED));
    let writes_peek = Some(Refusal::of::<Peeking>(WRITES_PEEK));
    let cases = [
        (
            at::<WatchingFirst>(actor, None, false, b"MARK", set(b"ok")),
            &marked,
        ),
        (
            at::<PeekingFirst>(actor, None, false, b"MARK", set(b"ok")),
            &peeked,
        ),
        (
            at::<WatchingFirst>(actor, None, false, b"PEEK", set(b"ok")),
            &peeked,
        ),
        (
            at::<WatchingFirst>(actor, None, false, b"", set(b"MARK")),
            &writes_mark,
        ),
        (
            at::<PeekingFirst>(actor, None, false, b"", set(b"MARK")),
            &writes_peek,
        ),
        (
            at::<WatchingFirst>(actor, None, false, b"", set(b"PEEK")),
            &writes_peek,
        ),
        (
            at::<WatchingFirst>(actor, None, false, b"PEEK", set(b"MARK")),
            &peeked,
        ),
    ];
    for (n, (refused, expected)) in cases.into_iter().enumerate() {
        assert_eq!(&refused.err(), expected, "case {n}");
    }
}

#[test]
fn the_handler_s_sends_are_checked_before_its_write_is_watched() {
    let leak = Write::Leak(b"MARK".to_vec()).into();
    assert_eq!(
        at::<WatchSecond>(seat_of(OTHER), None, false, b"", leak).err(),
        Some(Refusal::of::<First>(AuthorityError::LibrarySend))
    );
    let own = Write::Own(b"MARK".to_vec()).into();
    assert_eq!(
        at::<WatchSecond>(seat_of(OTHER), None, false, b"", own).err(),
        Some(Refusal::of::<First>(AuthorityError::HandlerSendsOwnMessage))
    );
}
