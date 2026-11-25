# The freeze role answers to the admin directly

RFP-002 has the admin authority change and revoke the freeze authority, so the freeze role's rules read the admin's slot and name the admin's role. lez-freeze-authority depends on lez-admin-authority and re-exports it with the engine, as v0.2's freeze depended on v0.2's admin (spel-freeze-authority ADR-0001). A program depends on the freeze crate alone and gets both roles from it. The three crates share one repository and one version (lez-authority ADR-0007), so the freeze always builds against the admin of its own release.

## Considered options

- **A role generic over the one it answers to,** depending on the engine only. The freeze would not tie a program to the admin crate, at the price of a parameter every program names, for a freeze that nothing on record asks to answer to anything but the admin.
