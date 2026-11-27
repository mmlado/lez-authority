# lez-authority

Authority primitives for LEZ actor programs: a slot per role inside a program's config, the hop that carries a sender's authority from their own actor to the config, and the guard in front of a program's plain handler. It does not know what an admin is.

## Language

**Config account**:
The program's one well-known account, a PDA of the program id and the program's seed. The config lives in the program's actor state on it.
_Avoid_: state account, admin account

**Uninitialized**:
A config account with no state yet. Only the root role's install creates the config. The guard refuses every other message there.
_Avoid_: empty config, default config

**Seat**:
A sender's own actor under the program, `(sender, program)`. The only place the runtime says whether the sender authorized the transaction.
_Avoid_: signer actor, caller actor

**Hop**:
The forward from a seat to the config account, carrying the original message and the program that reached the seat. Built only by the guard.
_Avoid_: relay, envelope, proxy

**Holder**:
Who holds a role: an account together with its via.
_Avoid_: owner, authority (the v0.2 word), admin (the role's name)

**Via**:
The program a holder acts through, or none for a key that signs at top level. A PDA holder's via is the program that grants its seed.
_Avoid_: governance, delegate

**Caller**:
The sender of a message as the guard saw it: in a hop, the seat's account and its via, and at a request for clearance, the sending actor's account alone, when that account authorized the transaction. Only the guard builds one.
_Avoid_: signer, origin

**Slot**:
One role's place in the config: its holder, and whatever else the role keeps there, such as the accounts a freeze authority froze. Unset until a holder is installed or a nominee accepts, then held, or renounced for good.
_Avoid_: authority config, admin config, role state

**Role**:
What a slot stands for, such as admin. Names the slot in refusals. A program may have more than one, each with its own name and slot.

**Root role**:
The first of a program's roles. Its install creates the config, and the guard's own refusals name it.
_Avoid_: main role, primary role

**Policy**:
A role's rules on its holder: who may install its first holder, who may nominate, who may renounce, and what renounce leaves. A role that answers to nobody keeps the defaults. A role that answers to another, as a freeze authority answers to the admin, reads that role's slot.
_Avoid_: permissions, access rules

**Guard**:
What runs in front of a program's handler: it forwards at seats, unwraps hops at the config, runs management operations, checks the holders a message declares, asks the role that refuses program messages whether one may run, hands a role its own message at the role's own actor, and refuses a handler that changed a slot. The guard attribute puts it there.
_Avoid_: dispatcher, framework, runtime

**Hook**:
A point in the guard where a role adds to what the guard does beside its policy, with the guard's own rules kept around it. A role's consent to a program message is one, an operation the role defines for itself is another, a role's own message handled at the role's own actor is a third, and a look at the state before and after the plain handler is a fourth. A consent and an operation may come with sync messages, which the config sends. A role that adds nothing to the guard, such as the admin, uses none.
_Avoid_: plugin, middleware, interceptor

**Plain handler**:
A LEZ program's own handler, taking the receive input and the program's message and returning the response, as it would without the library.
_Avoid_: callback, hook

**Context**:
What a role's policy and hooks see: the caller, if any, the config's actor, and the program's init gate. The handler never sees it.
_Avoid_: ctx in prose, session

**Library message**:
A message of the library's, not the program's: a role's management operation, the hop, a request for clearance, or a role's own message. It starts with the byte 255, so it travels next to the program's messages without the program's enum hosting it.
_Avoid_: hosted variant, injected instruction (the v0.2 word)

**Own message**:
A role's message from the config to one of the role's own actors, which that actor hands to the role with its own state, never to the handler. Only a role's consent or operation sends one.
_Avoid_: callback, role call

**Own actor**:
An actor of the program that only one role writes, through its own messages: the PDA of the program for a key the role chooses, with a seed that binds the role and the config. A freeze authority's marker for an account is one.
_Avoid_: role account, role PDA

**Management operation**:
A library message the guard runs at the config, without the handler: install, nominate and renounce on the role's slot under the role's policy, accept by the nominee, and the operations a role defines for itself, such as a freeze, through its hooks.
_Avoid_: admin instruction

**Nomination**:
A standing offer of the role to one account, made under the role's policy and kept in the slot until that account accepts, a later nomination replaces or cancels it, or the slot is renounced. A slot with no holder can carry one when the policy lets someone nominate there. The all-zero id cancels.
_Avoid_: pending transfer, candidate (the v0.2 word)

**Nominee**:
The account named in a nomination. It becomes the holder only by accepting through its own seat.
_Avoid_: pending holder

**Seed proof**:
What a PDA nominee's accept carries: the seed that, with the program that reached the seat, derives the nominee. Without it a program that merely inherited a grant could accept on the nominee's behalf.
_Avoid_: PDA claim

**Open message**:
A program message that needs no holder. It runs wherever it is sent, as in a program without the guard. At the config it needs an initialized config and the consent of the role that refuses program messages, unless it is exempt from it.
_Avoid_: public message

**Clearance**:
The config's consent to a program message handled at another actor of the program, asked in the same transaction when one of the program's roles requires it. A refusal reverts the whole transaction, the handler's effects included.
_Avoid_: admission (LEZ's word for admitting an account to a transaction), freeze check (one role's use of it)

**Gated message**:
A program message that declares it needs a role's holder. It travels through the sender's seat, and the guard checks the holder at the config before the handler runs.
_Avoid_: protected, restricted, admin-only (that is the admin crate's case)

**Config-only message**:
A program message that only the config account may send, as it does to the program's other actors.
_Avoid_: internal message, hub message

**Exempt message**:
A program message its program marked to run where one role's consent would refuse it, as an exempt message runs through a freeze. The mark names the role and covers that message only. It still needs its holders.
_Avoid_: whitelisted message, carve-out

**Hub**:
The config account acting for the holder: the handler of a gated message sends to other actors of the program, after the guard verified the holder once.
_Avoid_: dispatcher, relay

**Target**:
An actor of the program that receives a message from the hub. A config-only message runs there only when the config account itself sent it, not any actor of the program.
_Avoid_: callee, destination

**Refusal**:
A panic that stops the transaction, with a stable code in front of a sentence naming the role. A code never changes meaning.
_Avoid_: error (nothing is returned), exception

**Deployer**:
The holder of the key of a program's header account. On LEZ 0.4 that account is the program's id, so the deployer's seat is `(program, program)`.
_Avoid_: owner, uploader

**Genesis key**:
A key compiled into a program as the opt-in alternative to the deployer for installing the first holder, for programs whose header account is not a key.
_Avoid_: deployer key
