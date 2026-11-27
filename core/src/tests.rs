//! The slot's check and operations, and the refusal table.

use lee_core::account::AccountId;
use lee_core::program::PdaSeed;

use crate::*;

const KEY: AccountId = AccountId::new([1; 32]);
const OTHER: AccountId = AccountId::new([2; 32]);
const GOVERNANCE: AccountId = AccountId::new([6; 32]);

/// A role, to name a refusal.
struct Hold;

impl Role for Hold {
    const NAME: &'static str = "holder";
    type Slot = AuthoritySlot;
}

fn key(account: AccountId) -> Holder {
    Holder { account, via: None }
}

fn held(account: AccountId) -> AuthoritySlot {
    AuthoritySlot::held(key(account))
}

fn held_by_key_with(nominee: Option<AccountId>) -> AuthoritySlot {
    AuthoritySlot::Held {
        holder: key(KEY),
        nominee,
    }
}

/// The slot `op` leaves, or its refusal, after which the slot must be as it was.
fn after(
    mut slot: AuthoritySlot,
    op: impl FnOnce(&mut AuthoritySlot) -> Result<(), AuthorityError>,
) -> Result<AuthoritySlot, AuthorityError> {
    let before = slot.clone();
    op(&mut slot).inspect_err(|error| {
        assert_eq!(
            slot, before,
            "the refused operation ({error:?}) changed the slot"
        );
    })?;
    Ok(slot)
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
    assert_eq!(slot.check(&KEY, &None), Ok(()));
    assert_eq!(
        slot.check(&KEY, &Some(GOVERNANCE)),
        Err(AuthorityError::ViaNotAllowed)
    );
    assert_eq!(slot.check(&OTHER, &None), Err(AuthorityError::NotHolder));
    assert_eq!(
        slot.check(&OTHER, &Some(GOVERNANCE)),
        Err(AuthorityError::NotHolder)
    );
}

#[test]
fn a_pda_holder_acts_through_its_program_only() {
    let slot = AuthoritySlot::held(Holder {
        account: KEY,
        via: Some(GOVERNANCE),
    });
    assert_eq!(slot.check(&KEY, &Some(GOVERNANCE)), Ok(()));
    assert_eq!(slot.check(&KEY, &None), Err(AuthorityError::ViaNotAllowed));
    assert_eq!(
        slot.check(&KEY, &Some(OTHER)),
        Err(AuthorityError::ViaNotAllowed)
    );
}

#[test]
fn unset_and_renounced_slots_refuse_everyone() {
    assert_eq!(
        AuthoritySlot::Unset.check(&KEY, &None),
        Err(AuthorityError::NotInstalled)
    );
    assert_eq!(
        AuthoritySlot::Renounced.check(&KEY, &None),
        Err(AuthorityError::Renounced)
    );
}

#[test]
fn a_slot_with_an_offer_has_a_nominee_and_no_holder() {
    let offered = AuthoritySlot::Offered { nominee: KEY };
    assert_eq!(offered.holder(), None);
    assert_eq!(offered.nominee(), Some(KEY));
    assert_eq!(
        offered.check(&KEY, &None),
        Err(AuthorityError::NotInstalled)
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
fn install_takes_a_slot_with_no_holder_and_keeps_a_standing_offer() {
    assert_eq!(
        after(AuthoritySlot::Unset, |slot| slot.install(key(KEY))),
        Ok(held(KEY))
    );
    assert_eq!(
        after(AuthoritySlot::Offered { nominee: OTHER }, |slot| {
            slot.install(key(KEY))
        }),
        Ok(held_by_key_with(Some(OTHER)))
    );
    let pda = Holder {
        account: KEY,
        via: Some(GOVERNANCE),
    };
    assert_eq!(
        after(AuthoritySlot::Unset, |slot| slot.install(pda)),
        Ok(AuthoritySlot::held(pda))
    );
}

#[test]
fn install_refuses_a_held_or_renounced_slot() {
    assert_eq!(
        after(held_by_key_with(Some(OTHER)), |slot| slot
            .install(key(OTHER))),
        Err(AuthorityError::AlreadyInstalled)
    );
    assert_eq!(
        after(AuthoritySlot::Renounced, |slot| slot.install(key(KEY))),
        Err(AuthorityError::Renounced)
    );
}

#[test]
fn nominate_records_replaces_and_withdraws_the_holder_s_offer() {
    let slot = after(held(KEY), |slot| slot.nominate(Some(OTHER))).unwrap();
    assert_eq!(slot, held_by_key_with(Some(OTHER)));
    let slot = after(slot, |slot| slot.nominate(Some(GOVERNANCE))).unwrap();
    assert_eq!(slot, held_by_key_with(Some(GOVERNANCE)));
    assert_eq!(after(slot, |slot| slot.nominate(None)), Ok(held(KEY)));
}

#[test]
fn nominate_without_a_holder_makes_replaces_and_withdraws_a_standing_offer() {
    let slot = after(AuthoritySlot::Unset, |slot| slot.nominate(Some(KEY))).unwrap();
    assert_eq!(slot, AuthoritySlot::Offered { nominee: KEY });
    let slot = after(slot, |slot| slot.nominate(Some(OTHER))).unwrap();
    assert_eq!(slot, AuthoritySlot::Offered { nominee: OTHER });
    assert_eq!(
        after(slot, |slot| slot.nominate(None)),
        Ok(AuthoritySlot::Unset)
    );
    assert_eq!(
        after(AuthoritySlot::Unset, |slot| slot.nominate(None)),
        Ok(AuthoritySlot::Unset)
    );
}

#[test]
fn nominate_refuses_a_renounced_slot() {
    for next in [Some(KEY), None] {
        assert_eq!(
            after(AuthoritySlot::Renounced, |slot| slot.nominate(next)),
            Err(AuthorityError::Renounced)
        );
    }
}

#[test]
fn a_key_nominee_accepts_at_top_level_and_the_offer_ends() {
    assert_eq!(
        after(held_by_key_with(Some(OTHER)), |slot| {
            slot.accept(key(OTHER), None)
        }),
        Ok(held(OTHER))
    );
    assert_eq!(
        after(AuthoritySlot::Offered { nominee: OTHER }, |slot| {
            slot.accept(key(OTHER), None)
        }),
        Ok(held(OTHER))
    );
}

#[test]
fn a_pda_nominee_accepts_through_its_program_with_the_seed() {
    let seed = PdaSeed::new([9; 32]);
    let pda = Holder {
        account: AccountId::for_public_pda(&GOVERNANCE, &seed),
        via: Some(GOVERNANCE),
    };
    assert_eq!(
        after(held_by_key_with(Some(pda.account)), |slot| {
            slot.accept(pda, Some(seed))
        }),
        Ok(AuthoritySlot::held(pda))
    );
}

#[test]
fn accept_refuses_in_order_renounced_no_nomination_and_not_nominee() {
    assert_eq!(
        after(AuthoritySlot::Renounced, |slot| slot
            .accept(key(OTHER), None)),
        Err(AuthorityError::Renounced)
    );
    assert_eq!(
        after(AuthoritySlot::Unset, |slot| slot.accept(key(OTHER), None)),
        Err(AuthorityError::NoNomination)
    );
    assert_eq!(
        after(held(KEY), |slot| slot.accept(key(KEY), None)),
        Err(AuthorityError::NoNomination)
    );
    // A stranger whose seed would fail too, so the nominee is compared first.
    let stranger = Holder {
        account: GOVERNANCE,
        via: Some(KEY),
    };
    assert_eq!(
        after(held_by_key_with(Some(OTHER)), |slot| slot
            .accept(stranger, None)),
        Err(AuthorityError::NotNominee)
    );
}

#[test]
fn a_nominee_whose_seed_does_not_derive_it_is_refused() {
    let seed = PdaSeed::new([9; 32]);
    let pda = AccountId::for_public_pda(&GOVERNANCE, &seed);
    let through = |via| Holder {
        account: pda,
        via: Some(via),
    };
    for (by, pda_seed) in [
        (through(GOVERNANCE), Some(PdaSeed::new([8; 32]))),
        (through(GOVERNANCE), None),
        (through(OTHER), Some(seed)),
        (key(pda), Some(seed)),
    ] {
        assert_eq!(
            after(held_by_key_with(Some(pda)), |slot| slot
                .accept(by, pda_seed)),
            Err(AuthorityError::SeedProofFailed),
            "{by:?} with {pda_seed:?}"
        );
    }
}

#[test]
fn renounce_leaves_the_slot_it_is_given() {
    for left in [AuthoritySlot::Renounced, AuthoritySlot::Unset] {
        let mut slot = held_by_key_with(Some(OTHER));
        slot.renounce(left.clone());
        assert_eq!(slot, left);
    }
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
