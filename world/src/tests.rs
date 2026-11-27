use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::program::{AsyncMessage, PdaSeed, SyncMessage};

use super::*;

const PROGRAM: AccountId = AccountId::new([0xA0; 32]);
const OTHER: AccountId = AccountId::new([0xB0; 32]);
const KEY: AccountId = AccountId::new([1; 32]);
const SEED: PdaSeed = PdaSeed::new([7; 32]);

/// What the test program does with one message.
#[derive(Default, BorshSerialize, BorshDeserialize)]
struct Script {
    write: Option<Vec<u8>>,
    send: Vec<(Actor, Option<PdaSeed>, Script)>,
    later: Vec<Actor>,
    fail: bool,
    panic: bool,
}

fn scripted(input: &ReceiveInput) -> Result<Response, &'static str> {
    let script: Script = borsh::from_slice(&input.message).unwrap();
    assert!(!script.panic, "the program panicked");
    if script.fail {
        return Err("refused");
    }
    let mut response = script
        .write
        .map_or_else(Response::keep_state, Response::set_state);
    for (to, seed, next) in script.send {
        response
            .sync_messages
            .push(SyncMessage::new(to, &next).with_pda_seeds(seed));
    }
    for to in script.later {
        response
            .async_messages
            .push(AsyncMessage::new(to, &Script::default()));
    }
    Ok(response)
}

fn world() -> World<&'static str> {
    World::new()
        .program(PROGRAM, scripted)
        .program(OTHER, scripted)
}

fn at(account: AccountId) -> Actor {
    Actor::new(account, PROGRAM)
}

fn write(state: &[u8]) -> Script {
    Script {
        write: Some(state.to_vec()),
        ..Script::default()
    }
}

/// A transaction signed by `KEY` to its actor, listing `actors` beside it.
fn signed(script: &Script, actors: &[Actor]) -> Transaction {
    Transaction {
        to: at(KEY),
        message: borsh::to_vec(script).unwrap(),
        actors: [&[at(KEY)], actors].concat(),
        signers: vec![KEY],
        evidence: Vec::new(),
    }
}

#[test]
fn a_transaction_leaves_its_states_and_traces_every_delivery_in_order() {
    let target = Actor::new(OTHER, OTHER);
    let script = Script {
        send: vec![(target, None, write(&[2]))],
        ..write(&[1])
    };
    let mut world = world().starting_with(target, vec![9]);
    let outcome = world.transact(signed(&script, &[target])).unwrap();
    assert_eq!(world.state(at(KEY)), [1]);
    assert_eq!(world.state(target), [2]);
    let [first, second] = outcome.trace.as_slice() else {
        panic!("two deliveries");
    };
    assert_eq!(first.input.receiver, at(KEY));
    assert_eq!(first.input.from, None);
    assert!(first.input.is_authorized);
    assert_eq!(second.input.receiver, target);
    assert_eq!(second.input.from, Some(at(KEY)));
    assert!(!second.input.is_authorized);
    assert_eq!(&second.input.pre_state[..], [9]);
    assert_eq!(second.response.post_state.as_deref(), Some(&[2u8][..]));
}

#[test]
fn a_program_s_error_leaves_the_world_as_it_was() {
    let target = at(OTHER);
    let script = Script {
        send: vec![(
            target,
            None,
            Script {
                fail: true,
                ..Script::default()
            },
        )],
        ..write(&[1])
    };
    let mut world = world().starting_with(target, vec![9]);
    let refused = world.transact(signed(&script, &[target]));
    assert!(matches!(refused, Err(Failure::Refused("refused"))));
    assert_eq!(world.state(at(KEY)), []);
    assert_eq!(world.actors().collect::<Vec<_>>(), [target]);
}

#[test]
#[should_panic(expected = "the program panicked")]
fn a_program_s_panic_passes_through() {
    let script = Script {
        panic: true,
        ..Script::default()
    };
    let _unreached = world().transact(signed(&script, &[]));
}

#[test]
fn the_executor_refuses_an_actor_the_transaction_does_not_list() {
    let target = at(OTHER);
    let script = Script {
        send: vec![(target, None, write(&[2]))],
        ..Script::default()
    };
    let mut world = world().starting_with(target, vec![9]);
    let refused = world.transact(signed(&script, &[]));
    assert!(matches!(
        refused,
        Err(Failure::Executor(ExecutionError::UndeclaredActor { actor })) if actor == target
    ));
    assert_eq!(world.state(target), [9]);
}

#[test]
fn an_account_is_reached_when_it_holds_state_signed_or_is_vouched_for() {
    let target = at(OTHER);
    let script = Script {
        send: vec![(target, None, write(&[2]))],
        ..Script::default()
    };
    let refused = world().transact(signed(&script, &[target]));
    assert!(matches!(
        refused,
        Err(Failure::Executor(ExecutionError::UnadmittedPublicActor { actor })) if actor == target
    ));

    let mut vouched = world();
    let transaction = Transaction {
        evidence: vec![OTHER],
        ..signed(&script, &[target])
    };
    assert!(vouched.transact(transaction).is_ok());
    assert_eq!(vouched.state(target), [2]);

    let mut holding = world().starting_with(Actor::new(OTHER, OTHER), vec![9]);
    assert!(holding.transact(signed(&script, &[target])).is_ok());

    let mut signing = world();
    let transaction = Transaction {
        signers: vec![KEY, OTHER],
        ..signed(&script, &[target])
    };
    let outcome = signing.transact(transaction).unwrap();
    assert!(outcome.trace[1].input.is_authorized);
}

#[test]
fn a_seed_from_the_sending_program_reaches_and_authorizes_its_pda() {
    let pda = Actor::new(AccountId::for_public_pda(&PROGRAM, &SEED), OTHER);
    let seeded = |seed| Script {
        send: vec![(pda, seed, write(&[2]))],
        ..Script::default()
    };
    let mut world = world();
    let outcome = world.transact(signed(&seeded(Some(SEED)), &[pda])).unwrap();
    assert!(outcome.trace[1].input.is_authorized);
    let mut world = self::world();
    let other_seed = Some(PdaSeed::new([8; 32]));
    for seed in [None, other_seed] {
        let refused = world.transact(signed(&seeded(seed), &[pda]));
        assert!(matches!(
            refused,
            Err(Failure::Executor(
                ExecutionError::UnadmittedPublicActor { .. }
            ))
        ));
    }
}

#[test]
fn an_async_message_comes_back_undelivered() {
    let script = Script {
        later: vec![at(OTHER)],
        ..Script::default()
    };
    let mut world = world();
    let outcome = world.transact(signed(&script, &[])).unwrap();
    assert_eq!(outcome.trace.len(), 1);
    assert_eq!(outcome.async_messages.len(), 1);
}

#[test]
fn an_actor_that_writes_an_empty_state_holds_none() {
    let mut world = world().starting_with(at(KEY), vec![9]);
    let _outcome = world.transact(signed(&write(&[]), &[])).unwrap();
    assert_eq!(world.state(at(KEY)), []);
    assert_eq!(world.actors().count(), 0);
}

#[test]
fn a_delivery_to_a_program_the_world_does_not_hold_fails() {
    let unknown = AccountId::new([0xC0; 32]);
    let stray = Actor::new(KEY, unknown);
    let script = Script {
        send: vec![(stray, None, Script::default())],
        ..write(&[1])
    };
    let mut world = world();
    let refused = world.transact(signed(&script, &[stray]));
    assert!(matches!(refused, Err(Failure::UnknownProgram(id)) if id == unknown));
    assert_eq!(world.state(at(KEY)), []);
}

#[test]
fn an_account_a_transaction_reached_or_signed_for_stays_registered() {
    let target = at(OTHER);
    let reach = |state: &[u8]| Script {
        send: vec![(target, None, write(state))],
        ..Script::default()
    };
    let mut reached = world();
    let vouching = Transaction {
        evidence: vec![OTHER],
        ..signed(&reach(&[]), &[target])
    };
    let _outcome = reached.transact(vouching).unwrap();
    assert_eq!(reached.actors().count(), 0);
    assert!(reached.transact(signed(&reach(&[2]), &[target])).is_ok());

    let mut signed_for = world();
    let signing = Transaction {
        signers: vec![KEY, OTHER],
        ..signed(&Script::default(), &[])
    };
    let _outcome = signed_for.transact(signing).unwrap();
    assert!(signed_for.transact(signed(&reach(&[2]), &[target])).is_ok());

    let mut refused = world();
    let failing = Transaction {
        evidence: vec![OTHER],
        ..signed(
            &Script {
                fail: true,
                ..Script::default()
            },
            &[target],
        )
    };
    assert!(refused.transact(failing).is_err());
    assert!(refused.transact(signed(&reach(&[2]), &[target])).is_err());
}
