# A freeze reaches every actor, and a frozen account is a caller

RFP-002 asks for a freeze that rejects any attempt to interact with the program apart from unfreezing or changing the freeze authority, and for freezing one account while the rest of the program keeps running. On LEZ 0.4 a program message runs at any actor of the program, and only the config sees the freeze. A freeze refuses every program message wherever it is sent, except library messages and exempt messages, through the config's clearance (lez-authority [ADR-0005](https://github.com/mmlado/lez-authority/blob/main/docs/adr/0005-a-message-handled-elsewhere-asks-the-config-for-clearance.md)). A frozen account is refused wherever it is a message's caller. A message without a caller, such as a credit at its recipient's actor, gets only the program-wide check. This keeps the strict reading of v0.2, its ADR-0003, and drops its forced signature. On 0.4 a signature shows only at the signer's own actor, so forcing one costs a hop for every message. It stops nobody, since a frozen account's owner can sign with another key. And a message that cannot have a caller would have to be exempt, which takes it out of the program-wide freeze as well.

## Considered options

- **The config only.** No cost, but a program that keeps state at other actors keeps running through a freeze.
- **A caller on every freezable message,** as the auto mode of v0.2 injected a signer into every gated instruction.
- **The caller and an account the message names,** as the explicit target of the v0.2 proposal allowed. v0.2 dropped it, and it can come back as a marker if an integrator asks for it.
