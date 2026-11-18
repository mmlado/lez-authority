//! Drives the token sample on a local sequencer and reads the state back after each step: the
//! admin's `Init`, a mint, the freeze authority appointed, replaced and revoked, a freeze with
//! no freeze authority refused, a second mint, the freeze authority appointed again, and a mint
//! by the admin to itself. Then a transfer between two owners, and one without the config actor
//! or without the sender's signature, both refused. Then the freeze: the freeze authority
//! freezes the program, a mint and a transfer are refused, the admin's notice runs through the
//! freeze, the admin replaces the freeze authority while frozen, the successor releases, the
//! admin clears the notice and mints again, and a transfer lands.
//!
//! Usage: sample-runner <sample.bin> <payer account id>

use anyhow::{Context, Result, anyhow};
use lee_core::account::{AccountId, Actor};
use lez_freeze_authority::{
    AuthoritySlot, FreezeOp, Holder, Op, Slot, Wire, admin_op, freeze_authority_op, freeze_op,
};
use sample_core::{Balance, CONFIG_SEED, Config, Message, config_account};
use wallet::{
    AccountIdentity, AccountMention, DeclaredFee, ExecutionFailureKind, WalletCore,
    WalletSequencerClient, program_facades::program_loader::ProgramLoader,
};

const NOTICE: &str = "frozen for review";

#[tokio::main]
async fn main() -> Result<()> {
    let usage = "usage: sample-runner <sample.bin> <payer account id>";
    let mut args = std::env::args().skip(1);
    let bin = args.next().context(usage)?;
    let payer: AccountId = args
        .next()
        .context(usage)?
        .parse()
        .map_err(|error| anyhow!("payer account id: {error:?}"))?;
    let mut wallet = WalletCore::from_env()?;

    // The header account is the program's id, and its key is the deployer's.
    let bytecode = std::fs::read(&bin).with_context(|| format!("reading {bin}"))?;
    let user_elf_len = risc0_binfmt::ProgramBinary::decode(&bytecode)
        .map_err(|error| anyhow!("not a program binary: {error:?}"))?
        .user_elf
        .len();
    let segment_count = user_elf_len.div_ceil(program_loader_core::MAX_SEGMENT_DATA_LEN);
    let deployer = wallet.create_new_account_public(None).0;
    let segments: Vec<AccountId> =
        std::iter::repeat_with(|| wallet.create_new_account_public(None).0)
            .take(segment_count)
            .collect();
    let authority = wallet.create_new_account_public(None).0;
    let successor = wallet.create_new_account_public(None).0;
    let stranger = wallet.create_new_account_public(None).0;
    wallet.store_persistent_data()?;
    let program = ProgramLoader(&wallet)
        .deploy(deployer, &segments, bytecode, true, Some(payer))
        .await?;
    println!(
        "program   {program:?}, header declared gas {}",
        declared_gas(&wallet)
    );
    println!("deployer  {deployer:?}");
    println!("config    {:?}", config_account(program));
    println!("authority {authority:?}");
    println!("successor {successor:?}");
    println!("stranger  {stranger:?}");

    let config = || {
        AccountIdentity::PublicPda {
            program,
            seed: CONFIG_SEED,
        }
        .select_program_actor_state(program)
        .without_authorization()
    };
    let seat =
        |account: AccountId| AccountIdentity::Public(account).select_program_actor_state(program);
    let owner = |account: AccountId| {
        AccountIdentity::PublicNoSign(account).select_program_actor_state(program)
    };
    let send = |sender: AccountId| vec![seat(sender), config()];
    let mint = |to: AccountId, amount: u64| Message::Mint { to, amount }.into();
    let nominate = |next: AccountId| freeze_authority_op(Op::Nominate { next });
    let accept = || freeze_authority_op(Op::Accept { pda_seed: None });
    let freeze = || freeze_op(FreezeOp::Freeze);
    let release = || freeze_op(FreezeOp::Release);
    let notice = |text: &str| -> Wire<Message> { Message::SetNotice(text.to_owned()).into() };
    let transfer =
        |to: AccountId, amount: u64| -> Wire<Message> { Message::Transfer { to, amount }.into() };
    // rc3 runs only declared actors, and both actors of a transfer ask the config for clearance.
    let between = |from: AccountId, to: AccountId| vec![seat(from), config(), owner(to)];

    // The deployer is the program's own account, so the admin's gate lets it through.
    call(&wallet, payer, send(deployer), &admin_op(Op::Init), "init").await?;
    let expected = with_freeze(deployer, AuthoritySlot::Unset);
    check(
        &wallet,
        program,
        &expected,
        "after init, the freeze slot unset",
    )
    .await?;

    // Three hops, so the owner's actor is in the actor list too, unless it is the sender's seat:
    // the wallet refuses one account named twice with different signing.
    let to_owner = |sender: AccountId, to: AccountId| {
        let mut accounts = vec![seat(sender), config()];
        if to != sender {
            accounts.push(owner(to));
        }
        accounts
    };
    call(
        &wallet,
        payer,
        to_owner(deployer, authority),
        &mint(authority, 5),
        "mint",
    )
    .await?;
    check_balance(&wallet, program, authority, 5, "after the mint").await?;
    refused(
        &mut wallet,
        payer,
        to_owner(stranger, authority),
        &mint(authority, 5),
        "lez-authority/admin E103:",
        "a stranger's mint",
    )
    .await?;
    check_balance(
        &wallet,
        program,
        authority,
        5,
        "after the stranger's mint, unchanged",
    )
    .await?;

    refused(
        &mut wallet,
        payer,
        send(deployer),
        &freeze_authority_op(Op::Init),
        "lez-authority/freeze E137:",
        "the freeze role's init",
    )
    .await?;
    check(&wallet, program, &expected, "after it, unchanged").await?;
    refused(
        &mut wallet,
        payer,
        send(stranger),
        &nominate(stranger),
        "lez-freeze-authority/freeze E208:",
        "a stranger's nomination",
    )
    .await?;
    check(&wallet, program, &expected, "after it, unchanged").await?;

    call(
        &wallet,
        payer,
        send(deployer),
        &nominate(authority),
        "appoint",
    )
    .await?;
    let expected = with_freeze(deployer, AuthoritySlot::Offered { nominee: authority });
    check(&wallet, program, &expected, "after the admin's offer").await?;

    refused(
        &mut wallet,
        payer,
        send(stranger),
        &accept(),
        "lez-authority/freeze E114:",
        "a stranger's accept",
    )
    .await?;
    check(&wallet, program, &expected, "after it, unchanged").await?;
    call(&wallet, payer, send(authority), &accept(), "accept").await?;
    let expected = with_freeze(deployer, held(authority));
    check(&wallet, program, &expected, "after the first appointment").await?;

    call(
        &wallet,
        payer,
        send(deployer),
        &nominate(successor),
        "replace",
    )
    .await?;
    let offered = AuthoritySlot::Held {
        holder: Holder {
            account: authority,
            via: None,
        },
        nominee: Some(successor),
    };
    let expected = with_freeze(deployer, offered.clone());
    check(
        &wallet,
        program,
        &expected,
        "after the admin's offer, the authority still holds",
    )
    .await?;
    call(
        &wallet,
        payer,
        send(successor),
        &accept(),
        "the successor's accept",
    )
    .await?;
    let expected = with_freeze(deployer, held(successor));
    check(&wallet, program, &expected, "after the replacement").await?;
    refused(
        &mut wallet,
        payer,
        send(authority),
        &nominate(stranger),
        "lez-freeze-authority/freeze E208:",
        "the replaced freeze authority's nomination",
    )
    .await?;
    check(&wallet, program, &expected, "after it, unchanged").await?;

    let renounce = freeze_authority_op(Op::Renounce);
    call(&wallet, payer, send(deployer), &renounce, "revoke").await?;
    let expected = with_freeze(deployer, AuthoritySlot::Unset);
    check(&wallet, program, &expected, "after the revoke").await?;
    refused(
        &mut wallet,
        payer,
        send(successor),
        &nominate(stranger),
        "lez-freeze-authority/freeze E208:",
        "the revoked freeze authority's nomination",
    )
    .await?;
    check(&wallet, program, &expected, "after it, unchanged").await?;
    refused(
        &mut wallet,
        payer,
        send(successor),
        &freeze(),
        "lez-authority/freeze E131:",
        "a freeze with no freeze authority",
    )
    .await?;
    check(&wallet, program, &expected, "after it, unchanged").await?;

    // The config is the first mint's again, so the gas difference is the balance alone.
    call(
        &wallet,
        payer,
        to_owner(deployer, authority),
        &mint(authority, 3),
        "second mint",
    )
    .await?;
    check_balance(&wallet, program, authority, 8, "after the second mint").await?;

    call(
        &wallet,
        payer,
        send(deployer),
        &nominate(authority),
        "appoint again",
    )
    .await?;
    call(
        &wallet,
        payer,
        send(authority),
        &accept(),
        "the second accept",
    )
    .await?;
    let expected = with_freeze(deployer, held(authority));
    check(&wallet, program, &expected, "after appointing again").await?;

    call(
        &wallet,
        payer,
        to_owner(deployer, deployer),
        &mint(deployer, 2),
        "the admin's mint to itself",
    )
    .await?;
    check_balance(
        &wallet,
        program,
        deployer,
        2,
        "after the admin's mint to itself",
    )
    .await?;

    call(
        &wallet,
        payer,
        between(authority, successor),
        &transfer(successor, 3),
        "a transfer",
    )
    .await?;
    check_balance(&wallet, program, authority, 5, "after it, the sender").await?;
    check_balance(&wallet, program, successor, 3, "after it, the recipient").await?;
    refused(
        &mut wallet,
        payer,
        vec![seat(authority), owner(successor)],
        &transfer(successor, 3),
        "which is neither a declared public actor nor private",
        "a transfer without the config actor",
    )
    .await?;
    check_balance(&wallet, program, authority, 5, "after it, the sender").await?;
    check_balance(&wallet, program, successor, 3, "after it, the recipient").await?;
    refused(
        &mut wallet,
        payer,
        vec![owner(authority), config(), owner(successor)],
        &transfer(successor, 3),
        "the sender must sign the transfer",
        "an unsigned transfer",
    )
    .await?;
    check_balance(&wallet, program, authority, 5, "after it, the sender").await?;
    check_balance(&wallet, program, successor, 3, "after it, the recipient").await?;

    refused(
        &mut wallet,
        payer,
        send(deployer),
        &freeze(),
        "lez-authority/freeze E103:",
        "the admin's freeze",
    )
    .await?;
    check(&wallet, program, &expected, "after it, unchanged").await?;
    call(&wallet, payer, send(authority), &freeze(), "freeze").await?;
    check_frozen(&wallet, program, &expected, "after the freeze").await?;
    refused(
        &mut wallet,
        payer,
        to_owner(deployer, authority),
        &mint(authority, 1),
        "lez-freeze-authority/freeze E217:",
        "a mint while frozen",
    )
    .await?;
    check_balance(&wallet, program, authority, 5, "after it, unchanged").await?;
    refused(
        &mut wallet,
        payer,
        between(authority, successor),
        &transfer(successor, 1),
        "lez-freeze-authority/freeze E217:",
        "a transfer while frozen",
    )
    .await?;
    check_balance(&wallet, program, authority, 5, "after it, the sender").await?;
    check_balance(&wallet, program, successor, 3, "after it, the recipient").await?;
    refused(
        &mut wallet,
        payer,
        send(stranger),
        &notice(NOTICE),
        "lez-authority/admin E103:",
        "a stranger's notice while frozen",
    )
    .await?;
    check_frozen(&wallet, program, &expected, "after it, unchanged").await?;
    call(
        &wallet,
        payer,
        send(deployer),
        &notice(NOTICE),
        "the admin's notice while frozen",
    )
    .await?;
    let expected = Config {
        notice: NOTICE.to_owned(),
        ..expected
    };
    check_frozen(&wallet, program, &expected, "after the notice").await?;

    call(
        &wallet,
        payer,
        send(deployer),
        &nominate(successor),
        "replace while frozen",
    )
    .await?;
    let expected = Config {
        notice: NOTICE.to_owned(),
        ..with_freeze(deployer, offered)
    };
    check_frozen(
        &wallet,
        program,
        &expected,
        "after the admin's offer while frozen",
    )
    .await?;
    call(
        &wallet,
        payer,
        send(successor),
        &accept(),
        "the successor's accept while frozen",
    )
    .await?;
    let expected = Config {
        notice: NOTICE.to_owned(),
        ..with_freeze(deployer, held(successor))
    };
    check_frozen(
        &wallet,
        program,
        &expected,
        "after the replacement while frozen",
    )
    .await?;
    refused(
        &mut wallet,
        payer,
        send(authority),
        &release(),
        "lez-authority/freeze E103:",
        "the replaced freeze authority's release",
    )
    .await?;
    check_frozen(&wallet, program, &expected, "after it, unchanged").await?;
    call(&wallet, payer, send(successor), &release(), "release").await?;
    check(&wallet, program, &expected, "after the release").await?;
    call(
        &wallet,
        payer,
        send(deployer),
        &notice(""),
        "the admin clears the notice",
    )
    .await?;
    let expected = with_freeze(deployer, held(successor));
    check(&wallet, program, &expected, "after the notice is cleared").await?;
    call(
        &wallet,
        payer,
        to_owner(deployer, authority),
        &mint(authority, 1),
        "a mint after the release",
    )
    .await?;
    check_balance(
        &wallet,
        program,
        authority,
        6,
        "after the mint after the release",
    )
    .await?;
    call(
        &wallet,
        payer,
        between(successor, authority),
        &transfer(authority, 2),
        "a transfer after the release",
    )
    .await?;
    check_balance(&wallet, program, successor, 1, "after it, the sender").await?;
    check_balance(&wallet, program, authority, 8, "after it, the recipient").await?;

    println!(
        "all steps behaved. Each refused call was refused in the wallet's simulation, then included on chain, and changed nothing."
    );
    Ok(())
}

fn held(account: AccountId) -> AuthoritySlot {
    AuthoritySlot::held(Holder { account, via: None })
}

/// The config with `admin` as the admin and `freeze` as the freeze slot's holder part.
fn with_freeze(admin: AccountId, freeze: AuthoritySlot) -> Config {
    let mut config = Config {
        admin: held(admin),
        ..Config::default()
    };
    *config.freeze.authority_slot_mut() = freeze;
    config
}

/// Sends one root call over `accounts`, paid by `payer`, and waits for inclusion.
async fn call<C: WalletSequencerClient>(
    wallet: &WalletCore<C>,
    payer: AccountId,
    accounts: Vec<AccountMention>,
    message: &Wire<Message>,
    what: &str,
) -> Result<()> {
    let bytes = borsh::to_vec(message)?;
    let hash = wallet
        .send_pub_tx_paid_by(accounts, 0, bytes, Some(payer))
        .await
        .map_err(|error| anyhow!("{what}: {error:?}"))?;
    let (_tx, block) = wallet.poll_transaction(hash).await?;
    println!(
        "{what}: included in block {block:?}, declared gas {}",
        declared_gas(wallet)
    );
    Ok(())
}

/// Sends a call that must be refused with a text containing `needle`: the wallet's simulation
/// has to report it, then the call goes on chain and is reverted there.
async fn refused<C: WalletSequencerClient>(
    wallet: &mut WalletCore<C>,
    payer: AccountId,
    accounts: Vec<AccountMention>,
    message: &Wire<Message>,
    needle: &str,
    what: &str,
) -> Result<()> {
    let bytes = borsh::to_vec(message)?;
    match wallet
        .send_pub_tx_paid_by(accounts.clone(), 0, bytes, Some(payer))
        .await
    {
        Err(ExecutionFailureKind::SimulatedRevert(reason)) if reason.contains(needle) => {
            println!("{what}: refused in simulation with {needle}");
        }
        other => {
            return Err(anyhow!(
                "{what}: expected a simulated refusal with {needle}, got {other:?}"
            ));
        }
    }
    // Sent anyway, so the sequencer refuses it as well, not only the wallet.
    wallet.set_allow_revert(true);
    let sent = call(wallet, payer, accounts, message, what).await;
    wallet.set_allow_revert(false);
    sent
}

/// The gas the wallet measured and declared for its last send.
fn declared_gas<C: WalletSequencerClient>(wallet: &WalletCore<C>) -> String {
    match wallet.take_declared_fee() {
        Some(DeclaredFee::Public(fee)) => format!("{:?}", fee.gas_limit),
        _ => "none".to_owned(),
    }
}

/// Reads an owner's balance back and compares it with what the step should have left.
async fn check_balance<C: WalletSequencerClient>(
    wallet: &WalletCore<C>,
    program: AccountId,
    account: AccountId,
    expected: u64,
    when: &str,
) -> Result<()> {
    let found: Balance = match wallet
        .get_account_view(Actor::new(account, program))
        .await?
    {
        None => Balance::default(),
        Some(view) => {
            let state: &[u8] = view.data.actor_state(program);
            if state.is_empty() {
                Balance::default()
            } else {
                borsh::from_slice(state).context("the balance does not decode")?
            }
        }
    };
    println!("{when}: {found:?}");
    if found.amount != expected {
        return Err(anyhow!("{when}: expected {expected}"));
    }
    Ok(())
}

/// The config as the chain holds it, if the account holds any.
async fn read_config<C: WalletSequencerClient>(
    wallet: &WalletCore<C>,
    program: AccountId,
) -> Result<Option<Config>> {
    let actor = Actor::new(config_account(program), program);
    Ok(match wallet.get_account_view(actor).await? {
        None => None,
        Some(account) => {
            let state: &[u8] = account.data.actor_state(program);
            if state.is_empty() {
                None
            } else {
                Some(borsh::from_slice(state).context("the config does not decode")?)
            }
        }
    })
}

/// Reads the config back and compares it with what the step should have left.
async fn check<C: WalletSequencerClient>(
    wallet: &WalletCore<C>,
    program: AccountId,
    expected: &Config,
    when: &str,
) -> Result<()> {
    let found = read_config(wallet, program).await?;
    println!("{when}: {found:?}");
    if found.as_ref() != Some(expected) {
        return Err(anyhow!("{when}: expected {expected:?}"));
    }
    Ok(())
}

/// [`check`] for a frozen program. A client reads the freeze flag but cannot set it, so the
/// freeze slot is compared part by part.
async fn check_frozen<C: WalletSequencerClient>(
    wallet: &WalletCore<C>,
    program: AccountId,
    expected: &Config,
    when: &str,
) -> Result<()> {
    let found = read_config(wallet, program).await?;
    println!("{when}: {found:?}");
    let matches = found.as_ref().is_some_and(|found| {
        found.freeze.is_frozen()
            && found.freeze.authority_slot() == expected.freeze.authority_slot()
            && found.freeze.frozen_accounts() == expected.freeze.frozen_accounts()
            && Config {
                freeze: expected.freeze.clone(),
                ..found.clone()
            } == *expected
    });
    if !matches {
        return Err(anyhow!("{when}: expected {expected:?}, frozen"));
    }
    Ok(())
}
