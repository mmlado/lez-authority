//! The request for clearance from the program's other actors.

use super::*;

// A refusing role that lets its holder through by a holder check, and says whether it saw a via.
struct Pausing;

impl Role for Pausing {
    const NAME: &'static str = "pausing";
    type Slot = AuthoritySlot;
}

const VIA_SEEN: AuthorityError = AuthorityError::Role {
    krate: "lez-pausing-authority",
    code: 201,
    sentence: "the caller came with a via",
};

impl<C: HasSlot<Pausing>> Policy<C> for Pausing {}

impl<C: HasSlot<Pausing>> Hooks<C> for Pausing {
    const REFUSES_MESSAGES: bool = true;

    fn may_run(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        if ctx.caller().is_some_and(|caller| caller.via().is_some()) {
            return Err(Refusal::of::<Pausing>(VIA_SEEN));
        }
        ctx.check::<Pausing>(HasSlot::<Pausing>::slot(config))?;
        Ok(Vec::new())
    }
}

/// The request for clearance in `response`, if it sends one to the config.
fn clearance_in(response: &Response) -> Option<Clearance> {
    let to_config: Vec<&SyncMessage> = response
        .sync_messages
        .iter()
        .filter(|sent| sent.to == config_actor())
        .collect();
    match to_config.as_slice() {
        [] => None,
        [sent] => match borsh::from_slice::<Wire<()>>(&sent.message).unwrap() {
            Wire::Library(LibraryMessage::Clearance(request)) => Some(request),
            other => panic!("not a request for clearance: {other:?}"),
        },
        more => panic!("{} messages to the config", more.len()),
    }
}

fn clearance(authorized: bool) -> Wire<Halt> {
    Wire::Library(LibraryMessage::Clearance(Clearance { authorized }))
}

/// `message` handled at `KEY`'s seat of a program with the roles `(First, Marking)` over `pre`,
/// sent by `from` and signed by `KEY` when `authorized`. The delivery there, in a transaction
/// that passes.
fn handled_with(
    from: Option<Actor>,
    authorized: bool,
    pre: Option<&RootAndMark>,
    message: Halt,
    handler: fn(&ReceiveInput, Halt) -> Response,
) -> Result<Transition, Refusal> {
    let seat = seat_of(KEY);
    let message = borsh::to_vec(&Wire::Program(message)).unwrap();
    let (root, sent) = match from {
        None => (seat, message.clone()),
        Some(sender) if sender.program_account_id == PROGRAM => {
            let pass = Wire::Program(Halt::Pass {
                to: seat,
                bytes: message.clone(),
            });
            (sender, borsh::to_vec(&pass).unwrap())
        }
        Some(sender) => (
            sender,
            borsh::to_vec(&(seat, &message, None::<PdaSeed>)).unwrap(),
        ),
    };
    let signers = if authorized { vec![KEY] } else { Vec::new() };
    let trace = deliver::<(First, Marking), RootAndMark, Halt>(
        &states(pre),
        root,
        &signers,
        sent,
        &[seat, seat_of(OTHER)],
        handler,
    )?;
    let step = trace
        .iter()
        .find(|step| step.input.receiver == seat && step.input.message == message)
        .expect("the message reached the seat");
    assert_eq!(step.input.from, from);
    assert_eq!(step.input.is_authorized, authorized);
    Ok(step.clone())
}

/// What the guard returned at `KEY`'s seat for `message`, in a program whose config clears it.
fn handled(from: Option<Actor>, authorized: bool, message: Halt) -> Response {
    handled_with(from, authorized, Some(&root_and_mark()), message, halt)
        .unwrap()
        .response
}

#[test]
fn a_message_handled_elsewhere_asks_the_config_for_clearance() {
    for from in senders() {
        for authorized in [true, false] {
            let response = handled(from, authorized, Halt::Open);
            assert_eq!(clearance_in(&response), Some(Clearance { authorized }));
        }
    }
}

/// The senders a message handled away from the config can have: a client, another actor of the
/// program, an actor of another program, and the config.
fn senders() -> [Option<Actor>; 4] {
    [
        None,
        Some(seat_of(OTHER)),
        Some(Actor::new(KEY, ELSEWHERE)),
        Some(config_actor()),
    ]
}

#[test]
fn a_message_the_config_sends_on_asks_for_clearance_too() {
    for authorized in [true, false] {
        for message in [Halt::Open, Halt::FromConfig, Halt::Gated] {
            let response = handled(Some(config_actor()), authorized, message);
            assert_eq!(clearance_in(&response), Some(Clearance { authorized }));
        }
    }
}

#[test]
fn an_exempt_message_the_config_sends_on_asks_for_none() {
    let response = handled(Some(config_actor()), true, Halt::FromConfigExempt);
    assert_eq!(clearance_in(&response), None);
}

#[test]
fn the_request_for_clearance_follows_what_the_handler_sent() {
    let response = handled(None, true, Halt::WriteAndSend);
    assert_eq!(response.post_state.as_deref(), Some(&[7u8][..]));
    let [handler_s, request] = response.sync_messages.as_slice() else {
        panic!("the handler's message and the request");
    };
    assert_eq!(handler_s.to, seat_of(OTHER));
    assert_eq!(request.to, config_actor());
    assert_eq!(
        clearance_in(&response),
        Some(Clearance { authorized: true })
    );
}

#[test]
fn an_exempt_message_handled_elsewhere_asks_for_none() {
    for from in senders() {
        let response = handled(from, true, Halt::Exempt);
        assert_eq!(clearance_in(&response), None);
        assert!(response.sync_messages.is_empty());
    }
}

#[test]
fn without_a_refusing_role_nothing_asks_for_clearance() {
    let seat = seat_of(KEY);
    let message = borsh::to_vec(&Wire::Program(Msg::SetValue(1))).unwrap();
    for from in senders() {
        let (root, sent) = match from {
            None => (seat, message.clone()),
            Some(sender) if sender.program_account_id == PROGRAM => {
                let pass = Wire::Program(Msg::Pass {
                    to: KEY,
                    bytes: message.clone(),
                });
                (sender, borsh::to_vec(&pass).unwrap())
            }
            Some(sender) => (
                sender,
                borsh::to_vec(&(seat, &message, None::<PdaSeed>)).unwrap(),
            ),
        };
        let trace = run_bytes(Some(&two(None)), root, &[KEY], sent, &[seat]).unwrap();
        let step = trace
            .iter()
            .find(|step| step.input.receiver == seat && step.input.message == message)
            .expect("the message reached the seat");
        assert_eq!(step.input.from, from);
        assert!(step.input.is_authorized);
        assert!(step.response.sync_messages.is_empty());
    }
}

#[test]
fn a_message_that_needs_a_holder_is_forwarded_not_cleared() {
    let seat = seat_of(PROGRAM);
    let trace = run_with::<(First, Marking), _, _>(
        Some(&root_and_mark()),
        seat,
        &[PROGRAM],
        Wire::Program(Halt::Gated),
        &[],
        halt,
    )
    .unwrap();
    let response = response_at(&trace, seat);
    let [sent] = response.sync_messages.as_slice() else {
        panic!("one message from the seat");
    };
    assert!(matches!(
        borsh::from_slice::<Wire<()>>(&sent.message).unwrap(),
        Wire::Library(LibraryMessage::Forwarded(_))
    ));
}

/// `Halt::Open` at `sender`'s seat, signed by it when `authorized`, with the request for
/// clearance it sends answered at the config `pre`. Every delivery.
fn asking(
    sender: AccountId,
    authorized: bool,
    pre: Option<&RootAndMark>,
) -> Result<Vec<Transition>, Refusal> {
    let signers = if authorized { vec![sender] } else { Vec::new() };
    let message = Wire::Program(Halt::Open);
    run_with::<(First, Marking), _, _>(pre, seat_of(sender), &signers, message, &[], halt)
}

#[test]
fn the_config_answers_a_request_for_clearance_under_the_refusing_role_s_rules() {
    let trace = asking(KEY, true, Some(&root_and_mark())).unwrap();
    let cleared = step_at(&trace, config_actor());
    assert_eq!(cleared.input.from, Some(seat_of(KEY)));
    assert_eq!(
        cleared.input.message,
        borsh::to_vec(&clearance(true)).unwrap()
    );
    assert_eq!(cleared.response.post_state, None);
    assert!(cleared.response.sync_messages.is_empty());
    assert_eq!(
        asking(KEY, true, Some(&halted())).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
}

#[test]
fn a_clearance_sees_the_sender_only_when_it_authorized() {
    let at = |authorized| asking(OTHER, authorized, Some(&root_and_mark())).err();
    assert_eq!(at(true), Some(Refusal::of::<Marking>(BARRED)));
    assert_eq!(at(false), None);
}

#[test]
fn a_clearance_from_outside_the_program_is_refused() {
    let request = borsh::to_vec(&clearance(true)).unwrap();
    let foreign = Actor::new(KEY, ELSEWHERE);
    let sent_on = borsh::to_vec(&(config_actor(), &request, None::<PdaSeed>)).unwrap();
    for (root, message) in [(config_actor(), request.clone()), (foreign, sent_on)] {
        assert_eq!(
            deliver::<(First, Marking), RootAndMark, Halt>(
                &states(Some(&root_and_mark())),
                root,
                &[KEY],
                message,
                &[],
                halt,
            )
            .err(),
            Some(Refusal::of::<First>(AuthorityError::ClearanceFromOutside))
        );
    }
}

#[test]
fn before_the_root_role_s_init_every_clearance_is_refused() {
    assert_eq!(
        asking(KEY, true, None).err(),
        Some(Refusal::of::<First>(AuthorityError::NotInitialized))
    );
}

#[test]
fn a_clearance_is_answered_only_at_the_config() {
    assert_eq!(
        run_with::<(First, Marking), RootAndMark, _>(
            None,
            seat_of(KEY),
            &[KEY],
            clearance(true),
            &[],
            halt
        )
        .err(),
        Some(Refusal::of::<First>(AuthorityError::ClearanceNotAtConfig))
    );
}

#[test]
fn a_clearance_inside_a_hop_does_not_decode() {
    let hop = Wire::<Halt>::Library(LibraryMessage::Forwarded(Forwarded {
        via: None,
        inner: borsh::to_vec(&clearance(true)).unwrap(),
    }));
    // No transaction carries this: a seat refuses a request for clearance and forwards none.
    assert_eq!(
        at_with::<(First, Marking), RootAndMark, Halt>(
            config_actor(),
            Some(seat_of(KEY)),
            false,
            Some(&root_and_mark()),
            hop,
            halt,
        )
        .err(),
        Some(Refusal::of::<First>(AuthorityError::MessageDoesNotDecode))
    );
}

fn forge_clearance(_input: &ReceiveInput, _message: Halt) -> Response {
    let request = Wire::<()>::Library(LibraryMessage::Clearance(Clearance { authorized: true }));
    Response::keep_state().send_sync(config_actor(), &request)
}

#[test]
fn a_handler_cannot_send_the_config_a_request_for_clearance() {
    assert_eq!(
        handled_with(None, false, None, Halt::Exempt, forge_clearance).err(),
        Some(Refusal::of::<First>(AuthorityError::LibrarySend))
    );
}

#[test]
fn a_request_for_clearance_follows_the_hop_on_the_wire() {
    let request = Wire::<()>::Library(LibraryMessage::Clearance(Clearance { authorized: true }));
    assert_eq!(borsh::to_vec(&request).unwrap(), [255, 2, 1]);
}

/// `message` at `KEY`'s seat of a program with the roles `Rs`, signed by `KEY` when
/// `authorized`, over the config `pre`. Every delivery.
fn ask_at_seat<Rs: Roles<Marks, Ask> + 'static>(
    pre: &Marks,
    message: Ask,
    authorized: bool,
) -> Result<Vec<Transition>, Refusal> {
    let signers = if authorized { vec![KEY] } else { Vec::new() };
    let message = Wire::Program(message);
    run_with::<Rs, Marks, Ask>(Some(pre), seat_of(KEY), &signers, message, &[], keep)
}

#[test]
fn a_clearance_asks_only_the_role_that_refuses() {
    let answer = |pre: &Marks| ask_at_seat::<(Marking, Halting)>(pre, Ask::Open, true).err();
    assert_eq!(answer(&marks(7, 0)), None);
    assert_eq!(answer(&marks(0, 7)), Some(Refusal::of::<Marking>(HALTED)));
}

#[test]
fn a_refusing_root_role_asks_for_clearance_away_from_the_config() {
    fn at_seat<Rs: Roles<Marks, Ask> + 'static>(
        message: Ask,
        authorized: bool,
    ) -> Option<Clearance> {
        let trace = ask_at_seat::<Rs>(&marks(7, 7), message, authorized).unwrap();
        clearance_in(&response_at(&trace, seat_of(KEY)))
    }
    for authorized in [true, false] {
        let asked = Some(Clearance { authorized });
        assert_eq!(at_seat::<(Marking, Halting)>(Ask::Open, authorized), asked);
        assert_eq!(
            at_seat::<(Marking, Halting)>(Ask::ExemptHalting, authorized),
            asked
        );
        assert_eq!(
            at_seat::<(Marking, Halting)>(Ask::ExemptMarking, authorized),
            None
        );
        assert_eq!(at_seat::<(Marking,)>(Ask::Open, authorized), asked);
        assert_eq!(at_seat::<(Marking,)>(Ask::ExemptMarking, authorized), None);
    }
    let lone = |pre: &Marks| ask_at_seat::<(Marking,)>(pre, Ask::Open, true).err();
    assert_eq!(lone(&marks(7, 7)), None);
    assert_eq!(lone(&marks(0, 7)), Some(Refusal::of::<Marking>(HALTED)));
}

#[test]
fn a_clearance_at_a_config_that_does_not_decode_is_refused() {
    let broken = States::from([(config_actor(), vec![9])]);
    let message = borsh::to_vec(&Wire::Program(Halt::Open)).unwrap();
    assert_eq!(
        deliver::<(First, Marking), RootAndMark, Halt>(
            &broken,
            seat_of(KEY),
            &[],
            message,
            &[],
            halt
        )
        .err(),
        Some(Refusal::of::<First>(AuthorityError::ConfigDoesNotDecode))
    );
}

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct Paused(AuthoritySlot);

impl HasSlot<Pausing> for Paused {
    fn slot(&self) -> &AuthoritySlot {
        &self.0
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.0
    }
}

#[test]
fn a_holder_check_at_a_clearance_refuses_since_the_via_is_unknown() {
    assert_eq!(
        run_with::<(Pausing,), _, _>(
            Some(&Paused(held(KEY))),
            seat_of(KEY),
            &[KEY],
            Wire::Program(SetMark(1)),
            &[],
            keep
        )
        .err(),
        Some(Refusal::of::<Pausing>(AuthorityError::NotThroughSeat))
    );
}
