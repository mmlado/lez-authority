//! Cycle counts per hop, from the real guest under the risc0 executor, the way the sequencer
//! runs it, and the bytes a private sender's request for clearance adds. Prints tables, so run
//! it with `--nocapture`.

use std::collections::{BTreeMap, BTreeSet};

use freeze_sample_core::{Balance, Config, Message, NOTICE_MAX, config_account};
use lee::program::Program;
use lee_core::account::{AccountId, Actor, ActorState};
use lee_core::execution_state::{BoundaryStep, Delivery, PublicExecutionContext};
use lee_core::program::{MessageEnvelope, ReceiveInput};
use lez_freeze_authority::lez_authority::{Clearance, Forwarded, LibraryMessage};
use lez_freeze_authority::{AuthoritySlot, Holder, Slot, Wire};
use risc0_zkvm::{ExecutorEnv, default_executor};

const PROGRAM: AccountId = AccountId::new([0xA0; 32]);
const DEPLOYER: AccountId = PROGRAM;
const AUTHORITY: AccountId = AccountId::new([1; 32]);
const OWNER: AccountId = AccountId::new([2; 32]);
const RECIPIENT: AccountId = AccountId::new([3; 32]);
const BUDGET: u64 = 32 * 1024 * 1024;

fn actor(account: AccountId) -> Actor {
    Actor::new(account, PROGRAM)
}

fn config_actor() -> Actor {
    actor(config_account(PROGRAM))
}

fn input(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: Vec<u8>,
    message: &Wire<Message>,
) -> ReceiveInput {
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
    let info = default_executor()
        .execute(env, freeze_sample_methods::FREEZE_SAMPLE_ELF)
        .expect("the guest runs");
    info.cycles()
}

fn forwarded(message: &Wire<Message>) -> Wire<Message> {
    Wire::Library(LibraryMessage::Forwarded(Forwarded {
        via: None,
        inner: borsh::to_vec(message).unwrap(),
    }))
}

fn clearance(authorized: bool) -> Wire<Message> {
    Wire::Library(LibraryMessage::Clearance(Clearance { authorized }))
}

fn held(account: AccountId) -> AuthoritySlot {
    AuthoritySlot::held(Holder { account, via: None })
}

/// The config as the live run leaves it before the freeze: the deployer the admin, the
/// authority the freeze authority, not frozen.
fn appointed() -> Vec<u8> {
    with_notice("")
}

/// [`appointed`] with `notice` posted.
fn with_notice(notice: &str) -> Vec<u8> {
    let mut config = Config {
        admin: held(DEPLOYER),
        notice: notice.to_owned(),
        ..Config::default()
    };
    *config.freeze.authority_slot_mut() = held(AUTHORITY);
    borsh::to_vec(&config).unwrap()
}

fn balance(amount: u64) -> Vec<u8> {
    borsh::to_vec(&Balance { amount }).unwrap()
}

#[test]
fn cycles_per_hop() {
    let mint: Wire<Message> = Message::Mint {
        to: OWNER,
        amount: 5,
    }
    .into();
    let transfer: Wire<Message> = Message::Transfer {
        to: RECIPIENT,
        amount: 3,
    }
    .into();
    let rows: Vec<(&str, ReceiveInput)> = vec![
        (
            "seat hop, Mint",
            input(actor(DEPLOYER), None, true, vec![], &mint),
        ),
        (
            "config hop, Mint, sends the credit",
            input(
                config_actor(),
                Some(actor(DEPLOYER)),
                false,
                appointed(),
                &forwarded(&mint),
            ),
        ),
        (
            "owner hop, Credit from the config, asks for clearance",
            input(
                actor(OWNER),
                Some(config_actor()),
                false,
                vec![],
                &Message::Credit(5).into(),
            ),
        ),
        (
            "sender hop, Transfer, sends the credit, asks for clearance",
            input(actor(OWNER), None, true, balance(5), &transfer),
        ),
        (
            "recipient hop, Credit from the sender, asks for clearance",
            input(
                actor(RECIPIENT),
                Some(actor(OWNER)),
                false,
                vec![],
                &Message::Credit(3).into(),
            ),
        ),
        (
            "config, clearance for an account that did not sign",
            input(
                config_actor(),
                Some(actor(RECIPIENT)),
                false,
                appointed(),
                &clearance(false),
            ),
        ),
        (
            "config, clearance for an account that signed",
            input(
                config_actor(),
                Some(actor(OWNER)),
                false,
                appointed(),
                &clearance(true),
            ),
        ),
        (
            "config, clearance, the longest notice posted",
            input(
                config_actor(),
                Some(actor(OWNER)),
                false,
                with_notice(&"n".repeat(NOTICE_MAX)),
                &clearance(true),
            ),
        ),
    ];
    println!("\n| Hop | Cycles |\n| --- | ---: |");
    let mut by_name = BTreeMap::new();
    for (name, input) in &rows {
        let cycles = run(input);
        println!("| {name} | {cycles} |");
        by_name.insert(*name, cycles);
    }
    let unsigned = by_name["config, clearance for an account that did not sign"];
    let signed = by_name["config, clearance for an account that signed"];
    let mint_cycles = by_name["seat hop, Mint"]
        + by_name["config hop, Mint, sends the credit"]
        + by_name["owner hop, Credit from the config, asks for clearance"]
        + unsigned;
    let transfer_cycles = by_name["sender hop, Transfer, sends the credit, asks for clearance"]
        + by_name["recipient hop, Credit from the sender, asks for clearance"]
        + unsigned
        + signed;
    println!(
        "\n| Operation | Executions | Of them clearances | Cycles | Of them clearances |\n| --- | ---: | ---: | ---: | ---: |"
    );
    println!("| mint | 4 | 1 | {mint_cycles} | {unsigned} |");
    println!(
        "| transfer | 4 | 2 | {transfer_cycles} | {} |",
        unsigned + signed
    );
    println!(
        "\nprogram binary: {} bytes",
        freeze_sample_methods::FREEZE_SAMPLE_ELF.len()
    );
    for cycles in by_name.values() {
        assert!(*cycles > 0 && *cycles < BUDGET);
    }
}

#[test]
fn bytes_of_a_private_sender_s_request() {
    let declared = borsh::to_vec(&PublicExecutionContext::new(
        [config_actor()],
        Vec::<AccountId>::new(),
    ))
    .unwrap()
    .len()
        - borsh::to_vec(&PublicExecutionContext::default())
            .unwrap()
            .len();
    assert_eq!(declared, 64);
    for authorized in [false, true] {
        let request = BoundaryStep::PrivateToPublic(Delivery {
            envelope: MessageEnvelope {
                from: actor(OWNER),
                to: config_actor(),
                message: borsh::to_vec(&clearance(authorized)).unwrap(),
            },
            inherited_authorizations: BTreeSet::new(),
            inherits_entry_authorizations: true,
            pda_seeds: BTreeSet::new(),
        });
        let bytes = borsh::to_vec(&request).unwrap().len()
            + borsh::to_vec(&BoundaryStep::EndPublicSubtree).unwrap().len();
        println!(
            "a private sender's request, authorized {authorized}: a {bytes}-byte boundary, plus 32 per inherited authorization"
        );
        assert_eq!(bytes, 146);
    }
    println!(
        "the config actor declared once per transaction: {declared} bytes, so one request {} and two {}",
        declared + 146,
        declared + 2 * 146
    );
}
