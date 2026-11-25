//! A role's slot and the default rules, through one role.

use super::*;

/// A role that answers to nobody, as the admin does.
struct Hold;

impl Role for Hold {
    const NAME: &'static str = "holder";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<Hold>> Policy<C> for Hold {}
impl<C: HasSlot<Hold>> Hooks<C> for Hold {}

struct One(AuthoritySlot);

impl HasSlot<Hold> for One {
    fn slot(&self) -> &AuthoritySlot {
        &self.0
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.0
    }
}

/// The slot `op` leaves.
fn run_with(
    caller: Option<Caller>,
    gate: InitGate,
    slot: AuthoritySlot,
    op: Op,
) -> Result<AuthoritySlot, AuthorityError> {
    let mut config = One(slot);
    let ctx = Ctx {
        caller,
        config: config_actor(),
        gate,
    };
    run_op::<Hold, One>(&ctx, &mut config, op).map_err(|refusal| {
        assert_eq!(refusal.role, "holder");
        refusal.error
    })?;
    Ok(config.0)
}

fn run(
    caller: Option<Caller>,
    slot: AuthoritySlot,
    op: Op,
) -> Result<AuthoritySlot, AuthorityError> {
    run_with(caller, InitGate::Deployer, slot, op)
}

#[test]
fn the_deployer_installs_itself_and_the_slot_must_stay_that_way() {
    assert_eq!(
        run(Some(caller(PROGRAM, None)), AuthoritySlot::Unset, Op::Init),
        Ok(held(PROGRAM))
    );
}

#[test]
fn another_account_cannot_install() {
    assert_eq!(
        run(Some(caller(KEY, None)), AuthoritySlot::Unset, Op::Init),
        Err(AuthorityError::NotDeployer)
    );
}

#[test]
fn the_deployer_reached_through_a_program_cannot_install() {
    assert_eq!(
        run(
            Some(caller(PROGRAM, Some(GOVERNANCE))),
            AuthoritySlot::Unset,
            Op::Init
        ),
        Err(AuthorityError::NotDeployer)
    );
}

#[test]
fn with_a_genesis_key_only_that_key_installs() {
    assert_eq!(
        run_with(
            Some(caller(KEY, None)),
            InitGate::Genesis(KEY),
            AuthoritySlot::Unset,
            Op::Init
        ),
        Ok(held(KEY))
    );
}

#[test]
fn with_a_genesis_key_the_deployer_is_refused() {
    assert_eq!(
        run_with(
            Some(caller(PROGRAM, None)),
            InitGate::Genesis(KEY),
            AuthoritySlot::Unset,
            Op::Init
        ),
        Err(AuthorityError::NotGenesis)
    );
}

fn held_by_key_with(nominee: Option<AccountId>) -> AuthoritySlot {
    AuthoritySlot::Held {
        holder: Holder {
            account: KEY,
            via: None,
        },
        nominee,
    }
}

#[test]
fn nominate_records_replaces_and_cancels() {
    let by_key = || Some(caller(KEY, None));
    let slot = run(
        by_key(),
        held_by_key_with(None),
        Op::Nominate { next: OTHER },
    )
    .unwrap();
    assert_eq!(slot.nominee(), Some(OTHER));
    let slot = run(by_key(), slot, Op::Nominate { next: GOVERNANCE }).unwrap();
    assert_eq!(slot.nominee(), Some(GOVERNANCE));
    let slot = run(
        by_key(),
        slot,
        Op::Nominate {
            next: AccountId::default(),
        },
    )
    .unwrap();
    assert_eq!(slot, held(KEY));
}

#[test]
fn only_the_holder_nominates() {
    assert_eq!(
        run(
            Some(caller(OTHER, None)),
            held_by_key_with(None),
            Op::Nominate { next: OTHER }
        ),
        Err(AuthorityError::NotHolder)
    );
}

#[test]
fn a_key_nominee_accepts_at_top_level() {
    assert_eq!(
        run(
            Some(caller(OTHER, None)),
            held_by_key_with(Some(OTHER)),
            Op::Accept { pda_seed: None }
        ),
        Ok(held(OTHER))
    );
}

#[test]
fn a_pda_nominee_accepts_through_its_program_with_the_seed() {
    let seed = PdaSeed::new([9; 32]);
    let pda = AccountId::for_public_pda(&GOVERNANCE, &seed);
    assert_eq!(
        run(
            Some(caller(pda, Some(GOVERNANCE))),
            held_by_key_with(Some(pda)),
            Op::Accept {
                pda_seed: Some(seed)
            }
        ),
        Ok(AuthoritySlot::held(Holder {
            account: pda,
            via: Some(GOVERNANCE)
        }))
    );
}

#[test]
fn a_pda_nominee_with_the_wrong_seed_is_refused() {
    let pda = AccountId::for_public_pda(&GOVERNANCE, &PdaSeed::new([9; 32]));
    assert_eq!(
        run(
            Some(caller(pda, Some(GOVERNANCE))),
            held_by_key_with(Some(pda)),
            Op::Accept {
                pda_seed: Some(PdaSeed::new([8; 32]))
            }
        ),
        Err(AuthorityError::SeedProofFailed)
    );
}

#[test]
fn a_nominee_reached_through_a_program_without_a_seed_is_refused() {
    assert_eq!(
        run(
            Some(caller(OTHER, Some(GOVERNANCE))),
            held_by_key_with(Some(OTHER)),
            Op::Accept { pda_seed: None }
        ),
        Err(AuthorityError::SeedProofFailed)
    );
}

#[test]
fn only_the_nominee_accepts() {
    assert_eq!(
        run(
            Some(caller(GOVERNANCE, None)),
            held_by_key_with(Some(OTHER)),
            Op::Accept { pda_seed: None }
        ),
        Err(AuthorityError::NotNominee)
    );
}

#[test]
fn accept_without_a_nomination_is_refused() {
    assert_eq!(
        run(
            Some(caller(OTHER, None)),
            held_by_key_with(None),
            Op::Accept { pda_seed: None }
        ),
        Err(AuthorityError::NoNomination)
    );
}

#[test]
fn accept_after_renounce_is_refused() {
    let slot = run(
        Some(caller(KEY, None)),
        held_by_key_with(Some(OTHER)),
        Op::Renounce,
    );
    assert_eq!(slot, Ok(AuthoritySlot::Renounced));
    assert_eq!(
        run(
            Some(caller(OTHER, None)),
            AuthoritySlot::Renounced,
            Op::Accept { pda_seed: None }
        ),
        Err(AuthorityError::Renounced)
    );
}

#[test]
fn the_default_policy_makes_no_offer_on_a_slot_with_no_holder() {
    assert_eq!(
        run(
            Some(caller(PROGRAM, None)),
            AuthoritySlot::Unset,
            Op::Nominate { next: KEY }
        ),
        Err(AuthorityError::NotInstalled)
    );
}

#[test]
fn install_keeps_a_standing_offer() {
    assert_eq!(
        run(
            Some(caller(PROGRAM, None)),
            AuthoritySlot::Offered { nominee: KEY },
            Op::Init
        ),
        Ok(AuthoritySlot::Held {
            holder: Holder {
                account: PROGRAM,
                via: None,
            },
            nominee: Some(KEY),
        })
    );
}

#[test]
fn install_refuses_a_held_slot() {
    assert_eq!(
        run(Some(caller(PROGRAM, None)), held(KEY), Op::Init),
        Err(AuthorityError::AlreadyInstalled)
    );
}

#[test]
fn the_wire_keeps_the_program_s_encoding_and_marks_the_library_s() {
    let program: Wire<u16> = Wire::Program(7);
    assert_eq!(
        borsh::to_vec(&program).unwrap(),
        borsh::to_vec(&7u16).unwrap()
    );
    let op: Wire<u16> = Wire::op::<Hold>(Op::Init);
    let bytes = borsh::to_vec(&op).unwrap();
    assert_eq!(bytes[0], LIBRARY_TAG);
    assert_eq!(borsh::from_slice::<Wire<u16>>(&bytes).unwrap(), op);
    assert_eq!(
        borsh::from_slice::<Wire<u16>>(&borsh::to_vec(&7u16).unwrap()).unwrap(),
        program
    );
}
