//! The sync messages a role's consent and its own operations come with, which the config sends.

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
const IN_PROGRAM: [u64; 5] = [
    TO_CONFIG,
    PROGRAM_TO_CONFIG,
    PROGRAM_TO_ACTOR,
    LIBRARY_TO_ACTOR,
    AMONG_OTHERS,
];
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
        LIBRARY_ELSEWHERE => vec![library_to(witness())],
        PROGRAM_ELSEWHERE => vec![program_to(Actor::new(KEY, ELSEWHERE))],
        _ => vec![noted_from(ctx.caller().map(Caller::account), config)],
    }
}

impl<C: HasSlot<Sending>> Policy<C> for Sending {
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
        "lez-authority/sending E139: the sending must not send a message to an actor of its own program"
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
