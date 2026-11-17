//! The operations a role defines for itself.

use super::*;

// A role whose own operation tries to take its holder part.
struct Seizing;

impl Role for Seizing {
    const NAME: &'static str = "seizing";
    type Slot = Marked;
}

impl<C: HasSlot<Seizing>> Policy<C> for Seizing {
    fn operate(ctx: &Ctx, config: &C, _op: &[u8]) -> Result<Marked, Refusal> {
        let caller = ctx.caller().unwrap();
        Ok(Marked {
            authority_slot: AuthoritySlot::held(Holder {
                account: caller.account(),
                via: caller.via(),
            }),
            mark: HasSlot::<Seizing>::slot(config).mark + 1,
        })
    }
}

impl HasSlot<Seizing> for WithMark {
    fn slot(&self) -> &Marked {
        &self.0
    }
    fn slot_mut(&mut self) -> &mut Marked {
        &mut self.0
    }
}

#[test]
fn a_role_s_own_operation_changes_its_slot_and_nothing_else() {
    let mut expected = root_and_mark();
    expected.marking.mark = 9;
    assert_eq!(
        send_marked(KEY, Some(&root_and_mark()), set_mark_op(9)),
        Ok(expected)
    );
}

#[test]
fn a_role_s_own_operation_is_refused_in_its_name() {
    assert_eq!(
        send_marked(OTHER, Some(&root_and_mark()), set_mark_op(9)),
        Err(Refusal::of::<Marking>(AuthorityError::NotHolder))
    );
    let short = Wire::op::<Marking>(Op::Own(vec![9]));
    assert_eq!(
        send_marked(KEY, Some(&root_and_mark()), short),
        Err(Refusal::of::<Marking>(AuthorityError::MessageDoesNotDecode))
    );
}

#[test]
fn a_role_that_defines_no_operations_refuses_one_as_a_message_that_does_not_decode() {
    assert_eq!(
        send_marked(
            PROGRAM,
            Some(&root_and_mark()),
            Wire::op::<First>(Op::Own(Vec::new()))
        ),
        Err(Refusal::of::<First>(AuthorityError::MessageDoesNotDecode))
    );
    assert_eq!(
        send(
            KEY,
            Some(&two(Some(KEY))),
            Wire::op::<Second>(Op::Own(vec![1]))
        ),
        Err(Refusal::of::<Second>(AuthorityError::MessageDoesNotDecode))
    );
}

#[test]
fn before_the_root_role_s_init_no_operation_of_a_role_s_own_runs() {
    for message in [set_mark_op(9), Wire::op::<First>(Op::Own(Vec::new()))] {
        assert_eq!(
            send_marked(KEY, None, message),
            Err(Refusal::of::<First>(AuthorityError::NotInitialized))
        );
    }
}

#[test]
fn a_role_s_own_operation_sent_straight_to_the_config_is_refused_in_its_name() {
    assert_eq!(
        at_marked(
            config_actor(),
            None,
            false,
            Some(&root_and_mark()),
            set_mark_op(9)
        )
        .err(),
        Some(Refusal::of::<Marking>(AuthorityError::NotThroughSeat))
    );
}

#[test]
fn a_role_s_own_operation_follows_the_four_slot_operations_on_the_wire() {
    assert_eq!(borsh::to_vec(&Op::Init).unwrap(), [0]);
    assert_eq!(
        borsh::to_vec(&Op::Nominate { next: KEY }).unwrap(),
        [&[1][..], &[1; 32]].concat()
    );
    assert_eq!(
        borsh::to_vec(&Op::Accept { pda_seed: None }).unwrap(),
        [2, 0]
    );
    assert_eq!(borsh::to_vec(&Op::Renounce).unwrap(), [3]);
    assert_eq!(borsh::to_vec(&Op::Own(vec![9, 8])).unwrap(), [4, 9, 8]);
    assert_eq!(
        borsh::from_slice::<Op>(&[4, 9, 8]).unwrap(),
        Op::Own(vec![9, 8])
    );
    assert_eq!(borsh::from_slice::<Op>(&[4]).unwrap(), Op::Own(Vec::new()));
    let own = Wire::<u16>::op::<Marking>(Op::Own(vec![9]));
    let bytes = borsh::to_vec(&own).unwrap();
    assert_eq!(bytes.last(), Some(&9));
    assert_eq!(borsh::from_slice::<Wire<u16>>(&bytes).unwrap(), own);
}

#[test]
fn a_role_s_own_operation_never_moves_its_holder() {
    for authority_slot in [
        AuthoritySlot::Unset,
        AuthoritySlot::Offered { nominee: KEY },
        held(KEY),
        AuthoritySlot::Renounced,
    ] {
        let mut config = marked(authority_slot.clone());
        let ctx = Ctx {
            caller: Some(caller(OTHER, Some(ELSEWHERE))),
            program: PROGRAM,
            gate: InitGate::Deployer,
        };
        run_op::<Seizing, WithMark>(&ctx, &mut config, Op::Own(Vec::new())).unwrap();
        assert_eq!(
            config,
            WithMark(Marked {
                authority_slot,
                mark: 8
            })
        );
    }
}
