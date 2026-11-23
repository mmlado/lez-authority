//! Drives the sample on a local dev sequencer and reads the config back after each step.
//!
//! Usage: admin-sample-runner <admin-sample.bin> <payer account id>

use admin_sample_core::{CONFIG_SEED, Config, Message, TargetState, config_account};
use anyhow::{Context, Result, anyhow};
use lee_core::account::{AccountId, Actor};
use lez_admin_authority::{AdminOp, AuthoritySlot, Holder, Wire, admin_op};
use wallet::{
    AccountIdentity, AccountMention, DeclaredFee, ExecutionFailureKind, WalletCore,
    WalletSequencerClient, program_facades::program_loader::ProgramLoader,
};

#[tokio::main]
async fn main() -> Result<()> {
    let usage = "usage: admin-sample-runner <admin-sample.bin> <payer account id>";
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
    let stranger = wallet.create_new_account_public(None).0;
    let nominee = wallet.create_new_account_public(None).0;
    wallet.store_persistent_data()?;
    let program = ProgramLoader(&wallet)
        .deploy(deployer, &segments, bytecode, true, Some(payer))
        .await?;
    println!(
        "program  {program:?}, header declared gas {}",
        declared_gas(&wallet)
    );
    println!("deployer {deployer:?}");
    println!("config   {:?}", config_account(program));
    println!("stranger {stranger:?}");
    println!("nominee  {nominee:?}");

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
    let target = |account: AccountId| {
        AccountIdentity::PublicNoSign(account).select_program_actor_state(program)
    };

    refused(
        &mut wallet,
        payer,
        vec![seat(stranger), config()],
        &admin_op(AdminOp::Init),
        "E117",
        "stranger init",
    )
    .await?;
    check(
        &wallet,
        program,
        None,
        "after the stranger's init, must be empty",
    )
    .await?;

    // The deployer is the program's own account, so the gate lets it through.
    call(
        &wallet,
        payer,
        vec![seat(deployer), config()],
        &admin_op(AdminOp::Init),
        "init",
    )
    .await?;
    let expected = Config {
        open_value: 0,
        admin_value: 0,
        admin: AuthoritySlot::held(Holder {
            account: deployer,
            via: None,
        }),
    };
    check(&wallet, program, Some(&expected), "after init").await?;

    // No seat: the payer signs only for the fee.
    call(
        &wallet,
        payer,
        vec![config()],
        &Message::SetOpenValue(7).into(),
        "open setter",
    )
    .await?;
    let expected = Config {
        open_value: 7,
        ..expected
    };
    check(&wallet, program, Some(&expected), "after the open setter").await?;

    call(
        &wallet,
        payer,
        vec![seat(deployer), config()],
        &Message::SetAdminValue(9).into(),
        "admin setter",
    )
    .await?;
    let expected = Config {
        admin_value: 9,
        ..expected
    };
    check(&wallet, program, Some(&expected), "after the admin setter").await?;

    // Three hops, so the target is in the actor list too.
    let set_target = Message::SetTargetValue {
        target: stranger,
        value: 5,
    };
    call(
        &wallet,
        payer,
        vec![seat(deployer), config(), target(stranger)],
        &set_target.into(),
        "target setter",
    )
    .await?;
    check(
        &wallet,
        program,
        Some(&expected),
        "after the target setter, config unchanged",
    )
    .await?;
    check_target(
        &wallet,
        program,
        stranger,
        Some(&TargetState { value: 5 }),
        "the target's state",
    )
    .await?;

    refused(
        &mut wallet,
        payer,
        vec![seat(stranger), config()],
        &Message::SetAdminValue(1).into(),
        "E103",
        "stranger",
    )
    .await?;
    check(
        &wallet,
        program,
        Some(&expected),
        "after the stranger, must be unchanged",
    )
    .await?;

    let nominate = admin_op(AdminOp::Nominate { next: nominee });
    call(
        &wallet,
        payer,
        vec![seat(deployer), config()],
        &nominate,
        "nominate",
    )
    .await?;
    let expected = Config {
        admin: AuthoritySlot::Held {
            holder: Holder {
                account: deployer,
                via: None,
            },
            nominee: Some(nominee),
        },
        ..expected
    };
    check(&wallet, program, Some(&expected), "after nominate").await?;

    refused(
        &mut wallet,
        payer,
        vec![seat(nominee), config()],
        &Message::SetAdminValue(1).into(),
        "E103",
        "nominee before accept",
    )
    .await?;
    check(
        &wallet,
        program,
        Some(&expected),
        "after the nominee's early call, must be unchanged",
    )
    .await?;

    let accept = admin_op(AdminOp::Accept { pda_seed: None });
    call(
        &wallet,
        payer,
        vec![seat(nominee), config()],
        &accept,
        "accept",
    )
    .await?;
    let expected = Config {
        admin: AuthoritySlot::held(Holder {
            account: nominee,
            via: None,
        }),
        ..expected
    };
    check(&wallet, program, Some(&expected), "after accept").await?;

    call(
        &wallet,
        payer,
        vec![seat(nominee), config()],
        &Message::SetAdminValue(11).into(),
        "new admin setter",
    )
    .await?;
    let expected = Config {
        admin_value: 11,
        ..expected
    };
    check(
        &wallet,
        program,
        Some(&expected),
        "after the new admin's setter",
    )
    .await?;

    refused(
        &mut wallet,
        payer,
        vec![seat(deployer), config()],
        &Message::SetAdminValue(2).into(),
        "E103",
        "old admin",
    )
    .await?;
    check(
        &wallet,
        program,
        Some(&expected),
        "after the old admin, must be unchanged",
    )
    .await?;

    call(
        &wallet,
        payer,
        vec![seat(nominee), config()],
        &admin_op(AdminOp::Renounce),
        "renounce",
    )
    .await?;
    let expected = Config {
        admin: AuthoritySlot::Renounced,
        ..expected
    };
    check(&wallet, program, Some(&expected), "after renounce").await?;

    refused(
        &mut wallet,
        payer,
        vec![seat(nominee), config()],
        &Message::SetAdminValue(3).into(),
        "E102",
        "after renounce",
    )
    .await?;
    check(
        &wallet,
        program,
        Some(&expected),
        "after the renounced admin's call, must be unchanged",
    )
    .await?;

    println!(
        "all steps behaved. Each refused call carried its code in the wallet's simulation, then was included on chain and changed nothing."
    );
    Ok(())
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

/// Sends a call the guard must refuse with `code`: the wallet's simulation has to report the
/// code, then the call goes on chain and is reverted there.
async fn refused<C: WalletSequencerClient>(
    wallet: &mut WalletCore<C>,
    payer: AccountId,
    accounts: Vec<AccountMention>,
    message: &Wire<Message>,
    code: &str,
    what: &str,
) -> Result<()> {
    let needle = format!("lez-authority/admin {code}:");
    let bytes = borsh::to_vec(message)?;
    match wallet
        .send_pub_tx_paid_by(accounts.clone(), 0, bytes, Some(payer))
        .await
    {
        Err(ExecutionFailureKind::SimulatedRevert(reason)) if reason.contains(&needle) => {
            println!("{what}: refused in simulation with {code}");
        }
        other => {
            return Err(anyhow!(
                "{what}: expected a simulated {code} refusal, got {other:?}"
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

/// Reads a target's state back and compares it with what the step should have left.
async fn check_target<C: WalletSequencerClient>(
    wallet: &WalletCore<C>,
    program: AccountId,
    target: AccountId,
    expected: Option<&TargetState>,
    when: &str,
) -> Result<()> {
    let found: Option<TargetState> =
        match wallet.get_account_view(Actor::new(target, program)).await? {
            None => None,
            Some(account) => {
                let state: &[u8] = account.data.actor_state(program);
                if state.is_empty() {
                    None
                } else {
                    Some(borsh::from_slice(state).context("the target state does not decode")?)
                }
            }
        };
    println!("{when}: {found:?}");
    if found.as_ref() != expected {
        return Err(anyhow!("{when}: expected {expected:?}"));
    }
    Ok(())
}

/// Reads the config back and compares it with what the step should have left.
async fn check<C: WalletSequencerClient>(
    wallet: &WalletCore<C>,
    program: AccountId,
    expected: Option<&Config>,
    when: &str,
) -> Result<()> {
    let actor = Actor::new(config_account(program), program);
    let found: Option<Config> = match wallet.get_account_view(actor).await? {
        None => None,
        Some(account) => {
            let state: &[u8] = account.data.actor_state(program);
            if state.is_empty() {
                None
            } else {
                Some(borsh::from_slice(state).context("the config does not decode")?)
            }
        }
    };
    println!("{when}: {found:?}");
    if found.as_ref() != expected {
        return Err(anyhow!("{when}: expected {expected:?}"));
    }
    Ok(())
}
