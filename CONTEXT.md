# lez-authority

Authority primitives for LEZ actor programs: a slot per role inside a program's config, the hop that carries a sender's authority from their own actor to the config, and the guard that runs a program's handler behind both. Policy-free: it does not know what an admin is.

## Language

**Config account**:
The program's one well-known account, a PDA of the program id and the program's seed. The config lives in the program's actor state on it.
_Avoid_: state account, admin account

**Uninitialized**:
A config account with no state yet. Only the install operation creates the config; the guard refuses every other message there.
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
The sender of a hopped message as the guard saw it: the seat's account and its via. Only the guard builds one.
_Avoid_: signer, origin

**Slot**:
One role's place in the config. Unset until a holder is installed, held, or renounced for good.
_Avoid_: authority config, admin config

**Role**:
What a slot stands for, such as admin. Names the slot in refusals.

**Guard**:
The library function that wraps a program's handler: it forwards at seats, unwraps hops at the config, runs management operations, and refuses a handler that changed a slot by other means than the context.
_Avoid_: dispatcher, framework, runtime

**Context**:
What the guard hands the handler: the caller, if any, and the operations on the slot.
_Avoid_: ctx in prose, session

**Hosted variant**:
A library variant a program places in its own message enum so it travels on the same wire: the management operations and the hop.
_Avoid_: injected instruction (the v0.2 word)

**Management operation**:
A hosted operation the guard runs on a slot without the handler: install, nominate, accept, renounce.
_Avoid_: admin instruction

**Nomination**:
The holder's standing offer of the role to one account, kept in the slot until that account accepts, the holder replaces or cancels it, or the holder renounces. The all-zero id cancels.
_Avoid_: pending transfer, candidate (the v0.2 word)

**Nominee**:
The account named in a nomination. It becomes the holder only by accepting through its own seat.
_Avoid_: pending holder

**Seed proof**:
What a PDA nominee's accept carries: the seed that, with the program that reached the seat, derives the nominee. Without it a program that merely inherited a grant could accept on the nominee's behalf.
_Avoid_: PDA claim

**Open message**:
A message the handler receives with no caller. Anyone may send it to the config account.
_Avoid_: public message

**Gated message**:
A message whose handler arm requires the holder through the context. Sent through the sender's seat.
_Avoid_: protected, restricted, admin-only (that is the admin crate's case)

**Hub**:
The config account acting for the holder: a gated arm sends to other actors of the program through the context, after the guard verified the holder once.
_Avoid_: dispatcher, relay

**Target**:
An actor of the program that receives a message from the hub. It accepts the message only from the config account itself, not from any actor of the program.
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
