# A holder carries the program it acts through

A holder is an account together with its `via`, the program that reached the holder's seat when it was installed, or none for a key at top level. The guard accepts a caller only when both match, so a key holder can never be acted for by a program that merely carries its signature, and a PDA holder acts through the program that grants its seed and nothing else. LEZ's own `ping_receiver` keeps a separate config-wide `governance` program instead, which the authority's key may also act through. We chose the per-holder form because it needs nothing configured at init, moves with handover, and closes the lent-signature case by construction.

## Considered options

- **A `governance` field as in `ping_receiver`.** Lets a key holder also act through one program, with both the key's signature and that program required. A real capability, deliberately left out: it adds a field to configure, cannot change after init in ping's form, and reopens the lent-signature case for that program. If a consumer needs the conjunction, it is additive as a second optional program on the holder.
