//! Roles that keep more than their holder, the names refusals print, and roles appointed only by nomination.

use super::*;

// A role whose holder is only ever appointed by nomination, named in prose as the freeze
// authority is.

struct Deputy;

impl Role for Deputy {
    const NAME: &'static str = "deputy";
    const PROSE_NAME: &'static str = "deputy authority";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<Deputy>> Policy<C> for Deputy {
    const BY_NOMINATION_ONLY: bool = true;
}

impl<C: HasSlot<Deputy>> Hooks<C> for Deputy {}

fn mark_op(config: &mut WithMark, account: AccountId, op: Op) {
    let ctx = Ctx {
        caller: Some(caller(account, None)),
        config: config_actor(),
        gate: InitGate::Deployer,
    };
    run_op::<Marking, WithMark>(&ctx, config, op).unwrap();
}

#[test]
fn operations_change_only_the_holder_part_of_a_role_s_own_slot() {
    let mut config = marked(AuthoritySlot::Unset);
    mark_op(&mut config, PROGRAM, Op::Init);
    mark_op(&mut config, PROGRAM, Op::Nominate { next: KEY });
    mark_op(&mut config, KEY, Op::Accept { pda_seed: None });
    assert_eq!(config, marked(held(KEY)));
    mark_op(&mut config, KEY, Op::Renounce);
    assert_eq!(config, marked(AuthoritySlot::Renounced));
}

/// Writes the mark, a part of the role's slot that the handler must leave alone.
fn set_mark(input: &ReceiveInput, message: SetMark) -> Response {
    let mut config: WithMark = borsh::from_slice(&input.pre_state).unwrap();
    config.0.mark = message.0;
    Response::set_state(borsh::to_vec(&config).unwrap())
}

#[test]
fn a_handler_that_changes_the_rest_of_a_role_s_own_slot_is_refused() {
    let pre = marked(held(PROGRAM));
    assert_eq!(at_config_with::<(Marking,), _>(&pre, 7, set_mark), None);
    assert_eq!(
        at_config_with::<(Marking,), _>(&pre, 8, set_mark),
        Some(Refusal::of::<Marking>(AuthorityError::SlotChanged))
    );
}

#[test]
fn a_handler_that_changes_the_rest_of_the_second_role_s_slot_is_refused_by_its_name() {
    let pre = RootAndMark {
        first: held(PROGRAM),
        marking: Marked {
            authority_slot: held(KEY),
            mark: 7,
        },
    };
    assert_eq!(
        at_config_with::<(First, Marking), _>(&pre, 7, set_second_mark),
        None
    );
    assert_eq!(
        at_config_with::<(First, Marking), _>(&pre, 8, set_second_mark),
        Some(Refusal::of::<Marking>(AuthorityError::SlotChanged))
    );
}

fn replace_first(input: &ReceiveInput, _message: SetMark) -> Response {
    let mut config: RootAndMark = borsh::from_slice(&input.pre_state).unwrap();
    config.first = held(OTHER);
    Response::set_state(borsh::to_vec(&config).unwrap())
}

#[test]
fn a_handler_that_replaces_the_first_role_s_holder_is_refused_by_its_name() {
    let pre = RootAndMark {
        first: held(PROGRAM),
        marking: Marked {
            authority_slot: held(KEY),
            mark: 7,
        },
    };
    assert_eq!(
        at_config_with::<(First, Marking), _>(&pre, 7, replace_first),
        Some(Refusal::of::<First>(AuthorityError::SlotChanged))
    );
}

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct FirstAndDeputy {
    first: AuthoritySlot,
    deputy: AuthoritySlot,
}

impl HasSlot<First> for FirstAndDeputy {
    fn slot(&self) -> &AuthoritySlot {
        &self.first
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.first
    }
}

impl HasSlot<Deputy> for FirstAndDeputy {
    fn slot(&self) -> &AuthoritySlot {
        &self.deputy
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.deputy
    }
}

#[test]
fn a_refusal_names_the_role_by_name_in_its_prefix_and_in_prose_in_its_sentence() {
    assert_eq!(
        Refusal::of::<Deputy>(AuthorityError::NotHolder).text(),
        "lez-authority/deputy E103: the signing account is not the deputy authority"
    );
    assert_eq!(
        Refusal::of::<Deputy>(AuthorityError::ByNominationOnly).text(),
        "lez-authority/deputy E137: the deputy authority is appointed by nomination, never installed"
    );
}

#[test]
fn a_role_s_own_refusal_prints_with_its_crate_s_prefix() {
    let own = AuthorityError::Role {
        krate: "lez-deputy-authority",
        code: 208,
        sentence: "the sender is neither the deputy authority nor the first",
    };
    assert_eq!(own.code(), 208);
    assert_eq!(
        Refusal::of::<Deputy>(own).text(),
        "lez-deputy-authority/deputy E208: the sender is neither the deputy authority nor the first"
    );
}

#[test]
fn a_role_appointed_only_by_nomination_refuses_every_init() {
    for deputy in [
        AuthoritySlot::Unset,
        AuthoritySlot::Offered { nominee: KEY },
        held(OTHER),
        AuthoritySlot::Renounced,
    ] {
        for who in [
            caller(PROGRAM, None),
            caller(KEY, None),
            caller(PROGRAM, Some(ELSEWHERE)),
        ] {
            let ctx = Ctx {
                caller: Some(who),
                config: config_actor(),
                gate: InitGate::Deployer,
            };
            let before = || FirstAndDeputy {
                first: held(PROGRAM),
                deputy: deputy.clone(),
            };
            let mut config = before();
            assert_eq!(
                run_op::<Deputy, FirstAndDeputy>(&ctx, &mut config, Op::Init),
                Err(Refusal::of::<Deputy>(AuthorityError::ByNominationOnly))
            );
            assert_eq!(config, before());
        }
    }
}

#[test]
fn a_role_appointed_only_by_nomination_may_be_the_second_role() {
    let pre = FirstAndDeputy {
        first: held(PROGRAM),
        deputy: AuthoritySlot::Unset,
    };
    assert_eq!(at_config_with::<(First, Deputy), _>(&pre, 0, keep), None);
}

#[test]
fn a_one_role_guard_names_its_root_in_prose() {
    let op = Wire::<SetMark>::op::<First>(Op::Renounce);
    let input = ReceiveInput {
        receiver: config_actor(),
        from: None,
        is_authorized: false,
        pre_state: ActorState::from(Vec::new()),
        message: borsh::to_vec(&op).unwrap(),
    };
    let refused =
        try_guard::<(First,), RootAndMark, SetMark>(&input, &SEED, InitGate::Deployer, op, keep)
            .err()
            .unwrap();
    assert_eq!(
        refused,
        Refusal::of::<First>(AuthorityError::NotThroughSeat)
    );
    assert_eq!(
        refused.text(),
        "lez-authority/first E122: a message that needs the first role must go through the sender's own actor"
    );
}

#[test]
fn an_operation_of_the_second_role_sent_straight_to_the_config_names_that_role() {
    let op = Wire::<SetMark>::op::<Deputy>(Op::Nominate { next: KEY });
    let input = ReceiveInput {
        receiver: config_actor(),
        from: None,
        is_authorized: false,
        pre_state: ActorState::from(borsh::to_vec(&FirstAndDeputy::default()).unwrap()),
        message: borsh::to_vec(&op).unwrap(),
    };
    let refused = try_guard::<(First, Deputy), FirstAndDeputy, SetMark>(
        &input,
        &SEED,
        InitGate::Deployer,
        op,
        keep,
    )
    .err();
    assert_eq!(
        refused,
        Some(Refusal::of::<Deputy>(AuthorityError::NotThroughSeat))
    );
}

#[test]
fn a_slot_change_names_the_slot_by_the_role_s_name() {
    assert_eq!(
        Refusal::of::<Deputy>(AuthorityError::SlotChanged).text(),
        "lez-authority/deputy E126: a handler must not change the deputy slot"
    );
    assert_eq!(
        Refusal::of::<Deputy>(AuthorityError::NotInstalled).text(),
        "lez-authority/deputy E131: there is no deputy authority"
    );
}
