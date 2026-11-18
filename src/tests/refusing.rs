//! A role that refuses program messages at the config, and the messages exempt from it.

use super::*;

fn never(_input: &ReceiveInput, _message: SetMark) -> Response {
    unreachable!("a refused message never reaches the handler")
}

/// `message` sent straight to the config.
fn straight(pre: &RootAndMark, message: Halt) -> Result<Response, Refusal> {
    at_marked_with(
        config_actor(),
        None,
        false,
        Some(pre),
        Wire::Program(message),
        halt,
    )
}

/// `message` from `sender`'s seat to the config.
fn halt_through_seat(
    sender: AccountId,
    pre: &RootAndMark,
    message: Halt,
) -> Result<Response, Refusal> {
    let hop = at_marked_with(
        seat_of(sender),
        None,
        true,
        None,
        Wire::Program(message),
        halt,
    )?;
    let hop: Wire<Halt> = borsh::from_slice(&hop.sync_messages[0].message).unwrap();
    at_marked_with(
        config_actor(),
        Some(seat_of(sender)),
        false,
        Some(pre),
        hop,
        halt,
    )
}

#[test]
fn a_role_s_rules_refuse_a_program_message_at_the_config_in_its_name() {
    let refused = Some(Refusal::of::<Marking>(HALTED));
    assert_eq!(straight(&halted(), Halt::Open).err(), refused);
    assert_eq!(
        halt_through_seat(PROGRAM, &halted(), Halt::Gated).err(),
        refused
    );
    assert!(straight(&root_and_mark(), Halt::Open).is_ok());
    assert!(halt_through_seat(PROGRAM, &root_and_mark(), Halt::Gated).is_ok());
    assert_eq!(
        Refusal::of::<Marking>(HALTED).text(),
        "lez-marking-authority/marking E201: the mark is zero"
    );
}

#[test]
fn a_message_exempt_from_a_role_runs_where_its_rules_would_refuse_it() {
    assert!(straight(&halted(), Halt::Exempt).is_ok());
    assert!(halt_through_seat(PROGRAM, &halted(), Halt::GatedExempt).is_ok());
}

#[test]
fn an_exempt_message_still_needs_its_holder_and_leaves_the_slots_alone() {
    assert_eq!(
        halt_through_seat(OTHER, &halted(), Halt::GatedExempt).err(),
        Some(Refusal::of::<First>(AuthorityError::NotHolder))
    );
    assert_eq!(
        straight(&halted(), Halt::ExemptTouch).err(),
        Some(Refusal::of::<Marking>(AuthorityError::SlotChanged))
    );
}

#[test]
fn the_holders_come_before_a_role_s_rules() {
    assert_eq!(
        halt_through_seat(OTHER, &halted(), Halt::Gated).err(),
        Some(Refusal::of::<First>(AuthorityError::NotHolder))
    );
}

#[test]
fn a_role_s_rules_never_see_a_management_operation() {
    let mut expected = halted();
    expected.marking.mark = 9;
    assert_eq!(
        send_marked(KEY, Some(&halted()), set_mark_op(9)),
        Ok(expected)
    );
    let nominate = Wire::op::<First>(Op::Nominate { next: OTHER });
    let nominated = send_marked(PROGRAM, Some(&halted()), nominate).unwrap();
    assert_eq!(nominated.first.nominee(), Some(OTHER));
}

#[test]
fn before_init_an_exempt_message_at_the_config_is_refused_too() {
    assert_eq!(
        at_marked_with(
            config_actor(),
            None,
            false,
            None,
            Wire::Program(Halt::Exempt),
            halt
        )
        .err(),
        Some(Refusal::of::<First>(AuthorityError::NotInitialized))
    );
}

#[test]
fn a_role_s_rules_answer_before_the_handler_runs() {
    let refused = Some(Refusal::of::<Marking>(HALTED));
    assert_eq!(
        at_config_with::<(First, Marking), _>(&halted(), 7, never),
        refused
    );
    assert_eq!(
        at_config_with::<(First, Marking), _>(&halted(), 8, set_second_mark),
        refused
    );
}

#[test]
fn an_exempt_message_at_a_config_that_does_not_decode_is_refused() {
    let message = Wire::Program(Halt::Exempt);
    let input = ReceiveInput {
        receiver: config_actor(),
        from: None,
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

/// `message` sent straight to the config of a program with the roles `Rs`.
fn ask<Rs: Roles<Marks, Ask>>(pre: &Marks, message: Ask) -> Result<Response, Refusal> {
    at_with::<Rs, Marks, Ask>(
        config_actor(),
        None,
        false,
        Some(pre),
        Wire::Program(message),
        keep,
    )
}

/// `message` from `sender`'s seat to the config of a program with the roles `Both`.
fn ask_through_seat<Both: Roles<Marks, Ask>>(
    sender: AccountId,
    pre: &Marks,
    message: Ask,
) -> Result<Response, Refusal> {
    let hop = at_with::<Both, Marks, Ask>(
        seat_of(sender),
        None,
        true,
        None,
        Wire::Program(message),
        keep,
    )?;
    let hop: Wire<Ask> = borsh::from_slice(&hop.sync_messages[0].message).unwrap();
    at_with::<Both, Marks, Ask>(
        config_actor(),
        Some(seat_of(sender)),
        false,
        Some(pre),
        hop,
        keep,
    )
}

#[test]
fn a_lone_role_s_rules_refuse_every_program_message_but_an_exempt_one() {
    assert_eq!(
        ask::<(Marking,)>(&marks(0, 7), Ask::Open).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
    assert!(ask::<(Marking,)>(&marks(0, 7), Ask::ExemptMarking).is_ok());
    assert!(ask::<(Marking,)>(&marks(7, 0), Ask::Open).is_ok());
}

#[test]
fn rules_a_role_does_not_declare_are_never_asked() {
    assert!(ask::<(Halting,)>(&marks(7, 0), Ask::Open).is_ok());
    assert!(ask::<(Marking, Halting)>(&marks(7, 0), Ask::Open).is_ok());
    assert_eq!(
        ask::<(Marking, Halting)>(&marks(0, 0), Ask::Open).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
    assert_eq!(
        ask::<(Halting, Marking)>(&marks(0, 0), Ask::Open).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
}

#[test]
fn an_exemption_from_another_role_lifts_nothing() {
    assert_eq!(
        ask::<(Marking, Halting)>(&marks(0, 0), Ask::ExemptHalting).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
    assert!(ask::<(Marking, Halting)>(&marks(0, 0), Ask::ExemptMarking).is_ok());
}

#[test]
fn a_role_s_rules_refuse_its_own_holder_too() {
    assert_eq!(
        ask_through_seat::<(Marking, Halting)>(KEY, &marks(0, 7), Ask::GatedMarking).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
    assert!(ask_through_seat::<(Marking, Halting)>(KEY, &marks(7, 7), Ask::GatedMarking).is_ok());
}

#[test]
fn a_role_s_rules_see_who_sent_a_message_through_its_seat() {
    let mut held_by_other = marks(7, 7);
    held_by_other.marking.authority_slot = held(OTHER);
    assert_eq!(
        ask_through_seat::<(Marking, Halting)>(OTHER, &held_by_other, Ask::GatedMarking).err(),
        Some(Refusal::of::<Marking>(BARRED))
    );
    assert_eq!(
        ask_through_seat::<(Halting, Marking)>(OTHER, &held_by_other, Ask::GatedMarking).err(),
        Some(Refusal::of::<Marking>(BARRED))
    );
    assert!(ask::<(Marking, Halting)>(&held_by_other, Ask::Open).is_ok());
}

#[test]
fn a_marked_role_is_listed_only_under_its_exact_name() {
    assert!(listed(&["admin", "freeze"], &["freeze", "admin"]));
    assert!(listed(&["admin"], &[]));
    assert!(!listed(&["admin"], &["freeze"]));
    assert!(!listed(&["admin"], &["admi"]));
    assert!(!listed(&["admin"], &["admins"]));
    assert!(!listed(&["admin", "freeze"], &["admin", "pauser"]));
}
