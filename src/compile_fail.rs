//! Programs the guard refuses at compile time, kept as doctests that must fail to build.

/// A guard over two roles of one name.
///
/// ```compile_fail,E0080
/// use borsh::{BorshDeserialize, BorshSerialize};
/// use lee_core::program::{ReceiveInput, Response};
/// use lez_authority::{AuthoritySlot, Gated, HasSlot, InitGate, Policy, Role, Wire};
///
/// struct Owner;
///
/// impl Role for Owner {
///     const NAME: &'static str = "owner";
///     type Slot = AuthoritySlot;
/// }
///
/// impl<C: HasSlot<Owner>> Policy<C> for Owner {}
///
/// struct Guardian;
///
/// impl Role for Guardian {
///     const NAME: &'static str = "owner";
///     type Slot = AuthoritySlot;
/// }
///
/// impl<C: HasSlot<Guardian>> Policy<C> for Guardian {}
///
/// #[derive(Default, BorshSerialize, BorshDeserialize, HasSlot)]
/// struct Config {
///     #[slot(Owner)]
///     owner: AuthoritySlot,
///     #[slot(Guardian)]
///     guardian: AuthoritySlot,
/// }
///
/// #[derive(BorshSerialize, BorshDeserialize, Gated)]
/// enum Message {
///     Ping,
/// }
///
/// fn handle(input: &ReceiveInput, message: Wire<Message>) -> Response {
///     let seed = lez_authority::seed(b"one-name");
///     lez_authority::guard::<(Owner, Guardian), Config, Message>(
///         input,
///         &seed,
///         InitGate::Deployer,
///         message,
///         |_, _| Response::keep_state(),
///     )
/// }
///
/// let _reified: fn(&ReceiveInput, Wire<Message>) -> Response = handle;
/// ```
pub struct TwoRolesOfOneName;

/// A first role whose holder is only ever appointed by nomination, under a guard called by hand.
///
/// ```compile_fail,E0080
/// use borsh::{BorshDeserialize, BorshSerialize};
/// use lee_core::program::{ReceiveInput, Response};
/// use lez_authority::{AuthoritySlot, Gated, HasSlot, InitGate, Policy, Role, Wire};
///
/// struct Deputy;
///
/// impl Role for Deputy {
///     const NAME: &'static str = "deputy";
///     type Slot = AuthoritySlot;
/// }
///
/// impl<C: HasSlot<Deputy>> Policy<C> for Deputy {
///     const BY_NOMINATION_ONLY: bool = true;
/// }
///
/// #[derive(Default, BorshSerialize, BorshDeserialize, HasSlot)]
/// struct Config {
///     #[slot(Deputy)]
///     deputy: AuthoritySlot,
/// }
///
/// #[derive(BorshSerialize, BorshDeserialize, Gated)]
/// enum Message {
///     Ping,
/// }
///
/// fn handle(input: &ReceiveInput, message: Wire<Message>) -> Response {
///     let seed = lez_authority::seed(b"deputy-first");
///     lez_authority::guard::<(Deputy,), Config, Message>(
///         input,
///         &seed,
///         InitGate::Deployer,
///         message,
///         |_, _| Response::keep_state(),
///     )
/// }
///
/// let _reified: fn(&ReceiveInput, Wire<Message>) -> Response = handle;
/// ```
pub struct NominationOnlyRoot;

/// The same for a list of two roles, the case of a freeze authority put first.
///
/// ```compile_fail,E0080
/// use borsh::{BorshDeserialize, BorshSerialize};
/// use lee_core::program::{ReceiveInput, Response};
/// use lez_authority::{AuthoritySlot, Gated, HasSlot, InitGate, Policy, Refusal, Role, Wire};
///
/// struct Deputy;
///
/// impl Role for Deputy {
///     const NAME: &'static str = "deputy";
///     type Slot = AuthoritySlot;
/// }
///
/// impl<C: HasSlot<Deputy>> Policy<C> for Deputy {
///     const BY_NOMINATION_ONLY: bool = true;
/// }
///
/// struct Chief;
///
/// impl Role for Chief {
///     const NAME: &'static str = "chief";
///     type Slot = AuthoritySlot;
/// }
///
/// impl<C: HasSlot<Chief>> Policy<C> for Chief {}
///
/// #[derive(Default, BorshSerialize, BorshDeserialize, HasSlot)]
/// struct Config {
///     #[slot(Deputy)]
///     deputy: AuthoritySlot,
///     #[slot(Chief)]
///     chief: AuthoritySlot,
/// }
///
/// #[derive(BorshSerialize, BorshDeserialize, Gated)]
/// enum Message {
///     Ping,
/// }
///
/// fn handle(input: &ReceiveInput, message: Wire<Message>) -> Result<Response, Refusal> {
///     let seed = lez_authority::seed(b"deputy-first");
///     lez_authority::try_guard::<(Deputy, Chief), Config, Message>(
///         input,
///         &seed,
///         InitGate::Deployer,
///         message,
///         |_, _| Response::keep_state(),
///     )
/// }
///
/// let _reified: fn(&ReceiveInput, Wire<Message>) -> Result<Response, Refusal> = handle;
/// ```
pub struct NominationOnlyRootOfTwo;

/// A guard over two roles that both refuse program messages.
///
/// ```compile_fail,E0080
/// use borsh::{BorshDeserialize, BorshSerialize};
/// use lee_core::program::{ReceiveInput, Response};
/// use lez_authority::{AuthoritySlot, Gated, HasSlot, InitGate, Policy, Role, Wire};
///
/// struct Pauser;
///
/// impl Role for Pauser {
///     const NAME: &'static str = "pauser";
///     type Slot = AuthoritySlot;
/// }
///
/// impl<C: HasSlot<Pauser>> Policy<C> for Pauser {
///     const REFUSES_MESSAGES: bool = true;
/// }
///
/// struct Halter;
///
/// impl Role for Halter {
///     const NAME: &'static str = "halter";
///     type Slot = AuthoritySlot;
/// }
///
/// impl<C: HasSlot<Halter>> Policy<C> for Halter {
///     const REFUSES_MESSAGES: bool = true;
/// }
///
/// #[derive(Default, BorshSerialize, BorshDeserialize, HasSlot)]
/// struct Config {
///     #[slot(Pauser)]
///     pauser: AuthoritySlot,
///     #[slot(Halter)]
///     halter: AuthoritySlot,
/// }
///
/// #[derive(BorshSerialize, BorshDeserialize, Gated)]
/// enum Message {
///     Ping,
/// }
///
/// fn handle(input: &ReceiveInput, message: Wire<Message>) -> Response {
///     let seed = lez_authority::seed(b"two-refusing");
///     lez_authority::guard::<(Pauser, Halter), Config, Message>(
///         input,
///         &seed,
///         InitGate::Deployer,
///         message,
///         |_, _| Response::keep_state(),
///     )
/// }
///
/// let _reified: fn(&ReceiveInput, Wire<Message>) -> Response = handle;
/// ```
pub struct TwoRefusingRoles;

/// An `#[only]` marker naming a type that is no role.
///
/// ```compile_fail,E0277
/// use lez_authority::Gated;
///
/// #[derive(Gated)]
/// enum Message {
///     #[only(u64)]
///     Set(u64),
/// }
/// ```
pub struct OnlyNamingNoRole;

/// An `#[exempt]` marker naming a type that is no role.
///
/// ```compile_fail,E0277
/// use lez_authority::{AuthoritySlot, Gated};
///
/// #[derive(Gated)]
/// enum Message {
///     #[exempt(AuthoritySlot)]
///     Set(u64),
/// }
/// ```
pub struct ExemptNamingNoRole;

/// A marker naming a role the guard does not list, under a guard called by hand.
///
/// ```compile_fail,E0080
/// use borsh::{BorshDeserialize, BorshSerialize};
/// use lee_core::program::{ReceiveInput, Response};
/// use lez_authority::{AuthoritySlot, Gated, HasSlot, InitGate, Policy, Role, Wire};
///
/// struct Owner;
///
/// impl Role for Owner {
///     const NAME: &'static str = "owner";
///     type Slot = AuthoritySlot;
/// }
///
/// impl<C: HasSlot<Owner>> Policy<C> for Owner {}
///
/// struct Pauser;
///
/// impl Role for Pauser {
///     const NAME: &'static str = "pauser";
///     type Slot = AuthoritySlot;
/// }
///
/// #[derive(Default, BorshSerialize, BorshDeserialize, HasSlot)]
/// struct Config {
///     #[slot(Owner)]
///     owner: AuthoritySlot,
/// }
///
/// #[derive(BorshSerialize, BorshDeserialize, Gated)]
/// enum Message {
///     #[only(Pauser)]
///     Pause,
/// }
///
/// fn handle(input: &ReceiveInput, message: Wire<Message>) -> Response {
///     let seed = lez_authority::seed(b"owner-only");
///     lez_authority::guard::<(Owner,), Config, Message>(
///         input,
///         &seed,
///         InitGate::Deployer,
///         message,
///         |_, _| Response::keep_state(),
///     )
/// }
///
/// let _reified: fn(&ReceiveInput, Wire<Message>) -> Response = handle;
/// ```
pub struct MarkerNamingAnUnlistedRole;

/// An `#[exempt]` marker naming a role that refuses no program messages.
///
/// ```compile_fail,E0080
/// use borsh::{BorshDeserialize, BorshSerialize};
/// use lee_core::program::{ReceiveInput, Response};
/// use lez_authority::{AuthoritySlot, Gated, HasSlot, InitGate, Policy, Role, Wire};
///
/// struct Owner;
///
/// impl Role for Owner {
///     const NAME: &'static str = "owner";
///     type Slot = AuthoritySlot;
/// }
///
/// impl<C: HasSlot<Owner>> Policy<C> for Owner {}
///
/// #[derive(Default, BorshSerialize, BorshDeserialize, HasSlot)]
/// struct Config {
///     #[slot(Owner)]
///     owner: AuthoritySlot,
/// }
///
/// #[derive(BorshSerialize, BorshDeserialize, Gated)]
/// enum Message {
///     #[exempt(Owner)]
///     Ping,
/// }
///
/// fn handle(input: &ReceiveInput, message: Wire<Message>) -> Response {
///     let seed = lez_authority::seed(b"owner-exempt");
///     lez_authority::guard::<(Owner,), Config, Message>(
///         input,
///         &seed,
///         InitGate::Deployer,
///         message,
///         |_, _| Response::keep_state(),
///     )
/// }
///
/// let _reified: fn(&ReceiveInput, Wire<Message>) -> Response = handle;
/// ```
pub struct ExemptionFromARoleThatRefusesNothing;
