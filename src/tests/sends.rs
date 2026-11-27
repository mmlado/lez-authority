//! The sync messages a role's consent and its own operations come with, which the config sends,
//! and the role's own messages among them, which another actor of the program hands to the role.

use super::*;

// It declares REFUSES_MESSAGES so the guard asks its consent, which never refuses.
struct Sending;

impl Role for Sending {
    const NAME: &'static str = "sending";
    type Slot = Marked;
}

/// Under these marks the role sends a library message to the config, a program message to the
/// config, a program message to another actor of the program, a library message to another
/// actor of the program, and a program message to the config after one to another program.
const TO_CONFIG: u64 = 2;
const PROGRAM_TO_CONFIG: u64 = 3;
const PROGRAM_TO_ACTOR: u64 = 4;
const LIBRARY_TO_ACTOR: u64 = 5;
const AMONG_OTHERS: u64 = 6;
/// Under these it sends its own message to the config, the first role's own message, bytes that
/// start as its own message but do not decode, and its own operation to another actor.
const OWN_TO_CONFIG: u64 = 11;
const FIRST_S_OWN: u64 = 12;
const BROKEN_OWN: u64 = 13;
const OP_TO_ACTOR: u64 = 14;
const IN_PROGRAM: [u64; 9] = [
    TO_CONFIG,
    PROGRAM_TO_CONFIG,
    PROGRAM_TO_ACTOR,
    LIBRARY_TO_ACTOR,
    AMONG_OTHERS,
    OWN_TO_CONFIG,
    FIRST_S_OWN,
    BROKEN_OWN,
    OP_TO_ACTOR,
];
/// Under this it sends its own message to its own actor, and under the next one to a seat.
const OWN_TO_ACTOR: u64 = 10;
const OWN_TO_SEAT: u64 = 15;
/// Under these the role sends a library message, and a program message, to another program.
const LIBRARY_ELSEWHERE: u64 = 7;
const PROGRAM_ELSEWHERE: u64 = 8;

const WITNESS_SEED: PdaSeed = seed(b"sending/witness");

/// A PDA of the program as an actor of another program, where the role's messages go.
fn witness() -> Actor {
    Actor::new(
        AccountId::for_public_pda(&PROGRAM, &WITNESS_SEED),
        ELSEWHERE,
    )
}

/// The role's message for `caller`, naming the config its context gave, with the seed that
/// authorizes the PDA.
fn noted_from(caller: Option<AccountId>, config: Actor) -> SyncMessage {
    SyncMessage::new(witness(), &(caller, config)).with_pda_seeds([WITNESS_SEED])
}

fn noted(caller: Option<AccountId>) -> SyncMessage {
    noted_from(caller, config_actor())
}

/// The key of the role's own actor, the marker, and of a second one.
const MARKER_KEY: [u8; 32] = [7; 32];
const SECOND_KEY: [u8; 32] = [8; 32];

/// The sending role's own actor for `MARKER_KEY`.
fn marker() -> Actor {
    own_actor::<Sending>(config_actor(), &MARKER_KEY)
}

/// The context the config gives a role, with no caller.
fn config_ctx() -> Ctx {
    Ctx::without_caller(config_actor(), InitGate::Deployer)
}

/// Role `R`'s own message for `key`, as its consent or operation returns it.
fn own_for<R: Role>(key: [u8; 32], message: Vec<u8>) -> SyncMessage {
    config_ctx().own_message::<R>(key, message)
}

/// Role `R`'s own message for `key` as it travels.
fn own<R: Role, M>(key: [u8; 32], message: Vec<u8>) -> Wire<M> {
    Wire::Library(LibraryMessage::Own {
        role: R::NAME.into(),
        key,
        message,
    })
}

/// The role's own message for the marker, cut one byte short.
fn broken_own() -> SyncMessage {
    let mut sent = own_for::<Sending>(MARKER_KEY, Vec::new());
    sent.message.pop();
    sent
}

/// The role's own message for the marker, sent to a seat.
fn own_to_seat() -> SyncMessage {
    let mut sent = own_for::<Sending>(MARKER_KEY, vec![2]);
    sent.to = seat_of(OTHER);
    sent
}

fn library_to(to: Actor) -> SyncMessage {
    SyncMessage::new(
        to,
        &Wire::<()>::Library(LibraryMessage::Clearance(Clearance { authorized: true })),
    )
}

fn program_to(to: Actor) -> SyncMessage {
    SyncMessage::new(to, &Wire::Program(Note::Open))
}

/// What the role sends under `mark`, for the caller in `ctx`.
fn sent(ctx: &Ctx, mark: u64) -> Vec<SyncMessage> {
    let config = ctx.config();
    match mark {
        TO_CONFIG => vec![library_to(config)],
        PROGRAM_TO_CONFIG => vec![program_to(config)],
        PROGRAM_TO_ACTOR => vec![program_to(seat_of(OTHER))],
        LIBRARY_TO_ACTOR => vec![library_to(seat_of(OTHER))],
        AMONG_OTHERS => vec![noted_from(None, config), program_to(config)],
        OWN_TO_ACTOR => vec![own_for::<Sending>(MARKER_KEY, vec![2])],
        OWN_TO_SEAT => vec![own_to_seat()],
        OWN_TO_CONFIG => vec![SyncMessage::new(
            config,
            &own::<Sending, ()>(MARKER_KEY, vec![2]),
        )],
        FIRST_S_OWN => vec![own_for::<First>(MARKER_KEY, vec![2])],
        BROKEN_OWN => vec![broken_own()],
        OP_TO_ACTOR => vec![SyncMessage::new(
            seat_of(OTHER),
            &Wire::<()>::op::<Sending>(Op::Renounce),
        )],
        LIBRARY_ELSEWHERE => vec![library_to(witness())],
        PROGRAM_ELSEWHERE => vec![program_to(Actor::new(KEY, ELSEWHERE))],
        _ => vec![noted_from(ctx.caller().map(Caller::account), config)],
    }
}

impl<C: HasSlot<Sending>> Policy<C> for Sending {}

impl<C: HasSlot<Sending>> Hooks<C> for Sending {
    const REFUSES_MESSAGES: bool = true;

    fn operate(ctx: &Ctx, config: &C, op: &[u8]) -> Result<(Marked, Vec<SyncMessage>), Refusal> {
        let mark = borsh::from_slice(op)
            .map_err(|_| Refusal::of::<Sending>(AuthorityError::MessageDoesNotDecode))?;
        let slot = Marked {
            authority_slot: HasSlot::<Sending>::slot(config).authority_slot.clone(),
            mark,
        };
        Ok((slot, sent(ctx, mark)))
    }

    fn may_run(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        Ok(sent(ctx, HasSlot::<Sending>::slot(config).mark))
    }

    fn receive(ctx: &Ctx, state: &[u8], message: &[u8]) -> Result<Option<Vec<u8>>, Refusal> {
        match message {
            [0] => Err(Refusal::of::<Sending>(TURNED_AWAY)),
            [1] => Ok(None),
            _ => Ok(Some(received(state, message, ctx))),
        }
    }
}

/// What the role answers at an actor to its own message `[0]`.
const TURNED_AWAY: AuthorityError = AuthorityError::Role {
    krate: "lez-sending-authority",
    code: 201,
    sentence: "the actor turns the message away",
};

/// What the role writes at an actor for any other message: what it saw there.
fn received(state: &[u8], message: &[u8], ctx: &Ctx) -> Vec<u8> {
    let seen = (
        state.to_vec(),
        message.to_vec(),
        ctx.caller().is_none(),
        ctx.config(),
    );
    borsh::to_vec(&seen).unwrap()
}

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct RootAndSending {
    first: AuthoritySlot,
    sending: Marked,
}

impl HasSlot<First> for RootAndSending {
    fn slot(&self) -> &AuthoritySlot {
        &self.first
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.first
    }
}

impl HasSlot<Sending> for RootAndSending {
    fn slot(&self) -> &Marked {
        &self.sending
    }
    fn slot_mut(&mut self) -> &mut Marked {
        &mut self.sending
    }
}

/// The first role held by the deployer, the sending role by `KEY`, with `mark`.
fn sending(mark: u64) -> RootAndSending {
    RootAndSending {
        first: held(PROGRAM),
        sending: Marked {
            authority_slot: held(KEY),
            mark,
        },
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
enum Note {
    Open,
    Gated,
    Exempt,
}

impl Gated for Note {
    fn needs<R: Role>(&self) -> bool {
        matches!(self, Note::Gated)
            && std::any::TypeId::of::<R>() == std::any::TypeId::of::<First>()
    }

    fn exempt<R: Role>(&self) -> bool {
        matches!(self, Note::Exempt)
            && std::any::TypeId::of::<R>() == std::any::TypeId::of::<Sending>()
    }
}

/// The handler's own message.
fn handlers() -> SyncMessage {
    program_to(seat_of(OTHER))
}

fn note(_input: &ReceiveInput, _message: Note) -> Response {
    Response::keep_state().send(handlers())
}

/// The sending role second, first as the root, and alone.
type SendingSecond = (First, Sending);
type SendingFirst = (Sending, First);
type SendingAlone = (Sending,);

fn at<G: Roles<RootAndSending, Note>>(
    receiver: Actor,
    from: Option<Actor>,
    pre: Option<&RootAndSending>,
    message: Wire<Note>,
) -> Result<Response, Refusal> {
    at_with::<G, RootAndSending, Note>(receiver, from, false, pre, message, note)
}

/// Straight to the config, with no caller.
fn straight<G: Roles<RootAndSending, Note>>(
    pre: &RootAndSending,
    message: Note,
) -> Result<Response, Refusal> {
    at::<G>(config_actor(), None, Some(pre), Wire::Program(message))
}

/// From `sender`'s seat to the config. What the config answered.
fn through_seat<G: Roles<RootAndSending, Note>>(
    sender: AccountId,
    pre: &RootAndSending,
    message: Wire<Note>,
) -> Result<Response, Refusal> {
    let hop = at_with::<G, RootAndSending, Note>(seat_of(sender), None, true, None, message, note)?;
    let hop: Wire<Note> = borsh::from_slice(&hop.sync_messages[0].message).unwrap();
    at::<G>(config_actor(), Some(seat_of(sender)), Some(pre), hop)
}

/// A request for clearance from `from`, answered at the config.
fn cleared<G: Roles<RootAndSending, Note>>(
    from: Actor,
    authorized: bool,
    pre: &RootAndSending,
) -> Result<Response, Refusal> {
    let request = Wire::Library(LibraryMessage::Clearance(Clearance { authorized }));
    at::<G>(config_actor(), Some(from), Some(pre), request)
}

fn own_op(mark: u64) -> Wire<Note> {
    Wire::op::<Sending>(Op::Own(borsh::to_vec(&mark).unwrap()))
}

fn consent_goes_ahead<G: Roles<RootAndSending, Note>>() {
    let guard = std::any::type_name::<G>();
    let straight = straight::<G>(&sending(1), Note::Open).unwrap();
    assert_eq!(straight.sync_messages, [noted(None), handlers()], "{guard}");
}

#[test]
fn a_consent_s_messages_go_ahead_of_the_handler_s_own() {
    consent_goes_ahead::<SendingSecond>();
    consent_goes_ahead::<SendingFirst>();
    consent_goes_ahead::<SendingAlone>();
    for gated in [
        through_seat::<SendingSecond>(PROGRAM, &sending(1), Wire::Program(Note::Gated)),
        through_seat::<SendingFirst>(PROGRAM, &sending(1), Wire::Program(Note::Gated)),
    ] {
        assert_eq!(
            gated.unwrap().sync_messages,
            [noted(Some(PROGRAM)), handlers()]
        );
    }
}

fn clearance_answers<G: Roles<RootAndSending, Note>>() {
    let guard = std::any::type_name::<G>();
    for from in [seat_of(KEY), seat_of(OTHER), config_actor()] {
        for authorized in [true, false] {
            let response = cleared::<G>(from, authorized, &sending(1)).unwrap();
            assert_eq!(response.post_state, None, "{guard}");
            assert_eq!(
                response.sync_messages,
                [noted(authorized.then_some(from.account_id))],
                "{guard}"
            );
        }
    }
}

#[test]
fn a_request_for_clearance_is_answered_with_the_consent_s_messages() {
    clearance_answers::<SendingSecond>();
    clearance_answers::<SendingFirst>();
    clearance_answers::<SendingAlone>();
}

#[test]
fn an_exempt_message_comes_without_the_consent_s_messages() {
    let straight = straight::<SendingSecond>(&sending(1), Note::Exempt).unwrap();
    assert_eq!(straight.sync_messages, [handlers()]);
    let elsewhere = at_with::<SendingSecond, RootAndSending, Note>(
        seat_of(KEY),
        None,
        true,
        None,
        Wire::Program(Note::Exempt),
        note,
    )
    .unwrap();
    assert_eq!(elsewhere.sync_messages, [handlers()]);
}

fn operation_sends<G: Roles<RootAndSending, Note>>() {
    let guard = std::any::type_name::<G>();
    let response = through_seat::<G>(KEY, &sending(1), own_op(9)).unwrap();
    let config: RootAndSending = borsh::from_slice(response.post_state.as_ref().unwrap()).unwrap();
    assert_eq!(config, sending(9), "{guard}");
    assert_eq!(response.sync_messages, [noted(Some(KEY))], "{guard}");
}

#[test]
fn a_role_s_own_operation_sends_its_messages_with_the_new_state() {
    operation_sends::<SendingSecond>();
    operation_sends::<SendingFirst>();
    operation_sends::<SendingAlone>();
}

#[test]
fn a_holder_operation_asks_no_consent_and_sends_nothing() {
    let mut unset = sending(1);
    unset.sending.authority_slot = AuthoritySlot::Unset;
    let mut offered = sending(1);
    offered.sending.authority_slot = AuthoritySlot::Held {
        holder: Holder {
            account: KEY,
            via: None,
        },
        nominee: Some(OTHER),
    };
    let operations = [
        (PROGRAM, unset, Op::Init),
        (KEY, sending(1), Op::Nominate { next: OTHER }),
        (OTHER, offered, Op::Accept { pda_seed: None }),
        (KEY, sending(1), Op::Renounce),
    ];
    for (sender, pre, op) in operations {
        let response =
            through_seat::<SendingSecond>(sender, &pre, Wire::op::<Sending>(op)).unwrap();
        assert!(response.post_state.is_some());
        assert!(response.sync_messages.is_empty());
    }
}

fn refused_in_program<G: Roles<RootAndSending, Note>>() {
    let guard = std::any::type_name::<G>();
    let refused = Some(Refusal::of::<Sending>(AuthorityError::RoleSendsInProgram));
    for mark in IN_PROGRAM {
        let consent = straight::<G>(&sending(mark), Note::Open);
        assert_eq!(consent.err(), refused, "{guard}, mark {mark}, consent");
        let clearance = cleared::<G>(seat_of(KEY), true, &sending(mark));
        assert_eq!(clearance.err(), refused, "{guard}, mark {mark}, clearance");
        let operated = through_seat::<G>(KEY, &sending(1), own_op(mark));
        assert_eq!(operated.err(), refused, "{guard}, mark {mark}, operation");
    }
}

#[test]
fn a_role_that_sends_an_actor_of_its_program_a_message_is_refused() {
    refused_in_program::<SendingSecond>();
    refused_in_program::<SendingFirst>();
    refused_in_program::<SendingAlone>();
    let through_seat = through_seat::<SendingSecond>(
        PROGRAM,
        &sending(PROGRAM_TO_ACTOR),
        Wire::Program(Note::Gated),
    );
    assert_eq!(
        through_seat.err().unwrap().text(),
        "lez-authority/sending E139: the sending may send its own program only its own message, to an actor other than the config"
    );
}

#[test]
fn a_role_s_messages_to_another_program_are_sent() {
    let library = straight::<SendingSecond>(&sending(LIBRARY_ELSEWHERE), Note::Open).unwrap();
    assert_eq!(library.sync_messages, [library_to(witness()), handlers()]);
    let program =
        cleared::<SendingSecond>(seat_of(KEY), true, &sending(PROGRAM_ELSEWHERE)).unwrap();
    assert_eq!(
        program.sync_messages,
        [program_to(Actor::new(KEY, ELSEWHERE))]
    );
    let operated =
        through_seat::<SendingSecond>(KEY, &sending(1), own_op(LIBRARY_ELSEWHERE)).unwrap();
    assert_eq!(operated.sync_messages, [library_to(witness())]);
}

/// Runs `message` at `receiver`, from `from`, over the actor's state `state`.
fn at_actor<G: Roles<RootAndSending, Note>>(
    receiver: Actor,
    from: Option<Actor>,
    state: &[u8],
    message: Wire<Note>,
) -> Result<Response, Refusal> {
    let input = ReceiveInput {
        receiver,
        from,
        is_authorized: false,
        pre_state: ActorState::from(state.to_vec()),
        message: borsh::to_vec(&message).unwrap(),
    };
    try_guard::<G, RootAndSending, Note>(&input, &SEED, InitGate::Deployer, message, note)
}

/// What the role writes for `message` over `state`, with no caller and the config's actor.
fn wrote(state: &[u8], message: &[u8]) -> Option<ActorState> {
    let seen = (state.to_vec(), message.to_vec(), true, config_actor());
    Some(ActorState::from(borsh::to_vec(&seen).unwrap()))
}

#[test]
fn a_role_s_own_actor_is_a_pda_of_the_program_for_its_role_config_and_key() {
    let own = own_for::<Sending>(MARKER_KEY, vec![2]);
    assert_eq!(own.to, marker());
    assert_eq!(own.to.program_account_id, PROGRAM);
    let [seed] = own.pda_seeds.iter().copied().collect::<Vec<_>>()[..] else {
        panic!("one seed");
    };
    assert_eq!(
        AccountId::for_public_pda(&PROGRAM, &seed),
        own.to.account_id
    );
    let other_config = Actor::new(config_account(PROGRAM, &seed_of_other_config()), PROGRAM);
    let others = [
        own_actor::<Sending>(config_actor(), &SECOND_KEY),
        own_actor::<First>(config_actor(), &MARKER_KEY),
        own_actor::<Sending>(other_config, &MARKER_KEY),
        config_actor(),
        seat_of(OTHER),
    ];
    for other in others {
        assert_ne!(marker(), other);
    }
}

fn seed_of_other_config() -> PdaSeed {
    seed(b"two-roles/v2")
}

#[test]
fn a_role_s_own_message_travels_with_its_bytes_last_and_unprefixed() {
    let bytes = borsh::to_vec(&own::<Sending, ()>(MARKER_KEY, vec![9, 8])).unwrap();
    let mut expected = vec![LIBRARY_TAG, 3, 7, 0, 0, 0];
    expected.extend_from_slice(b"sending");
    expected.extend_from_slice(&MARKER_KEY);
    expected.extend_from_slice(&[9, 8]);
    assert_eq!(bytes, expected);
}

fn own_sent<G: Roles<RootAndSending, Note>>() {
    let guard = std::any::type_name::<G>();
    let own = own_for::<Sending>(MARKER_KEY, vec![2]);
    let consent = straight::<G>(&sending(OWN_TO_ACTOR), Note::Open).unwrap();
    assert_eq!(consent.sync_messages, [own.clone(), handlers()], "{guard}");
    let clearance = cleared::<G>(seat_of(KEY), true, &sending(OWN_TO_ACTOR)).unwrap();
    assert_eq!(
        clearance.sync_messages,
        std::slice::from_ref(&own),
        "{guard}"
    );
    let operated = through_seat::<G>(KEY, &sending(1), own_op(OWN_TO_ACTOR)).unwrap();
    assert_eq!(
        operated.sync_messages,
        std::slice::from_ref(&own),
        "{guard}"
    );
    let message = borsh::from_slice(&own.message).unwrap();
    let at_marker = at_actor::<G>(own.to, Some(config_actor()), b"before", message).unwrap();
    assert_eq!(at_marker.post_state, wrote(b"before", &[2]), "{guard}");
}

#[test]
fn a_role_s_own_message_to_its_own_actor_is_sent() {
    own_sent::<SendingSecond>();
    own_sent::<SendingFirst>();
    own_sent::<SendingAlone>();
}

fn receives<G: Roles<RootAndSending, Note>>() {
    let guard = std::any::type_name::<G>();
    for key in [MARKER_KEY, SECOND_KEY] {
        let receiver = own_actor::<Sending>(config_actor(), &key);
        let at = |message: Vec<u8>| {
            at_actor::<G>(
                receiver,
                Some(config_actor()),
                b"before",
                own::<Sending, Note>(key, message),
            )
        };
        let wrote_state = at(vec![2, 3]).unwrap();
        assert_eq!(wrote_state.post_state, wrote(b"before", &[2, 3]), "{guard}");
        assert!(wrote_state.sync_messages.is_empty(), "{guard}");
        let kept = at(vec![1]).unwrap();
        assert_eq!(kept.post_state, None, "{guard}");
        assert!(kept.sync_messages.is_empty(), "{guard}");
        assert_eq!(
            at(vec![0]).err(),
            Some(Refusal::of::<Sending>(TURNED_AWAY)),
            "{guard}"
        );
    }
}

#[test]
fn a_role_s_own_message_runs_its_rules_over_the_actor_s_state() {
    receives::<SendingSecond>();
    receives::<SendingFirst>();
    receives::<SendingAlone>();
}

fn only_at_own_actor<G: Roles<RootAndSending, Note>>() {
    let guard = std::any::type_name::<G>();
    let elsewhere = [
        seat_of(OTHER),
        own_actor::<Sending>(config_actor(), &SECOND_KEY),
        own_actor::<First>(config_actor(), &MARKER_KEY),
    ];
    for receiver in elsewhere {
        // The role turns [0] away, which pins E141 ahead of its rules.
        for message in [vec![2], vec![0]] {
            let own = at_actor::<G>(
                receiver,
                Some(config_actor()),
                b"before",
                own::<Sending, Note>(MARKER_KEY, message),
            );
            assert_eq!(
                own.err(),
                Some(Refusal::of::<Sending>(AuthorityError::NotOwnActor)),
                "{guard}, at {receiver:?}"
            );
        }
    }
}

#[test]
fn a_role_s_own_message_runs_only_at_its_own_actor_for_its_key() {
    only_at_own_actor::<SendingSecond>();
    only_at_own_actor::<SendingFirst>();
    only_at_own_actor::<SendingAlone>();
    let consent = straight::<SendingSecond>(&sending(OWN_TO_SEAT), Note::Open).unwrap();
    assert_eq!(consent.sync_messages[0], own_to_seat());
    let message = borsh::from_slice(&own_to_seat().message).unwrap();
    let at_seat = at_actor::<SendingSecond>(seat_of(OTHER), Some(config_actor()), b"", message);
    assert_eq!(
        at_seat.err().unwrap().text(),
        "lez-authority/sending E141: the sending's own message reached an actor that is not its own for that key"
    );
}

fn only_from_config<G: Roles<RootAndSending, Note>>() {
    let guard = std::any::type_name::<G>();
    let foreign_config = Actor::new(config_actor().account_id, ELSEWHERE);
    let ways = [
        (marker(), None),
        (marker(), Some(seat_of(KEY))),
        (marker(), Some(foreign_config)),
        (config_actor(), Some(config_actor())),
        (config_actor(), Some(seat_of(KEY))),
    ];
    for (receiver, from) in ways {
        for message in [
            own::<Sending, Note>(MARKER_KEY, vec![2]),
            own::<Stranger, Note>(MARKER_KEY, vec![2]),
        ] {
            let own = at_actor::<G>(receiver, from, b"", message);
            assert_eq!(
                own.err().map(|refusal| refusal.error),
                Some(AuthorityError::NotFromConfig),
                "{guard}, at {receiver:?} from {from:?}"
            );
        }
    }
}

#[test]
fn a_role_s_own_message_comes_only_from_the_config_to_another_actor() {
    only_from_config::<SendingSecond>();
    only_from_config::<SendingFirst>();
    only_from_config::<SendingAlone>();
    let refused = at_actor::<SendingSecond>(
        marker(),
        None,
        b"",
        own::<Sending, Note>(MARKER_KEY, vec![2]),
    );
    assert_eq!(
        refused.err(),
        Some(Refusal::of::<First>(AuthorityError::NotFromConfig))
    );
}

struct Stranger;

impl Role for Stranger {
    const NAME: &'static str = "stranger";
    type Slot = AuthoritySlot;
}

/// `R`'s own message for the marker key, at `R`'s own actor, from the config, under the guard `G`.
fn at_own_actor<G: Roles<RootAndSending, Note>, R: Role>() -> Option<Refusal> {
    let receiver = own_actor::<R>(config_actor(), &MARKER_KEY);
    let message = own::<R, Note>(MARKER_KEY, vec![2]);
    at_actor::<G>(receiver, Some(config_actor()), b"", message).err()
}

#[test]
fn an_own_message_for_a_role_without_rules_for_it_does_not_decode() {
    let does_not_decode = |role| {
        Some(Refusal {
            error: AuthorityError::MessageDoesNotDecode,
            ..role
        })
    };
    let first = Refusal::of::<First>(AuthorityError::MessageDoesNotDecode);
    let sending = Refusal::of::<Sending>(AuthorityError::MessageDoesNotDecode);
    assert_eq!(
        at_own_actor::<SendingSecond, First>(),
        does_not_decode(first.clone())
    );
    assert_eq!(
        at_own_actor::<SendingFirst, First>(),
        does_not_decode(first.clone())
    );
    assert_eq!(
        at_own_actor::<SendingSecond, Stranger>(),
        does_not_decode(first)
    );
    assert_eq!(
        at_own_actor::<SendingFirst, Stranger>(),
        does_not_decode(sending.clone())
    );
    assert_eq!(
        at_own_actor::<SendingAlone, Stranger>(),
        does_not_decode(sending)
    );
}

#[test]
fn an_own_message_inside_a_hop_does_not_decode() {
    let hop = Wire::Library(LibraryMessage::Forwarded(Forwarded {
        via: None,
        inner: borsh::to_vec(&own::<Sending, Note>(MARKER_KEY, vec![2])).unwrap(),
    }));
    let at_config = at::<SendingSecond>(config_actor(), Some(seat_of(KEY)), Some(&sending(1)), hop);
    assert_eq!(
        at_config.err(),
        Some(Refusal::of::<First>(AuthorityError::MessageDoesNotDecode))
    );
}

/// A handler that sends the role's own message to its own actor, sync or async, to the config,
/// sync or async, or to another program, or that sends a seat the role's operation.
#[derive(BorshSerialize, BorshDeserialize)]
enum Forge {
    ToMarker,
    AsyncToMarker,
    ToConfig,
    AsyncToConfig,
    Elsewhere,
    OpToSeat,
}

impl Gated for Forge {}

fn forged_own() -> Wire<()> {
    own::<Sending, ()>(MARKER_KEY, vec![2])
}

fn forge(_input: &ReceiveInput, message: Forge) -> Response {
    let own = forged_own();
    let to_config = config_actor();
    let sent = Response::keep_state();
    match message {
        Forge::ToMarker => sent.send(SyncMessage::new(marker(), &own)),
        Forge::AsyncToMarker => sent.send(AsyncMessage::new(marker(), &own)),
        Forge::ToConfig => sent.send(SyncMessage::new(to_config, &own)),
        Forge::AsyncToConfig => sent.send(AsyncMessage::new(to_config, &own)),
        Forge::Elsewhere => sent.send(SyncMessage::new(witness(), &own)),
        Forge::OpToSeat => sent.send(SyncMessage::new(
            seat_of(OTHER),
            &Wire::<()>::op::<Sending>(Op::Renounce),
        )),
    }
}

fn forged(receiver: Actor, message: Forge) -> Result<Response, Refusal> {
    let pre = (receiver == config_actor()).then(|| sending(1));
    at_with::<SendingSecond, RootAndSending, Forge>(
        receiver,
        None,
        false,
        pre.as_ref(),
        Wire::Program(message),
        forge,
    )
}

#[test]
fn a_handler_cannot_send_a_role_s_own_message_in_its_program() {
    let refused = Some(Refusal::of::<First>(AuthorityError::HandlerSendsOwnMessage));
    let to_config = Some(Refusal::of::<First>(AuthorityError::LibrarySend));
    for receiver in [config_actor(), seat_of(KEY)] {
        assert_eq!(forged(receiver, Forge::ToMarker).err(), refused);
        assert_eq!(forged(receiver, Forge::AsyncToMarker).err(), refused);
        assert_eq!(forged(receiver, Forge::ToConfig).err(), to_config);
        assert_eq!(forged(receiver, Forge::AsyncToConfig).err(), to_config);
        let elsewhere = forged(receiver, Forge::Elsewhere).unwrap();
        let own = SyncMessage::new(witness(), &forged_own());
        assert!(elsewhere.sync_messages.contains(&own));
        let operation = forged(receiver, Forge::OpToSeat).unwrap();
        let op = SyncMessage::new(seat_of(OTHER), &Wire::<()>::op::<Sending>(Op::Renounce));
        assert!(operation.sync_messages.contains(&op));
    }
    assert_eq!(
        forged(config_actor(), Forge::ToMarker)
            .err()
            .unwrap()
            .text(),
        "lez-authority/first E140: a handler must not send a role's own message to an actor of its program"
    );
}
