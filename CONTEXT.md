# lez-freeze-authority

A freeze authority for LEZ actor programs: one account that can freeze the whole program, or single accounts, and release them. It is a role of [lez-authority](https://github.com/mmlado/lez-authority) that answers to the admin of [lez-admin-authority](https://github.com/mmlado/lez-admin-authority), whose glossaries apply here.

## Language

**Freeze authority**:
The holder of the freeze role: the one account, with its via, that freeze operations require. It takes the role by accepting a nomination from the admin or from the freeze authority before it.
_Avoid_: pauser, freezer, guardian

**Freeze slot**:
The freeze role's slot in the config: the freeze authority, whether the program is frozen, and the frozen accounts. It is born with the config, with no freeze authority and nothing frozen.
_Avoid_: freeze config (the v0.2 word), freeze state

**Freeze**:
The program-wide state in which every program message is refused, wherever it is sent, except exempt messages. A freeze never refuses a management operation.
_Avoid_: pause, halt, program-wide freeze

**Frozen account**:
An account the freeze authority froze. Every program message with that account as its caller is refused, except exempt messages. Its management operations still run.
_Avoid_: blocked account, blocklisted account

**Freeze operation**:
A management operation of the freeze role: freezing or releasing the program, or one account. The guard runs it at the config, and a freeze never refuses it.
_Avoid_: freeze instruction (the v0.2 word), freeze message

**Release**:
The end of a freeze, or of one account's freeze, by a freeze operation.
_Avoid_: unfreeze, thaw, resume

**Freezable message**:
Any program message that is not exempt. A freeze refuses it, and so does a frozen account that is its caller.
_Avoid_: gated message (the engine's word for a message that needs a holder), frozen message

**Exempt message**:
A program message its program marked to run through a freeze. The mark covers that message only, not the messages its handler sends.
_Avoid_: carve-out, whitelisted message
