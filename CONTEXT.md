# lez-authority

Authority primitives for LEZ actor programs: a slot per role inside a program's config, the hop that carries a sender's authority from their own actor to the config, and the guard in front of a program's plain handler. Policy-free: it does not know what an admin is.

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
The sender of a message as its seat vouched for it: the seat's account, when that account authorized the message, and its via. Only the guard builds one, and it travels in a hop or a request for clearance.
_Avoid_: signer, origin

**Slot**:
One role's place in the config: its holder, and whatever else the role keeps there, such as the accounts a freeze authority froze. Unset until a holder is installed or a nominee accepts, then held, or renounced for good.
_Avoid_: authority config, admin config, role state

**Role**:
What a slot stands for, such as admin. Names the slot in refusals. A program may have more than one, each with its own slot.

**Root role**:
The first of a program's roles. Its install creates the config, and the guard's own refusals name it.
_Avoid_: main role, primary role

**Policy**:
A role's rules: who may install its first holder, who may nominate, who may renounce, and what renounce leaves. A role that answers to nobody keeps the defaults. A role that answers to another, as a freeze authority answers to the admin, reads that role's slot.
_Avoid_: permissions, access rules

**Guard**:
What runs in front of a program's handler: it forwards at seats, unwraps hops at the config, runs management operations, checks the holders a message declares, and refuses a handler that changed a slot. The guard attribute puts it there.
_Avoid_: dispatcher, framework, runtime

**Plain handler**:
A LEZ program's own handler, taking the receive input and the program's message and returning the response, as it would without the library.
_Avoid_: callback, hook

**Context**:
What a role's policy sees: the caller, if any, and the program's init gate. The handler never sees it.
_Avoid_: ctx in prose, session

**Library message**:
A message of the library's, not the program's: a role's management operation, the hop, or a request for clearance. It starts with the byte 255, so it travels next to the program's messages without the program's enum hosting it.
_Avoid_: hosted variant, injected instruction (the v0.2 word)

**Management operation**:
A library message the guard runs at the config under its role's policy, without the handler: install, nominate, accept and renounce on the role's slot, and the operations a role defines for itself, such as a freeze.
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
A program message that needs no holder. It runs wherever it is sent, as in a program without the guard. At the config it needs an initialized config.
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
