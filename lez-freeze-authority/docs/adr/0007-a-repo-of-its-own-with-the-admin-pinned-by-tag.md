# The freeze role has a repo of its own and pins the admin by tag

RFP-002 has the admin authority change and revoke the freeze authority, so the freeze role's rules read the admin's slot and name the admin's role. The freeze role lives in a repo of its own, lez-freeze-authority, with a hard dependency on lez-admin-authority by git tag, as v0.2's freeze depended on v0.2's admin (v0.2 ADR-0001). RFP-002 then gets its own repo, README, sample and CI, as RFP-001 has. Cargo identifies a git dependency by its ref string. When a program pins a different admin tag from the one the freeze crate pins, the build holds two copies of the admin crate, and the program no longer compiles against the freeze role. Every admin release therefore needs a freeze release that re-pins it, and the README names the matching versions. A program depends on the freeze crate alone, which re-exports the admin and the engine, so it cannot mix their versions.

## Considered options

- **A second crate in lez-admin-authority's workspace,** depending on the admin by path. One tag would pin both crates, so a program could not mix their versions, and one CI would cover both. RFP-002's deliverable would live in RFP-001's repo, under the admin's name.
- **A repo generic over the role it answers to,** depending on the engine only. Admin releases would no longer force freeze releases, at the price of a parameter every program names, for a freeze that nothing on record asks to answer to anything but the admin.
