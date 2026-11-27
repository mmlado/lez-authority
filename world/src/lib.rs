//! Test support for this repository: programs run in process through LEZ's own executor.
//!
//! A [`World`] holds programs and their actor states. [`World::transact`] hands one public
//! transaction to `lee_core`'s executor, which delivers every message and decides who
//! authorized what, as it does in the sequencer. The world only runs the programs and keeps
//! the states.
//!
//! This crate is outside the versioning promise. Its interface may change in any release.

#![warn(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};

use lee_core::RootCall;
use lee_core::account::{AccountId, Actor, ActorState};
use lee_core::execution_state::{
    ExecutionEnvironment, ExecutionError, PublicExecutionContext, TransactionEntry, TransitionView,
    WholeExecution,
};
use lee_core::program::{MessageBody, ReceiveInput, Response, Transition};

type Program<E> = Box<dyn Fn(&ReceiveInput) -> Result<Response, E>>;

/// Programs and the states of their actors.
pub struct World<E> {
    programs: BTreeMap<AccountId, Program<E>>,
    states: BTreeMap<Actor, ActorState>,
    registered: BTreeSet<AccountId>,
}

/// One public transaction: a message to `to`, with the actors it may reach.
#[derive(Clone, Debug)]
pub struct Transaction {
    /// The actor the message goes to.
    pub to: Actor,
    /// The message, encoded.
    pub message: Vec<u8>,
    /// Every actor the transaction reaches. The executor refuses a delivery to any other.
    pub actors: Vec<Actor>,
    /// The accounts that authorized the transaction. The world checks no signature, so it
    /// takes any account here, a PDA too.
    pub signers: Vec<AccountId>,
    /// Accounts the transaction vouches for, as a client does with a key or a PDA's program
    /// and seed. An account the world has not registered and that did not sign is reached only
    /// when listed here or seeded by the message that reaches it.
    pub evidence: Vec<AccountId>,
}

/// What a transaction did, beside the states it left in the world.
pub struct Outcome {
    /// Every delivery in order, with the input the executor built and the program's response.
    pub trace: Vec<Transition>,
    /// The async messages the transaction emitted. The world delivers none of them.
    pub async_messages: Vec<MessageBody>,
}

/// Why a transaction left the world as it was.
#[derive(Debug)]
pub enum Failure<E> {
    /// A program returned an error.
    Refused(E),
    /// The executor refused the transaction.
    Executor(ExecutionError),
    /// A delivery reached a program the world does not hold.
    UnknownProgram(AccountId),
}

impl<E> From<ExecutionError> for Failure<E> {
    fn from(error: ExecutionError) -> Self {
        Self::Executor(error)
    }
}

impl<E> Default for World<E> {
    fn default() -> Self {
        Self {
            programs: BTreeMap::new(),
            states: BTreeMap::new(),
            registered: BTreeSet::new(),
        }
    }
}

impl<E> World<E> {
    /// A world with no program and no state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the program with this id. A panic in `handler` passes through
    /// [`transact`](Self::transact).
    #[must_use]
    pub fn program(
        mut self,
        id: AccountId,
        handler: impl Fn(&ReceiveInput) -> Result<Response, E> + 'static,
    ) -> Self {
        self.programs.insert(id, Box::new(handler));
        self
    }

    /// Sets the state `actor` starts with, as a chain's state before the test's first
    /// transaction, and registers its account. Every later change comes from
    /// [`transact`](Self::transact).
    #[must_use]
    pub fn starting_with(mut self, actor: Actor, state: Vec<u8>) -> Self {
        self.registered.insert(actor.account_id);
        self.keep(actor, ActorState::from(state));
        self
    }

    /// The state `actor` holds, empty when it holds none.
    #[must_use]
    pub fn state(&self, actor: Actor) -> &[u8] {
        self.states.get(&actor).map_or(&[], |state| state)
    }

    /// Every actor that holds state.
    pub fn actors(&self) -> impl Iterator<Item = Actor> {
        self.states.keys().copied()
    }

    /// Runs one transaction through the executor. The world keeps the states it leaves and
    /// registers every account it reached and every signer, as the chain does, so a later
    /// transaction reaches them without evidence.
    ///
    /// The validity windows the programs return are checked against no block and no time.
    ///
    /// # Errors
    /// A program's error, the executor's refusal, or a delivery to a program the world does
    /// not hold. The world is then unchanged.
    pub fn transact(&mut self, transaction: Transaction) -> Result<Outcome, Failure<E>> {
        let Transaction {
            to,
            message,
            actors,
            signers,
            evidence,
        } = transaction;
        let mut run = Run {
            world: self,
            vouched: signers.iter().copied().chain(evidence).collect(),
            trace: Vec::new(),
        };
        let context = PublicExecutionContext::new(actors, signers.clone());
        let root = TransactionEntry::Sync(RootCall { to, message });
        let public = WholeExecution::new(context, root, Vec::new(), None, &[])?
            .execute(&mut run)?
            .public;
        let trace = run.trace;
        self.registered.extend(signers);
        for (account, data) in public.accounts {
            self.registered.insert(account);
            for (program, state) in data.actor_states {
                self.keep(Actor::new(account, program), state);
            }
        }
        Ok(Outcome {
            trace,
            async_messages: public.async_messages,
        })
    }

    fn keep(&mut self, actor: Actor, state: ActorState) {
        if state.is_empty() {
            self.states.remove(&actor);
        } else {
            self.states.insert(actor, state);
        }
    }
}

/// One transaction's view of the world, which the executor drives.
struct Run<'world, E> {
    world: &'world World<E>,
    vouched: BTreeSet<AccountId>,
    trace: Vec<Transition>,
}

impl<E> ExecutionEnvironment for Run<'_, E> {
    type Error = Failure<E>;

    fn handle_message(
        &mut self,
        input: &ReceiveInput,
        _view: &TransitionView<'_>,
    ) -> Result<Transition, Failure<E>> {
        let id = input.receiver.program_account_id;
        let program = self
            .world
            .programs
            .get(&id)
            .ok_or(Failure::UnknownProgram(id))?;
        let transition = program(input)
            .map_err(Failure::Refused)?
            .into_transition(input.clone());
        self.trace.push(transition.clone());
        Ok(transition)
    }

    fn public_actor_state(&mut self, actor: Actor) -> Result<ActorState, Failure<E>> {
        Ok(self
            .world
            .states
            .get(&actor)
            .cloned()
            .unwrap_or_else(ActorState::empty))
    }

    fn admits(&mut self, account_id: AccountId) -> Result<bool, Failure<E>> {
        Ok(self.world.registered.contains(&account_id) || self.vouched.contains(&account_id))
    }
}

#[cfg(test)]
mod tests;
