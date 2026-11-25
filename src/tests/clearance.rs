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

impl<C: HasSlot<Pausing>> Policy<C> for Pausing {
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

#[test]
fn a_message_handled_elsewhere_asks_the_config_for_clearance() {
    for from in senders() {
        for authorized in [true, false] {
            let response = at_marked_with(
                seat_of(KEY),
                from,
                authorized,
                None,
                Wire::Program(Halt::Open),
                halt,
            )
            .unwrap();
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
            let response = at_marked_with(
                seat_of(KEY),
                Some(config_actor()),
                authorized,
                None,
                Wire::Program(message),
                halt,
            )
            .unwrap();
            assert_eq!(clearance_in(&response), Some(Clearance { authorized }));
        }
    }
}

#[test]
fn an_exempt_message_the_config_sends_on_asks_for_none() {
    let response = at_marked_with(
        seat_of(KEY),
        Some(config_actor()),
        true,
        None,
        Wire::Program(Halt::FromConfigExempt),
        halt,
    )
    .unwrap();
    assert_eq!(clearance_in(&response), None);
}

#[test]
fn the_request_for_clearance_follows_what_the_handler_sent() {
    let response = at_marked_with(
        seat_of(KEY),
        None,
        true,
        None,
        Wire::Program(Halt::WriteAndSend),
        halt,
    )
    .unwrap();
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
        let response = at_marked_with(
            seat_of(KEY),
            from,
            true,
            None,
            Wire::Program(Halt::Exempt),
            halt,
        )
        .unwrap();
        assert_eq!(clearance_in(&response), None);
        assert!(response.sync_messages.is_empty());
    }
}

#[test]
fn without_a_refusing_role_nothing_asks_for_clearance() {
    for from in senders() {
        let response = at(
            seat_of(KEY),
            from,
            true,
            None,
            Wire::Program(Msg::SetValue(1)),
        )
        .unwrap();
        assert!(response.sync_messages.is_empty());
    }
}

#[test]
fn a_message_that_needs_a_holder_is_forwarded_not_cleared() {
    let response = at_marked_with(
        seat_of(PROGRAM),
        None,
        true,
        None,
        Wire::Program(Halt::Gated),
        halt,
    )
    .unwrap();
    let [sent] = response.sync_messages.as_slice() else {
        panic!("one message from the seat");
    };
    assert!(matches!(
        borsh::from_slice::<Wire<()>>(&sent.message).unwrap(),
        Wire::Library(LibraryMessage::Forwarded(_))
    ));
}

#[test]
fn the_config_answers_a_request_for_clearance_under_the_refusing_role_s_rules() {
    let cleared = at_marked_with(
        config_actor(),
        Some(seat_of(KEY)),
        false,
        Some(&root_and_mark()),
        clearance(true),
        halt,
    )
    .unwrap();
    assert_eq!(cleared.post_state, None);
    assert!(cleared.sync_messages.is_empty());
    assert_eq!(
        at_marked_with(
            config_actor(),
            Some(seat_of(KEY)),
            false,
            Some(&halted()),
            clearance(true),
            halt,
        )
        .err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
}

#[test]
fn a_clearance_sees_the_sender_only_when_it_authorized() {
    let at = |authorized| {
        at_marked_with(
            config_actor(),
            Some(seat_of(OTHER)),
            false,
            Some(&root_and_mark()),
            clearance(authorized),
            halt,
        )
        .err()
    };
    assert_eq!(at(true), Some(Refusal::of::<Marking>(BARRED)));
    assert_eq!(at(false), None);
}

#[test]
fn a_clearance_from_outside_the_program_is_refused() {
    for from in [None, Some(Actor::new(KEY, ELSEWHERE))] {
        assert_eq!(
            at_marked_with(
                config_actor(),
                from,
                true,
                Some(&root_and_mark()),
                clearance(true),
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
        at_marked_with(
            config_actor(),
            Some(seat_of(KEY)),
            true,
            None,
            clearance(true),
            halt
        )
        .err(),
        Some(Refusal::of::<First>(AuthorityError::NotInitialized))
    );
}

#[test]
fn a_clearance_is_answered_only_at_the_config() {
    assert_eq!(
        at_marked_with(
            seat_of(KEY),
            Some(seat_of(OTHER)),
            true,
            None,
            clearance(true),
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
    assert_eq!(
        at_marked_with(
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
        at_marked_with(
            seat_of(KEY),
            None,
            false,
            None,
            Wire::Program(Halt::Exempt),
            forge_clearance,
        )
        .err(),
        Some(Refusal::of::<First>(AuthorityError::LibrarySend))
    );
}

#[test]
fn a_request_for_clearance_follows_the_hop_on_the_wire() {
    let request = Wire::<()>::Library(LibraryMessage::Clearance(Clearance { authorized: true }));
    assert_eq!(borsh::to_vec(&request).unwrap(), [255, 2, 1]);
}

#[test]
fn a_clearance_asks_only_the_role_that_refuses() {
    let answer = |pre: &Marks| {
        let request = LibraryMessage::Clearance(Clearance { authorized: true });
        at_with::<(Marking, Halting), Marks, Ask>(
            config_actor(),
            Some(seat_of(KEY)),
            false,
            Some(pre),
            Wire::Library(request),
            keep,
        )
        .err()
    };
    assert_eq!(answer(&marks(7, 0)), None);
    assert_eq!(answer(&marks(0, 7)), Some(Refusal::of::<Marking>(HALTED)));
}

#[test]
fn a_refusing_root_role_asks_for_clearance_away_from_the_config() {
    fn at_seat<Rs: Roles<Marks, Ask>>(message: Ask, authorized: bool) -> Option<Clearance> {
        let response = at_with::<Rs, Marks, Ask>(
            seat_of(KEY),
            None,
            authorized,
            None,
            Wire::Program(message),
            keep,
        )
        .unwrap();
        clearance_in(&response)
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
    let lone = |pre: &Marks| {
        at_with::<(Marking,), Marks, Ask>(
            config_actor(),
            Some(seat_of(KEY)),
            false,
            Some(pre),
            Wire::Library(LibraryMessage::Clearance(Clearance { authorized: true })),
            keep,
        )
        .err()
    };
    assert_eq!(lone(&marks(7, 7)), None);
    assert_eq!(lone(&marks(0, 7)), Some(Refusal::of::<Marking>(HALTED)));
}

#[test]
fn a_clearance_at_a_config_that_does_not_decode_is_refused() {
    let message = clearance(true);
    let input = ReceiveInput {
        receiver: config_actor(),
        from: Some(seat_of(KEY)),
        is_authorized: false,
        pre_state: ActorState::from(vec![9]),
        message: borsh::to_vec(&message).unwrap(),
    };
    assert_eq!(
        try_guard::<(First, Marking), RootAndMark, Halt>(
            &input,
            &SEED,
            InitGate::Deployer,
            message,
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
    let message =
        Wire::<SetMark>::Library(LibraryMessage::Clearance(Clearance { authorized: true }));
    let input = ReceiveInput {
        receiver: config_actor(),
        from: Some(seat_of(KEY)),
        is_authorized: false,
        pre_state: ActorState::from(borsh::to_vec(&Paused(held(KEY))).unwrap()),
        message: borsh::to_vec(&message).unwrap(),
    };
    assert_eq!(
        try_guard::<(Pausing,), Paused, SetMark>(&input, &SEED, InitGate::Deployer, message, keep)
            .err(),
        Some(Refusal::of::<Pausing>(AuthorityError::NotThroughSeat))
    );
}
