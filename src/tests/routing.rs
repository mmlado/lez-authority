//! Where each message runs and what is checked on the way, through two roles.

use super::*;

/// A role the program does not have.
struct Third;

impl Role for Third {
    const NAME: &'static str = "third";
    type Slot = AuthoritySlot;
}

#[test]
fn only_the_first_role_creates_the_config() {
    assert_eq!(
        send(PROGRAM, None, Wire::op::<Second>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::NotInitialized))
    );
    assert_eq!(
        send(PROGRAM, None, Wire::op::<First>(Op::Init)),
        Ok(two(None))
    );
}

#[test]
fn the_first_role_installs_and_nominates_for_the_second() {
    assert_eq!(
        send(KEY, Some(&two(None)), Wire::op::<Second>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::NotHolder))
    );
    let config = send(PROGRAM, Some(&two(None)), Wire::op::<Second>(Op::Init)).unwrap();
    assert_eq!(config, two(Some(PROGRAM)));
    let nominate = Wire::op::<Second>(Op::Nominate { next: KEY });
    let config = send(PROGRAM, Some(&config), nominate).unwrap();
    let accept = Wire::op::<Second>(Op::Accept { pda_seed: None });
    let config = send(KEY, Some(&config), accept).unwrap();
    assert_eq!(config, two(Some(KEY)));
    let nominate = Wire::op::<Second>(Op::Nominate { next: OTHER });
    let config = send(PROGRAM, Some(&config), nominate).unwrap();
    assert_eq!(config.second.nominee(), Some(OTHER));
    assert_eq!(
        send(
            OTHER,
            Some(&config),
            Wire::op::<Second>(Op::Nominate { next: OTHER })
        ),
        Err(Refusal::of::<First>(AuthorityError::NotHolder))
    );
}

#[test]
fn a_role_s_policy_refuses_before_its_slot_does() {
    let renounced = Two {
        second: AuthoritySlot::Renounced,
        ..two(None)
    };
    for op in [Op::Init, Op::Nominate { next: OTHER }] {
        let name = format!("{op:?}");
        assert_eq!(
            send(KEY, Some(&renounced), Wire::op::<Second>(op)),
            Err(Refusal::of::<First>(AuthorityError::NotHolder)),
            "{name}"
        );
    }
}

#[test]
fn a_gated_message_takes_the_holder_of_its_role() {
    let config = send(KEY, Some(&two(Some(KEY))), Msg::GatedValue(5).into()).unwrap();
    assert_eq!(config.value, 5);
}

#[test]
fn the_first_roles_holder_does_not_hold_the_second() {
    assert_eq!(
        send(PROGRAM, Some(&two(Some(KEY))), Msg::GatedValue(5).into()),
        Err(Refusal::of::<Second>(AuthorityError::NotHolder))
    );
}

#[test]
fn require_refuses_a_message_that_skipped_the_seat() {
    let refused = run(
        Some(&two(Some(KEY))),
        config_actor(),
        &[],
        Msg::GatedValue(3).into(),
    )
    .err();
    assert_eq!(
        refused,
        Some(Refusal::of::<Second>(AuthorityError::NotThroughSeat))
    );
}

#[test]
fn an_offer_on_a_slot_with_no_holder_is_made_replaced_and_cancelled() {
    let offer = |next| Wire::op::<Second>(Op::Nominate { next });
    let config = send(PROGRAM, Some(&two(None)), offer(KEY)).unwrap();
    assert_eq!(config.second, AuthoritySlot::Offered { nominee: KEY });
    let config = send(PROGRAM, Some(&config), offer(OTHER)).unwrap();
    assert_eq!(config.second, AuthoritySlot::Offered { nominee: OTHER });
    let config = send(PROGRAM, Some(&config), offer(AccountId::default())).unwrap();
    assert_eq!(config, two(None));
    let config = send(PROGRAM, Some(&config), offer(AccountId::default())).unwrap();
    assert_eq!(config, two(None));
}

#[test]
fn only_the_offered_account_accepts_and_then_holds_the_role() {
    let offer = Wire::op::<Second>(Op::Nominate { next: KEY });
    let config = send(PROGRAM, Some(&two(None)), offer).unwrap();
    let accept = || Wire::op::<Second>(Op::Accept { pda_seed: None });
    assert_eq!(
        send(OTHER, Some(&config), accept()),
        Err(Refusal::of::<Second>(AuthorityError::NotNominee))
    );
    assert_eq!(send(KEY, Some(&config), accept()), Ok(two(Some(KEY))));
}

#[test]
fn an_accept_with_nothing_offered_is_refused_as_no_nomination() {
    let accept = || Wire::op::<Second>(Op::Accept { pda_seed: None });
    assert_eq!(
        send(KEY, Some(&two(None)), accept()),
        Err(Refusal::of::<Second>(AuthorityError::NoNomination))
    );
    let offer = |next| Wire::op::<Second>(Op::Nominate { next });
    let offered = send(PROGRAM, Some(&two(None)), offer(KEY)).unwrap();
    let cancelled = send(PROGRAM, Some(&offered), offer(AccountId::default())).unwrap();
    assert_eq!(
        send(KEY, Some(&cancelled), accept()),
        Err(Refusal::of::<Second>(AuthorityError::NoNomination))
    );
}

#[test]
fn a_nomination_does_not_revive_a_renounced_slot() {
    let renounced = Two {
        second: AuthoritySlot::Renounced,
        ..two(None)
    };
    assert_eq!(
        send(
            PROGRAM,
            Some(&renounced),
            Wire::op::<Second>(Op::Nominate { next: KEY })
        ),
        Err(Refusal::of::<Second>(AuthorityError::Renounced))
    );
}

#[test]
fn a_renounce_drops_a_pending_nominee() {
    let nominate = Wire::op::<Second>(Op::Nominate { next: OTHER });
    let pending = send(KEY, Some(&two(Some(KEY))), nominate).unwrap();
    let config = send(PROGRAM, Some(&pending), Wire::op::<Second>(Op::Renounce)).unwrap();
    assert_eq!(config, two(None));
    assert_eq!(
        send(
            OTHER,
            Some(&config),
            Wire::op::<Second>(Op::Accept { pda_seed: None })
        ),
        Err(Refusal::of::<Second>(AuthorityError::NoNomination))
    );
}

#[test]
fn a_renounce_withdraws_a_standing_offer() {
    let offer = Wire::op::<Second>(Op::Nominate { next: KEY });
    let offered = send(PROGRAM, Some(&two(None)), offer).unwrap();
    let config = send(PROGRAM, Some(&offered), Wire::op::<Second>(Op::Renounce)).unwrap();
    assert_eq!(config, two(None));
}

#[test]
fn renouncing_the_second_role_leaves_it_for_the_first_to_fill() {
    let config = send(KEY, Some(&two(Some(KEY))), Wire::op::<Second>(Op::Renounce)).unwrap();
    assert_eq!(config, two(None));
    let config = send(PROGRAM, Some(&config), Wire::op::<Second>(Op::Init)).unwrap();
    assert_eq!(config, two(Some(PROGRAM)));
}

#[test]
fn the_first_role_stays_renounced_and_takes_the_second_with_it() {
    let config = send(PROGRAM, Some(&two(None)), Wire::op::<First>(Op::Renounce)).unwrap();
    assert_eq!(config.first, AuthoritySlot::Renounced);
    assert_eq!(
        send(PROGRAM, Some(&config), Wire::op::<First>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::Renounced))
    );
    assert_eq!(
        send(PROGRAM, Some(&config), Wire::op::<Second>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::Renounced))
    );
}

#[test]
fn a_handler_that_changes_the_second_slot_is_refused_by_its_name() {
    let refused = run(
        Some(&two(Some(KEY))),
        config_actor(),
        &[],
        Msg::ClearSecond.into(),
    )
    .err();
    assert_eq!(
        refused,
        Some(Refusal::of::<Second>(AuthorityError::SlotChanged))
    );
}

#[test]
fn an_open_message_runs_where_it_is_sent() {
    let trace = run(
        Some(&two(None)),
        config_actor(),
        &[],
        Msg::SetValue(4).into(),
    )
    .unwrap();
    let at_config = response_at(&trace, config_actor());
    let config: Two = borsh::from_slice(at_config.post_state.as_ref().unwrap()).unwrap();
    assert_eq!(config.value, 4);
    let trace = run(None, seat_of(KEY), &[], Msg::SetValue(6).into()).unwrap();
    let elsewhere = response_at(&trace, seat_of(KEY));
    assert!(elsewhere.sync_messages.is_empty(), "nothing was forwarded");
    let state: Two = borsh::from_slice(elsewhere.post_state.as_ref().unwrap()).unwrap();
    assert_eq!(state.value, 6);
}

#[test]
fn before_init_even_an_open_message_at_the_config_is_refused() {
    assert_eq!(
        run(None, config_actor(), &[], Msg::SetValue(4).into()).err(),
        Some(Refusal::of::<First>(AuthorityError::NotInitialized))
    );
}

#[test]
fn the_config_reaches_another_actor_of_the_program() {
    let target = AccountId::new([7; 32]);
    let trace = run(
        Some(&two(Some(KEY))),
        seat_of(KEY),
        &[KEY],
        Msg::ToTarget(target).into(),
    )
    .unwrap();
    let response = response_at(&trace, config_actor());
    let [sent] = response.sync_messages.as_slice() else {
        panic!("one message to the target");
    };
    assert_eq!(sent.to, Actor::new(target, PROGRAM));
    let at_target = step_at(&trace, sent.to);
    assert_eq!(at_target.input.from, Some(config_actor()));
    assert!(!at_target.input.is_authorized);
    assert_eq!(at_target.response.post_state.as_deref(), Some(&[1u8][..]));
    assert!(at_target.response.sync_messages.is_empty());
}

#[test]
fn a_config_only_message_from_anyone_else_is_refused() {
    let target = Actor::new(AccountId::new([7; 32]), PROGRAM);
    let refused = Some(Refusal::of::<First>(AuthorityError::NotFromConfig));
    let signed = [target.account_id];
    assert_eq!(run(None, target, &signed, Msg::Mark.into()).err(), refused);
    let pass = |to: Actor| {
        Wire::Program(Msg::Pass {
            to: to.account_id,
            bytes: borsh::to_vec(&Wire::Program(Msg::Mark)).unwrap(),
        })
    };
    assert_eq!(
        run(None, seat_of(KEY), &signed, pass(target)).err(),
        refused
    );
    assert_eq!(
        run(Some(&two(None)), config_actor(), &[], pass(config_actor())).err(),
        refused
    );
}

#[test]
fn a_vouched_message_may_call_another_program() {
    let target = AccountId::new([7; 32]);
    let response = through_seat(KEY, Some(&two(Some(KEY))), Msg::ToElsewhere(target).into())
        .ok()
        .unwrap();
    let [sent] = response.sync_messages.as_slice() else {
        panic!("one message to the other program");
    };
    assert_eq!(sent.to, Actor::new(target, ELSEWHERE));
}

#[test]
fn an_operation_sent_straight_to_the_config_is_refused() {
    let nominate = Wire::op::<First>(Op::Nominate { next: KEY });
    assert_eq!(
        run(Some(&two(None)), config_actor(), &[], nominate).err(),
        Some(Refusal::of::<First>(AuthorityError::NotThroughSeat))
    );
}

#[test]
fn an_unsigned_sender_is_refused_at_its_seat() {
    assert_eq!(
        run(None, seat_of(KEY), &[], Wire::op::<First>(Op::Init)).err(),
        Some(Refusal::of::<First>(AuthorityError::MissingSignature))
    );
}

#[test]
fn a_forward_from_another_program_is_refused() {
    let hop = Wire::<Msg>::Library(LibraryMessage::Forwarded(Forwarded {
        via: None,
        inner: borsh::to_vec(&Wire::<Msg>::op::<First>(Op::Init)).unwrap(),
    }));
    let foreign = Actor::new(KEY, ELSEWHERE);
    let through = sent_on(config_actor(), &borsh::to_vec(&hop).unwrap());
    assert_eq!(
        run_bytes(None, foreign, &[], through, &[]).err(),
        Some(Refusal::of::<First>(AuthorityError::NotOwnProgram))
    );
    assert_eq!(
        run(None, seat_of(KEY), &[KEY], hop).err(),
        Some(Refusal::of::<First>(AuthorityError::ForwardedNotAtConfig))
    );
}

#[test]
fn a_handler_cannot_forge_a_hop() {
    let hop = Wire::<Msg>::Library(LibraryMessage::Forwarded(Forwarded {
        via: None,
        inner: borsh::to_vec(&Wire::<Msg>::op::<First>(Op::Nominate { next: OTHER })).unwrap(),
    }));
    let pass = Msg::Pass {
        to: config_actor().account_id,
        bytes: borsh::to_vec(&hop).unwrap(),
    };
    assert_eq!(
        run(None, seat_of(PROGRAM), &[], pass.into()).err(),
        Some(Refusal::of::<First>(AuthorityError::LibrarySend))
    );
}

#[test]
fn a_handler_cannot_forge_a_hop_through_an_async_message() {
    let hop = Wire::<Msg>::Library(LibraryMessage::Forwarded(Forwarded {
        via: None,
        inner: borsh::to_vec(&Wire::<Msg>::op::<First>(Op::Nominate { next: OTHER })).unwrap(),
    }));
    let pass = Msg::PassAsync {
        to: config_actor().account_id,
        bytes: borsh::to_vec(&hop).unwrap(),
    };
    assert_eq!(
        run(None, seat_of(PROGRAM), &[], pass.into()).err(),
        Some(Refusal::of::<First>(AuthorityError::LibrarySend))
    );
}

#[test]
fn a_handler_at_the_config_cannot_send_it_a_library_message() {
    let pass = Msg::Pass {
        to: config_actor().account_id,
        bytes: borsh::to_vec(&Wire::<Msg>::op::<First>(Op::Renounce)).unwrap(),
    };
    assert_eq!(
        run(Some(&two(None)), config_actor(), &[], pass.into()).err(),
        Some(Refusal::of::<First>(AuthorityError::LibrarySend))
    );
}

#[test]
fn an_operation_for_a_role_the_program_lacks_does_not_decode() {
    assert_eq!(
        send(PROGRAM, Some(&two(None)), Wire::op::<Third>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::MessageDoesNotDecode))
    );
}
