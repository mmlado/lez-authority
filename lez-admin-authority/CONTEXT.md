# lez-admin-authority

A single admin per LEZ actor program: installed through the program's own message enum, handed over by nominate and accept, renounceable, and checked wherever the program's handler asks. The admin is a role of [lez-authority](https://github.com/mmlado/lez-authority), whose glossary applies here.

## Language

**Admin**:
The holder of the admin role: the one account, with its via, that gated messages require.
_Avoid_: owner, authority, superuser

**Init gate**:
Who may install the first admin: the deployer, or the genesis key when the program opts into one. Closes the race between deployment and the first `Init`.
_Avoid_: bootstrap, self-election (the v0.2 rule this replaces)

**Admin-only message**:
A gated message that needs the admin: marked `#[only(Admin)]`, so the guard checks the admin before the handler runs. The program decides which of its messages are admin-only.
_Avoid_: privileged instruction

**Handover**:
The only way the admin role moves: the admin nominates, the nominee accepts. There is no one-step transfer, because a PDA cannot co-sign a transaction and one shape must serve keys and PDAs alike.
_Avoid_: transfer, rotation

**Renounce**:
The admin empties the slot for good. No admin can be installed afterwards.
_Avoid_: revoke, burn, reset

**Sample**:
The reference program in this repo: two values, one open setter, one admin-only setter. It is the integrator's view of the library and the first thing every change must keep working.
_Avoid_: demo, example
