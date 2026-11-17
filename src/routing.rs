//! The guard: where each message runs, and what is checked on the way.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};
use lee_core::program::{PdaSeed, ReceiveInput, Response};

use crate::*;

/// The config account of a program with this seed.
#[must_use]
pub fn config_account(program: AccountId, seed: &PdaSeed) -> AccountId {
    AccountId::for_public_pda(&program, seed)
}

/// A label padded with zeros to the 32 bytes a seed holds.
///
/// # Panics
/// At compile time, for a label longer than 32 bytes.
#[must_use]
pub const fn seed(label: &[u8]) -> PdaSeed {
    assert!(label.len() <= 32, "seed label longer than 32 bytes");
    let mut bytes = [0u8; 32];
    let mut i = 0;
    while i < label.len() {
        bytes[i] = label[i];
        i += 1;
    }
    PdaSeed::new(bytes)
}

/// Runs `handler` behind the guard. The [`guard`](macro@guard) attribute writes this call,
/// and a program may write it itself. `Rs` is the program's roles, `(A,)` or `(A, B)`, `C`
/// the config the program keeps on the config account, and `gate` says who may install the
/// root role.
///
/// - A library operation goes to the config through the sender's seat, and the guard runs it
///   there under the role's policy. The root role's `Init` creates the config.
/// - A message that needs a holder goes the same way. At the config the guard checks the
///   holder, asks [`Policy::may_run`] of the role that refuses program messages, unless the
///   message is exempt from it, runs the handler, and refuses a result that changed a slot.
/// - A message only the config sends runs only when it came from the config account.
/// - A program message handled at an actor other than the config, when a role refuses program
///   messages and the message is not exempt from it, asks the config for clearance: the guard
///   adds the request to the handler's response, and the config answers it under that role's
///   rules in the same transaction.
/// - Anything else runs where it was sent, as in a program without the guard. At the config
///   that needs an initialized config and the consent of the role that refuses program
///   messages, unless the message is exempt from it, and the slots are checked afterwards.
///
/// Wherever the handler runs, the guard refuses a response that sends the config a library
/// message, which the config would take for a hop from that actor.
///
/// The handler of a message that came through a seat sees `input.from` as the sender's own
/// actor and `input.message` as the hop. A PDA holder's grant comes with the call, so every
/// program the handler calls receives it too.
///
/// # Panics
/// On every refusal, with [`Refusal::text`] as the text.
pub fn guard<Rs, C, M>(
    input: &ReceiveInput,
    seed: &PdaSeed,
    gate: InitGate,
    message: Wire<M>,
    handler: impl FnOnce(&ReceiveInput, M) -> Response,
) -> Response
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
    M: Gated + BorshDeserialize,
{
    match try_guard::<Rs, C, M>(input, seed, gate, message, handler) {
        Ok(response) => response,
        Err(refusal) => panic!("{}", refusal.text()),
    }
}

/// [`guard()`] with the guard's refusals as a value. The handler's own panics stay panics.
///
/// # Errors
/// See [`Refusal`].
pub fn try_guard<Rs, C, M>(
    input: &ReceiveInput,
    seed: &PdaSeed,
    gate: InitGate,
    message: Wire<M>,
    handler: impl FnOnce(&ReceiveInput, M) -> Response,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
    M: Gated + BorshDeserialize,
{
    const {
        assert!(
            !Rs::ROOT_BY_NOMINATION_ONLY,
            "the first role creates the config with its Init, so its holder cannot be appointed only by nomination"
        );
        assert!(
            Rs::NAMES_DISTINCT,
            "the guard's roles need distinct names, since a client's operation names its role"
        );
        assert!(
            Rs::AT_MOST_ONE_REFUSES,
            "one of the guard's roles at most may refuse program messages, since a request for clearance names no role"
        );
        assert!(
            Rs::MARKS_LISTED,
            "a marker on the message enum names a role that is not among the guard's roles"
        );
        assert!(
            Rs::EXEMPTIONS_REFUSE,
            "an exempt marker names a role that refuses no program messages"
        );
    }
    let refuse = |error| Refusal {
        role: Rs::ROOT,
        prose: Rs::ROOT_PROSE,
        error,
    };
    let program = input.receiver.program_account_id;
    let config_actor = Actor::new(config_account(program, seed), program);
    let at_config = input.receiver == config_actor;
    let from_config = input.from == Some(config_actor);
    // The config trusts a hop from any actor of this program, so a handler must not send one.
    let handler = |input: &ReceiveInput, message: M| {
        let response = handler(input, message);
        if sends_library_message(&response, config_actor) {
            return Err(refuse(AuthorityError::LibrarySend));
        }
        Ok(response)
    };
    // Only the config sees a refusing role's rules, so a message handled elsewhere asks it.
    let cleared = |response: Response, clearance: bool| {
        if !clearance {
            return response;
        }
        let request = Wire::<()>::Library(LibraryMessage::Clearance(Clearance {
            authorized: input.is_authorized,
        }));
        response.send_sync(config_actor, &request)
    };
    match message {
        Wire::Program(message) => {
            if message.only_from_config() {
                if at_config || !from_config {
                    return Err(refuse(AuthorityError::NotFromConfig));
                }
                let clearance = Rs::needs_clearance(&message);
                return handler(input, message).map(|response| cleared(response, clearance));
            }
            if !at_config {
                if from_config || !Rs::needs_any(&message) {
                    let clearance = Rs::needs_clearance(&message);
                    return handler(input, message).map(|response| cleared(response, clearance));
                }
                return seat(input, config_actor).map_err(refuse);
            }
            at_the_config::<Rs, C, M>(input, program, gate, None, message, handler)
        }
        Wire::Library(LibraryMessage::Op { role, .. }) => {
            let (role, prose) = Rs::index(&role).map_or((Rs::ROOT, Rs::ROOT_PROSE), Rs::names);
            let refuse = |error| Refusal { role, prose, error };
            if at_config {
                return Err(refuse(AuthorityError::NotThroughSeat));
            }
            seat(input, config_actor).map_err(refuse)
        }
        Wire::Library(LibraryMessage::Clearance(request)) => {
            if !at_config {
                return Err(refuse(AuthorityError::ClearanceNotAtConfig));
            }
            let from = input
                .from
                .filter(|_| input.from_own_program())
                .ok_or_else(|| refuse(AuthorityError::ClearanceFromOutside))?;
            answer_clearance::<Rs, C, M>(input, program, gate, from, request)
        }
        Wire::Library(LibraryMessage::Forwarded(hop)) => {
            if !at_config {
                return Err(refuse(AuthorityError::ForwardedNotAtConfig));
            }
            if !input.from_own_program() {
                return Err(refuse(AuthorityError::NotOwnProgram));
            }
            let from = input
                .from
                .ok_or_else(|| refuse(AuthorityError::NotOwnProgram))?;
            let caller = Caller {
                account: from.account_id,
                via: hop.via,
                via_known: true,
            };
            let inner: Wire<M> = borsh::from_slice(&hop.inner)
                .map_err(|_| refuse(AuthorityError::MessageDoesNotDecode))?;
            match inner {
                Wire::Library(LibraryMessage::Op { role, op }) => {
                    let role = Rs::index(&role)
                        .ok_or_else(|| refuse(AuthorityError::MessageDoesNotDecode))?;
                    manage::<Rs, C, M>(input, program, gate, caller, role, op)
                }
                Wire::Library(LibraryMessage::Forwarded(_) | LibraryMessage::Clearance(_)) => {
                    Err(refuse(AuthorityError::MessageDoesNotDecode))
                }
                Wire::Program(message) => {
                    if message.only_from_config() {
                        return Err(refuse(AuthorityError::NotFromConfig));
                    }
                    at_the_config::<Rs, C, M>(input, program, gate, Some(caller), message, handler)
                }
            }
        }
    }
}

/// A management operation at the config, which the root role's `Init` may create.
fn manage<Rs, C, M>(
    input: &ReceiveInput,
    program: AccountId,
    gate: InitGate,
    caller: Caller,
    role: usize,
    op: Op,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
{
    let refuse = |error| Refusal {
        role: Rs::ROOT,
        prose: Rs::ROOT_PROSE,
        error,
    };
    let pre: &[u8] = &input.pre_state;
    let mut config: C = if pre.is_empty() {
        if role != 0 || op != Op::Init {
            return Err(refuse(AuthorityError::NotInitialized));
        }
        C::default()
    } else {
        borsh::from_slice(pre).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?
    };
    let ctx = Ctx {
        caller: Some(caller),
        program,
        gate,
    };
    Rs::run_op(&ctx, &mut config, role, op)?;
    let bytes = borsh::to_vec(&config).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
    Ok(Response::set_state(bytes))
}

/// A program message at the config: the declared holders, the refusing role's consent, the
/// handler, then the slots.
fn at_the_config<Rs, C, M>(
    input: &ReceiveInput,
    program: AccountId,
    gate: InitGate,
    caller: Option<Caller>,
    message: M,
    handler: impl FnOnce(&ReceiveInput, M) -> Result<Response, Refusal>,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
    M: Gated,
{
    let refuse = |error| Refusal {
        role: Rs::ROOT,
        prose: Rs::ROOT_PROSE,
        error,
    };
    let pre: &[u8] = &input.pre_state;
    if pre.is_empty() {
        return Err(refuse(AuthorityError::NotInitialized));
    }
    let config: C =
        borsh::from_slice(pre).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
    let ctx = Ctx {
        caller,
        program,
        gate,
    };
    Rs::gate(&ctx, &config, &message)?;
    Rs::may_run(&ctx, &config, &message)?;
    let response = handler(input, message)?;
    if let Some(post) = &response.post_state {
        let post: &[u8] = post;
        let after: C =
            borsh::from_slice(post).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
        Rs::unchanged(&after, &config)?;
    }
    Ok(response)
}

/// A request for clearance at the config: the refusing role's answer, with no handler. The
/// caller is the sending actor's account when it authorized the transaction.
fn answer_clearance<Rs, C, M>(
    input: &ReceiveInput,
    program: AccountId,
    gate: InitGate,
    from: Actor,
    request: Clearance,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
{
    let refuse = |error| Refusal {
        role: Rs::ROOT,
        prose: Rs::ROOT_PROSE,
        error,
    };
    let pre: &[u8] = &input.pre_state;
    if pre.is_empty() {
        return Err(refuse(AuthorityError::NotInitialized));
    }
    let config: C =
        borsh::from_slice(pre).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
    let ctx = Ctx {
        caller: request.authorized.then_some(Caller {
            account: from.account_id,
            via: None,
            via_known: false,
        }),
        program,
        gate,
    };
    Rs::clear(&ctx, &config)?;
    Ok(Response::keep_state())
}

/// Hop 1, at the sender's own actor: vouch and forward the message as it arrived.
fn seat(input: &ReceiveInput, config_actor: Actor) -> Result<Response, AuthorityError> {
    if !input.is_authorized {
        return Err(AuthorityError::MissingSignature);
    }
    let hop = Wire::<()>::Library(LibraryMessage::Forwarded(Forwarded {
        via: input.from.map(|from| from.program_account_id),
        inner: input.message.clone(),
    }));
    Ok(Response::keep_state().send_sync(config_actor, &hop))
}

/// Whether `response` sends a library message to `actor`.
fn sends_library_message(response: &Response, actor: Actor) -> bool {
    let sync = response
        .sync_messages
        .iter()
        .map(|sent| (sent.to, &sent.message));
    sync.chain(
        response
            .async_messages
            .iter()
            .map(|sent| (sent.to, &sent.message)),
    )
    .any(|(to, message)| to == actor && message.first() == Some(&LIBRARY_TAG))
}
