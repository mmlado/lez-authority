//! A role's slot and the default rules, through one role.

use super::*;

/// A role that answers to nobody, as the admin does.
struct Hold;

impl Role for Hold {
    const NAME: &'static str = "holder";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<Hold>> Policy<C> for Hold {}

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
fn codes_are_stable() {
    let expected: [(AuthorityError, u16); 26] = [
        (AuthorityError::NotInitialized, 101),
        (AuthorityError::Renounced, 102),
        (AuthorityError::NotHolder, 103),
        (AuthorityError::MissingSignature, 104),
        (AuthorityError::SeedProofFailed, 107),
        (AuthorityError::MessageDoesNotDecode, 108),
        (AuthorityError::ConfigDoesNotDecode, 109),
        (AuthorityError::NoNomination, 113),
        (AuthorityError::NotNominee, 114),
        (AuthorityError::NotGenesis, 115),
        (AuthorityError::NotDeployer, 117),
        (AuthorityError::AlreadyInstalled, 118),
        (AuthorityError::NotOwnProgram, 119),
        (AuthorityError::ViaNotAllowed, 120),
        (AuthorityError::ForwardedNotAtConfig, 121),
        (AuthorityError::NotThroughSeat, 122),
        (AuthorityError::NotFromConfig, 123),
        (AuthorityError::SlotChanged, 126),
        (AuthorityError::NotInstalled, 131),
        (AuthorityError::LibrarySend, 135),
        (AuthorityError::ClearanceFromOutside, 136),
        (AuthorityError::ByNominationOnly, 137),
        (AuthorityError::ClearanceNotAtConfig, 138),
        (AuthorityError::RoleSendsInProgram, 139),
        (AuthorityError::HandlerSendsOwnMessage, 140),
        (AuthorityError::NotOwnActor, 141),
    ];
    for (error, code) in &expected {
        assert_eq!(error.code(), *code, "{error:?}");
        assert!(
            AuthorityError::ALL.contains(error),
            "{error:?} is missing from ALL"
        );
        listed_in_all(error);
    }
    for error in &AuthorityError::ALL {
        assert!(
            expected.iter().any(|(listed, _)| listed == error),
            "{error:?} is missing from the table"
        );
    }
    let mut codes: Vec<u16> = AuthorityError::ALL
        .iter()
        .map(AuthorityError::code)
        .collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(
        codes.len(),
        AuthorityError::ALL.len(),
        "two refusals share a code"
    );
    assert_eq!(
        AuthorityError::NotHolder.text("admin", "admin"),
        "lez-authority/admin E103: the signing account is not the admin"
    );
    assert_eq!(
        Refusal::of::<Hold>(AuthorityError::NotDeployer).text(),
        "lez-authority/holder E117: only the deployer installs the first holder: the program's own account, through its seat at top level"
    );
}

#[test]
fn a_key_holder_acts_at_top_level_only() {
    let slot = held(KEY);
    assert_eq!(slot.check(&caller(KEY, None)), Ok(()));
    assert_eq!(
        slot.check(&caller(KEY, Some(GOVERNANCE))),
        Err(AuthorityError::ViaNotAllowed)
    );
    assert_eq!(
        slot.check(&caller(OTHER, None)),
        Err(AuthorityError::NotHolder)
    );
}

#[test]
fn a_pda_holder_acts_through_its_program_only() {
    let slot = AuthoritySlot::held(Holder {
        account: KEY,
        via: Some(GOVERNANCE),
    });
    assert_eq!(slot.check(&caller(KEY, Some(GOVERNANCE))), Ok(()));
    assert_eq!(
        slot.check(&caller(KEY, None)),
        Err(AuthorityError::ViaNotAllowed)
    );
    assert_eq!(
        slot.check(&caller(KEY, Some(OTHER))),
        Err(AuthorityError::ViaNotAllowed)
    );
}

#[test]
fn unset_and_renounced_slots_refuse_everyone() {
    assert_eq!(
        AuthoritySlot::Unset.check(&caller(KEY, None)),
        Err(AuthorityError::NotInstalled)
    );
    assert_eq!(
        AuthoritySlot::Renounced.check(&caller(KEY, None)),
        Err(AuthorityError::Renounced)
    );
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
fn a_slot_with_an_offer_has_a_nominee_and_no_holder() {
    let offered = AuthoritySlot::Offered { nominee: KEY };
    assert_eq!(offered.holder(), None);
    assert_eq!(offered.nominee(), Some(KEY));
    assert_eq!(
        offered.check(&caller(KEY, None)),
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
fn the_slot_s_existing_encodings_keep_their_bytes() {
    let key = borsh::to_vec(&KEY).unwrap();
    assert_eq!(borsh::to_vec(&AuthoritySlot::Unset).unwrap(), [0]);
    assert_eq!(
        borsh::to_vec(&held(KEY)).unwrap(),
        [&[1][..], &key, &[0, 0]].concat()
    );
    assert_eq!(borsh::to_vec(&AuthoritySlot::Renounced).unwrap(), [2]);
    assert_eq!(
        borsh::to_vec(&AuthoritySlot::Offered { nominee: KEY }).unwrap(),
        [&[3][..], &key].concat()
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

/// Every engine refusal, so a new one does not compile until someone lists it, beside the table
/// and `ALL`.
fn listed_in_all(error: &AuthorityError) {
    match error {
        AuthorityError::NotInitialized
        | AuthorityError::ConfigDoesNotDecode
        | AuthorityError::MessageDoesNotDecode
        | AuthorityError::MissingSignature
        | AuthorityError::NotOwnProgram
        | AuthorityError::ForwardedNotAtConfig
        | AuthorityError::NotThroughSeat
        | AuthorityError::NotInstalled
        | AuthorityError::AlreadyInstalled
        | AuthorityError::NotDeployer
        | AuthorityError::NotGenesis
        | AuthorityError::NoNomination
        | AuthorityError::NotNominee
        | AuthorityError::SeedProofFailed
        | AuthorityError::Renounced
        | AuthorityError::NotHolder
        | AuthorityError::ViaNotAllowed
        | AuthorityError::SlotChanged
        | AuthorityError::NotFromConfig
        | AuthorityError::LibrarySend
        | AuthorityError::ClearanceFromOutside
        | AuthorityError::ByNominationOnly
        | AuthorityError::ClearanceNotAtConfig
        | AuthorityError::RoleSendsInProgram
        | AuthorityError::HandlerSendsOwnMessage
        | AuthorityError::NotOwnActor => {}
        AuthorityError::Role { .. } => panic!("a role crate's refusal is not the engine's"),
    }
}
