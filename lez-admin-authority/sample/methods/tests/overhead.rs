//! Cycle counts per hop, from the real guest under the risc0 executor, the way the sequencer
//! runs it. Prints a table; run with `--nocapture`.

use lee::program::Program;
use lee_core::account::{AccountId, Actor, ActorState};
use lee_core::program::ReceiveInput;
use lez_admin_authority::lez_authority::{Forwarded, LibraryMessage};
use lez_admin_authority::{AdminOp, AuthoritySlot, Holder, Wire, admin_op};
use risc0_zkvm::{ExecutorEnv, default_executor};
use sample_core::{Config, Message, config_account};

const PROGRAM: AccountId = AccountId::new([0xA0; 32]);
const DEPLOYER: AccountId = PROGRAM;
const STRANGER: AccountId = AccountId::new([2; 32]);
const BUDGET: u64 = 32 * 1024 * 1024;

fn actor(account: AccountId) -> Actor {
    Actor::new(account, PROGRAM)
}

fn config_actor() -> Actor {
    actor(config_account(PROGRAM))
}

fn input(receiver: Actor, from: Option<Actor>, authorized: bool, pre: Vec<u8>, message: &Wire<Message>) -> ReceiveInput {
    ReceiveInput {
        receiver,
        from,
        is_authorized: authorized,
        pre_state: ActorState::from(pre),
        message: borsh::to_vec(message).unwrap(),
    }
}

fn run(input: &ReceiveInput) -> u64 {
    let mut builder = ExecutorEnv::builder();
    builder.session_limit(Some(BUDGET));
    Program::write_receive_input(input, &mut builder).unwrap();
    let env = builder.build().unwrap();
    let info = default_executor().execute(env, sample_methods::SAMPLE_ELF).expect("the guest runs");
    info.cycles()
}

fn forwarded(message: &Wire<Message>) -> Wire<Message> {
    Wire::Library(LibraryMessage::Forwarded(Forwarded { via: None, inner: borsh::to_vec(message).unwrap() }))
}

fn initialized() -> Vec<u8> {
    borsh::to_vec(&Config {
        open_value: 0,
        admin_value: 0,
        admin: AuthoritySlot::held(Holder { account: DEPLOYER, via: None }),
    })
    .unwrap()
}

#[test]
fn cycles_per_hop() {
    let init: Wire<Message> = admin_op(AdminOp::Init);
    let set_admin: Wire<Message> = Message::SetAdminValue(9).into();
    let set_target: Wire<Message> = Message::SetTargetValue { target: STRANGER, value: 5 }.into();
    let nominate: Wire<Message> = admin_op(AdminOp::Nominate { next: STRANGER });
    let rows: Vec<(&str, ReceiveInput)> = vec![
        ("seat hop, Init", input(actor(DEPLOYER), None, true, vec![], &init)),
        ("config hop, Init installs the admin", input(config_actor(), Some(actor(DEPLOYER)), false, vec![], &forwarded(&init))),
        ("config, SetOpenValue, no hop", input(config_actor(), None, false, initialized(), &Message::SetOpenValue(7).into())),
        ("seat hop, SetAdminValue", input(actor(DEPLOYER), None, true, vec![], &set_admin)),
        ("config hop, SetAdminValue", input(config_actor(), Some(actor(DEPLOYER)), false, initialized(), &forwarded(&set_admin))),
        ("config hop, Nominate", input(config_actor(), Some(actor(DEPLOYER)), false, initialized(), &forwarded(&nominate))),
        ("config hop, SetTargetValue, sends to the target", input(config_actor(), Some(actor(DEPLOYER)), false, initialized(), &forwarded(&set_target))),
        ("target hop, TargetValue", input(actor(STRANGER), Some(config_actor()), false, vec![], &Message::TargetValue(5).into())),
    ];
    println!("\n| Hop | Cycles |\n| --- | ---: |");
    let mut by_name = std::collections::BTreeMap::new();
    for (name, input) in &rows {
        let cycles = run(input);
        println!("| {name} | {cycles} |");
        by_name.insert(*name, cycles);
    }
    let seat = by_name["seat hop, SetAdminValue"];
    println!("\n| Operation | Hops | Cycles |\n| --- | --- | ---: |");
    println!("| open setter | 1 | {} |", by_name["config, SetOpenValue, no hop"]);
    println!("| init | 2 | {} |", by_name["seat hop, Init"] + by_name["config hop, Init installs the admin"]);
    println!("| admin setter | 2 | {} |", seat + by_name["config hop, SetAdminValue"]);
    println!("| nominate | 2 | {} |", seat + by_name["config hop, Nominate"]);
    println!("| target setter | 3 | {} |", seat + by_name["config hop, SetTargetValue, sends to the target"] + by_name["target hop, TargetValue"]);
    println!("\nprogram binary: {} bytes", sample_methods::SAMPLE_ELF.len());
    for cycles in by_name.values() {
        assert!(*cycles > 0 && *cycles < BUDGET);
    }
}
