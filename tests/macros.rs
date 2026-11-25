//! The macros write what a program would write by hand: the derived impls answer as their
//! hand-written twins do, and a handler under the attribute answers every input as the hand
//! expansion does, refusals included.

use std::panic::{AssertUnwindSafe, catch_unwind};

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor, ActorState};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_authority::{
    AuthoritySlot, Gated, HasSlot, Holder, Hooks, InitGate, Op, Policy, Role, Roles, Slot, Wire,
    config_account,
};

struct First;

impl Role for First {
    const NAME: &'static str = "first";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<First>> Policy<C> for First {}
impl<C: HasSlot<First>> Hooks<C> for First {}

struct Second;

impl Role for Second {
    const NAME: &'static str = "second";
    type Slot = AuthoritySlot;
}

// The messages below are exempt from it, so it declares that it refuses, with no rules yet.
impl<C: HasSlot<Second>> Policy<C> for Second {}

impl<C: HasSlot<Second>> Hooks<C> for Second {
    const REFUSES_MESSAGES: bool = true;
}

/// A role that keeps more than its holder, as a freeze authority keeps what it froze.
struct Third;

impl Role for Third {
    const NAME: &'static str = "third";
    type Slot = Tagged;
}

impl<C: HasSlot<Third>> Policy<C> for Third {}
impl<C: HasSlot<Third>> Hooks<C> for Third {}

#[derive(Debug, Default, PartialEq, Eq)]
struct Tagged {
    authority_slot: AuthoritySlot,
    tag: u8,
}

impl Slot for Tagged {
    fn authority_slot(&self) -> &AuthoritySlot {
        &self.authority_slot
    }
    fn authority_slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.authority_slot
    }
}

#[derive(Default, HasSlot)]
struct Mixed {
    #[slot(First)]
    first: AuthoritySlot,
    #[slot(Third)]
    third: Tagged,
}

const PROGRAM: AccountId = AccountId::new([0xA0; 32]);
const KEY: AccountId = AccountId::new([1; 32]);
const SEED: PdaSeed = lez_authority::seed(b"macros-test/v1");

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, HasSlot)]
struct Config {
    value: u64,
    #[slot(First)]
    first: AuthoritySlot,
    #[slot(Second)]
    second: AuthoritySlot,
}

#[derive(Clone, BorshSerialize, BorshDeserialize, HasSlot)]
struct Pair(u64, #[slot(First)] AuthoritySlot);

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize, Gated)]
enum Message {
    Open(u64),
    #[only(First)]
    OnlyFirst(u64),
    #[only(Second)]
    OnlySecond {
        value: u64,
    },
    #[from_config]
    Mark,
    #[exempt(Second)]
    Exempt(u64),
    #[only(First)]
    #[exempt(Second)]
    OnlyFirstExempt(u64),
    #[from_config]
    #[exempt(Second)]
    MarkExempt,
}

/// The same enum, its `Gated` written by hand.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
enum ByHand {
    Open(u64),
    OnlyFirst(u64),
    OnlySecond { value: u64 },
    Mark,
    Exempt(u64),
    OnlyFirstExempt(u64),
    MarkExempt,
}

impl Gated for ByHand {
    const MARKED: &'static [&'static str] = &[First::NAME, Second::NAME];
    const EXEMPTED: &'static [&'static str] = &[Second::NAME];

    fn needs<R: Role>(&self) -> bool {
        match self {
            ByHand::OnlyFirst(_) | ByHand::OnlyFirstExempt(_) => R::NAME == First::NAME,
            ByHand::OnlySecond { .. } => R::NAME == Second::NAME,
            ByHand::Open(_) | ByHand::Mark | ByHand::Exempt(_) | ByHand::MarkExempt => false,
        }
    }

    fn only_from_config(&self) -> bool {
        matches!(self, ByHand::Mark | ByHand::MarkExempt)
    }

    fn exempt<R: Role>(&self) -> bool {
        matches!(
            self,
            ByHand::Exempt(_) | ByHand::OnlyFirstExempt(_) | ByHand::MarkExempt
        ) && R::NAME == Second::NAME
    }
}

#[test]
fn every_derived_message_declares_what_its_hand_written_twin_does() {
    let pairs = [
        (Message::Open(1), ByHand::Open(1)),
        (Message::OnlyFirst(2), ByHand::OnlyFirst(2)),
        (
            Message::OnlySecond { value: 3 },
            ByHand::OnlySecond { value: 3 },
        ),
        (Message::Mark, ByHand::Mark),
        (Message::Exempt(5), ByHand::Exempt(5)),
        (Message::OnlyFirstExempt(6), ByHand::OnlyFirstExempt(6)),
        (Message::MarkExempt, ByHand::MarkExempt),
    ];
    assert_eq!(<Message as Gated>::MARKED, <ByHand as Gated>::MARKED);
    assert_eq!(<Message as Gated>::EXEMPTED, <ByHand as Gated>::EXEMPTED);
    for (derived, by_hand) in &pairs {
        assert_eq!(
            borsh::to_vec(derived).unwrap(),
            borsh::to_vec(by_hand).unwrap()
        );
        assert_eq!(derived.needs::<First>(), by_hand.needs::<First>());
        assert_eq!(derived.needs::<Second>(), by_hand.needs::<Second>());
        assert_eq!(derived.only_from_config(), by_hand.only_from_config());
        assert_eq!(derived.exempt::<First>(), by_hand.exempt::<First>());
        assert_eq!(derived.exempt::<Second>(), by_hand.exempt::<Second>());
    }
}

#[test]
fn the_derive_names_each_marked_role_once_for_the_guard_to_check() {
    assert_eq!(<Message as Gated>::MARKED, [First::NAME, Second::NAME]);
    const {
        assert!(<(First, Second) as Roles<Config, Message>>::MARKS_LISTED);
        assert!(!<(First,) as Roles<Config, Message>>::MARKS_LISTED);
        assert!(!<(First, Third) as Roles<Mixed, Message>>::MARKS_LISTED);
    }
}

#[derive(Gated)]
#[allow(dead_code)]
enum ExemptOnly {
    #[exempt(Second)]
    Set,
}

#[test]
fn a_role_named_only_by_an_exemption_is_marked_too() {
    assert_eq!(<ExemptOnly as Gated>::MARKED, [Second::NAME]);
    assert_eq!(<ExemptOnly as Gated>::EXEMPTED, [Second::NAME]);
    assert_eq!(<Message as Gated>::EXEMPTED, [Second::NAME]);
    const {
        assert!(!<(First,) as Roles<Config, ExemptOnly>>::MARKS_LISTED);
    }
}

#[derive(Gated)]
#[allow(dead_code)]
enum ExemptThird {
    #[exempt(Third)]
    Set,
}

#[derive(Gated)]
#[allow(dead_code)]
enum ExemptFirst {
    #[exempt(First)]
    Set,
}

#[test]
fn an_exemption_from_a_role_that_refuses_nothing_is_caught() {
    const {
        assert!(<(First, Second) as Roles<Config, Message>>::EXEMPTIONS_REFUSE);
        assert!(<(First, Second) as Roles<Config, ExemptFirst>>::MARKS_LISTED);
        assert!(!<(First, Second) as Roles<Config, ExemptFirst>>::EXEMPTIONS_REFUSE);
        assert!(<(First, Third) as Roles<Mixed, ExemptThird>>::MARKS_LISTED);
        assert!(!<(First, Third) as Roles<Mixed, ExemptThird>>::EXEMPTIONS_REFUSE);
    }
}

// A second type under each listed role's name, as a role crate linked twice gives a program.
mod copy {
    use lez_authority::{AuthoritySlot, Role};

    pub struct First;

    impl Role for First {
        const NAME: &'static str = "first";
        type Slot = AuthoritySlot;
    }

    pub struct Second;

    impl Role for Second {
        const NAME: &'static str = "second";
        type Slot = AuthoritySlot;
    }
}

#[derive(BorshSerialize, BorshDeserialize, Gated)]
enum Copied {
    #[only(copy::First)]
    #[exempt(copy::Second)]
    SetValue(u64),
}

#[test]
fn a_role_linked_twice_is_one_role_to_the_guard() {
    assert!(Copied::SetValue(5).needs::<First>());
    assert!(Copied::SetValue(5).exempt::<Second>());
    let message = Wire::Program(Copied::SetValue(5));
    let held = Config {
        first: AuthoritySlot::held(Holder {
            account: KEY,
            via: None,
        }),
        ..Config::default()
    };
    let straight = ReceiveInput {
        receiver: Actor::new(config_account(PROGRAM, &SEED), PROGRAM),
        from: None,
        is_authorized: false,
        pre_state: ActorState::from(borsh::to_vec(&held).unwrap()),
        message: borsh::to_vec(&message).unwrap(),
    };
    let refused = lez_authority::try_guard::<(First, Second), Config, Copied>(
        &straight,
        &SEED,
        InitGate::Deployer,
        message,
        |_, _| Response::keep_state(),
    )
    .err()
    .map(|refusal| refusal.text());
    assert_eq!(
        refused.as_deref(),
        Some(
            "lez-authority/first E122: a message that needs the first must go through the sender's own actor"
        )
    );
}

// A program's own items named like the derive's internals, and a role that is the enum's own.
#[allow(dead_code, non_upper_case_globals)]
mod clashing {
    use super::{First, Second};
    use lez_authority::{Gated, Role};

    static role: u8 = 1;
    const id: u8 = 2;

    #[derive(Gated)]
    pub enum Message<__R> {
        #[only(First)]
        #[exempt(Second)]
        Set(__R),
    }

    #[derive(Gated)]
    pub enum ByRole<R: Role> {
        #[only(R)]
        Set(core::marker::PhantomData<R>),
    }
}

#[test]
fn the_derive_compiles_beside_items_named_like_its_internals() {
    let message = clashing::Message::Set(1u8);
    assert!(message.needs::<First>());
    assert!(!message.needs::<Second>());
    assert!(message.exempt::<Second>());
    let by_role = clashing::ByRole::<Second>::Set(core::marker::PhantomData);
    assert!(by_role.needs::<Second>());
    assert_eq!(<clashing::ByRole<Second> as Gated>::MARKED, [Second::NAME]);
}

#[test]
fn each_marked_field_is_its_role_s_slot() {
    let holder = |byte| {
        AuthoritySlot::held(Holder {
            account: AccountId::new([byte; 32]),
            via: None,
        })
    };
    let mut config = Config {
        value: 0,
        first: holder(1),
        second: holder(2),
    };
    assert_eq!(HasSlot::<First>::slot(&config), &holder(1));
    assert_eq!(HasSlot::<Second>::slot(&config), &holder(2));
    *HasSlot::<Second>::slot_mut(&mut config) = AuthoritySlot::Renounced;
    assert_eq!(config.second, AuthoritySlot::Renounced);
    assert_eq!(config.first, holder(1));

    let pair = Pair(7, holder(1));
    assert_eq!(HasSlot::<First>::slot(&pair), &holder(1));
}

#[test]
fn a_marked_field_may_hold_a_role_s_own_slot_type() {
    let mut config = Mixed::default();
    HasSlot::<Third>::slot_mut(&mut config).tag = 3;
    assert_eq!(HasSlot::<Third>::slot(&config).tag, 3);
    assert_eq!(
        HasSlot::<Third>::slot(&config).authority_slot(),
        &AuthoritySlot::Unset
    );
    assert_eq!(HasSlot::<First>::slot(&config), &AuthoritySlot::Unset);
}

// Configs a macro_rules! writes, which the derive's spans must not break.
macro_rules! declare {
    ($slot:ident) => {
        #[derive(Default, HasSlot)]
        struct Declared {
            #[slot(First)]
            first: $slot,
        }
    };
}

declare!(AuthoritySlot);

macro_rules! with_slot {
    ($item:item) => {
        #[derive(Default, HasSlot)]
        $item
    };
}

with_slot! {
    struct Wrapped {
        #[slot(First)]
        first: AuthoritySlot,
    }
}

with_slot! {
    struct WrappedTuple(#[slot(First)] AuthoritySlot);
}

#[test]
fn a_config_a_macro_writes_derives_its_slot() {
    assert_eq!(
        HasSlot::<First>::slot(&Declared::default()),
        &AuthoritySlot::Unset
    );
    assert_eq!(
        HasSlot::<First>::slot(&Wrapped::default()),
        &AuthoritySlot::Unset
    );
    assert_eq!(
        HasSlot::<First>::slot(&WrappedTuple::default()),
        &AuthoritySlot::Unset
    );
}

/// The program's own code, shared by both handlers below.
fn body(input: &ReceiveInput, message: Message) -> Response {
    let pre: &[u8] = &input.pre_state;
    if let Message::Mark | Message::MarkExempt = message {
        return Response::set_state(vec![9]);
    }
    let mut config: Config = if pre.is_empty() {
        Config::default()
    } else {
        borsh::from_slice(pre).unwrap()
    };
    match message {
        Message::Open(value)
        | Message::OnlyFirst(value)
        | Message::OnlySecond { value }
        | Message::Exempt(value)
        | Message::OnlyFirstExempt(value) => {
            config.value = value;
        }
        Message::Mark | Message::MarkExempt => unreachable!(),
    }
    Response::set_state(borsh::to_vec(&config).unwrap())
}

#[lez_authority::guard(roles(First, Second), config = Config, seed = SEED)]
fn derived(input: &ReceiveInput, message: Message) -> Response {
    body(input, message)
}

fn by_hand(input: &ReceiveInput, message: Wire<Message>) -> Response {
    fn by_hand(input: &ReceiveInput, message: Message) -> Response {
        body(input, message)
    }
    lez_authority::guard::<(First, Second), Config, Message>(
        input,
        &SEED,
        InitGate::Deployer,
        message,
        by_hand,
    )
}

/// What a handler answered: the response's bytes, or the panic's text.
fn answer(
    handler: fn(&ReceiveInput, Wire<Message>) -> Response,
    input: &ReceiveInput,
) -> Result<Vec<u8>, String> {
    let message: Wire<Message> = borsh::from_slice(&input.message).unwrap();
    catch_unwind(AssertUnwindSafe(|| handler(input, message)))
        .map(|response| borsh::to_vec(&response).unwrap())
        .map_err(|panic| {
            panic
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "a panic without text".into())
        })
}

fn input(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: Option<&Config>,
    message: &Wire<Message>,
) -> ReceiveInput {
    ReceiveInput {
        receiver,
        from,
        is_authorized: authorized,
        pre_state: ActorState::from(pre.map(|c| borsh::to_vec(c).unwrap()).unwrap_or_default()),
        message: borsh::to_vec(message).unwrap(),
    }
}

fn forwarded(message: &Wire<Message>) -> Wire<Message> {
    Wire::Library(lez_authority::LibraryMessage::Forwarded(
        lez_authority::Forwarded {
            via: None,
            inner: borsh::to_vec(message).unwrap(),
        },
    ))
}

#[test]
fn a_handler_under_the_attribute_answers_as_the_hand_expansion_does() {
    let config_actor = Actor::new(config_account(PROGRAM, &SEED), PROGRAM);
    let seat = |account| Actor::new(account, PROGRAM);
    let held = Config {
        value: 0,
        first: AuthoritySlot::held(Holder {
            account: PROGRAM,
            via: None,
        }),
        second: AuthoritySlot::Unset,
    };
    let init = Wire::op::<First>(Op::Init);
    let nominate = Wire::op::<First>(Op::Nominate { next: KEY });
    let inputs = [
        input(seat(PROGRAM), None, true, None, &init),
        input(seat(PROGRAM), None, false, None, &init),
        input(
            config_actor,
            Some(seat(PROGRAM)),
            false,
            None,
            &forwarded(&init),
        ),
        input(
            config_actor,
            Some(seat(KEY)),
            false,
            None,
            &forwarded(&init),
        ),
        input(
            config_actor,
            Some(seat(PROGRAM)),
            false,
            Some(&held),
            &forwarded(&nominate),
        ),
        input(config_actor, None, false, Some(&held), &nominate),
        input(
            config_actor,
            None,
            false,
            Some(&held),
            &Message::Open(4).into(),
        ),
        input(config_actor, None, false, None, &Message::Open(4).into()),
        input(seat(KEY), None, false, None, &Message::Open(5).into()),
        input(
            seat(PROGRAM),
            None,
            true,
            None,
            &Message::OnlyFirst(6).into(),
        ),
        input(
            config_actor,
            Some(seat(PROGRAM)),
            false,
            Some(&held),
            &forwarded(&Message::OnlyFirst(6).into()),
        ),
        input(
            config_actor,
            Some(seat(KEY)),
            false,
            Some(&held),
            &forwarded(&Message::OnlyFirst(6).into()),
        ),
        input(
            config_actor,
            None,
            false,
            Some(&held),
            &Message::OnlySecond { value: 7 }.into(),
        ),
        input(
            seat(KEY),
            Some(config_actor),
            false,
            None,
            &Message::Mark.into(),
        ),
        input(seat(KEY), None, true, None, &Message::Mark.into()),
    ];
    for input in &inputs {
        assert_eq!(answer(derived, input), answer(by_hand, input), "{input:?}");
    }
    let installed = answer(derived, &inputs[2]).unwrap();
    assert!(!installed.is_empty());
    assert_eq!(
        answer(derived, &inputs[3]).unwrap_err(),
        "lez-authority/first E117: only the deployer installs the first first: the program's own account, through its seat at top level"
    );
}

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, HasSlot)]
struct One {
    #[slot(First)]
    first: AuthoritySlot,
}

#[derive(Clone, BorshSerialize, BorshDeserialize, Gated)]
enum Nothing {
    Open,
}

#[lez_authority::guard(roles(First), config = One, seed = SEED, genesis = KEY)]
fn with_genesis(_input: &ReceiveInput, _message: Nothing) -> Response {
    Response::keep_state()
}

#[test]
fn a_genesis_key_replaces_the_deployer() {
    let config_actor = Actor::new(config_account(PROGRAM, &SEED), PROGRAM);
    let init = Wire::<Nothing>::op::<First>(Op::Init);
    let hop = Wire::<Nothing>::Library(lez_authority::LibraryMessage::Forwarded(
        lez_authority::Forwarded {
            via: None,
            inner: borsh::to_vec(&init).unwrap(),
        },
    ));
    let at_config = |sender: AccountId| ReceiveInput {
        receiver: config_actor,
        from: Some(Actor::new(sender, PROGRAM)),
        is_authorized: false,
        pre_state: ActorState::from(Vec::new()),
        message: borsh::to_vec(&hop).unwrap(),
    };
    let response = with_genesis(&at_config(KEY), hop.clone());
    let one: One = borsh::from_slice(response.post_state.as_ref().unwrap()).unwrap();
    assert_eq!(
        one.first,
        AuthoritySlot::held(Holder {
            account: KEY,
            via: None
        })
    );
    let refused = catch_unwind(AssertUnwindSafe(|| {
        with_genesis(&at_config(PROGRAM), hop.clone())
    }));
    let text = refused
        .err()
        .unwrap()
        .downcast_ref::<String>()
        .cloned()
        .unwrap();
    assert!(text.starts_with("lez-authority/first E115"), "{text}");
}
