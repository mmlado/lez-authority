# Tests run programs through LEZ's own executor

Each crate's tests delivered messages with a loop of their own. The loop decided who had authorized a delivery and which accounts a transaction could reach, so a test could pass on an input no chain produces, and each crate kept its own copy. The tests now run programs in a world. lez-authority-world, in `world/`, holds programs and their actor states and hands one public transaction at a time to `lee_core`'s executor. The executor delivers every message and decides authorization and admission, as it does in the sequencer. The world only runs the programs, keeps the states a transaction leaves, and registers the accounts it reached or signed for. A program is a function from the receive input to a response or an error, so the world knows nothing of the guard. A test states the actors and signers of its transaction, and helpers in each test file hide the common case. A test sets the states it starts with. After that, every change comes from a transaction the executor accepted. An input no transaction can carry, such as an own message the guard did not build, stays a direct call to the guard and says so.

The world runs one public sync transaction. It returns async messages undelivered, runs nothing private, charges no fee and checks no validity window. The crate is the repository's test support and is outside the versioning promise.

## Considered options

- **A delivery loop in the engine's own test support**, shared by the three crates. It removes the copies, but the rules of authorization and admission stay ours to keep in step with LEZ.
- **A local sequencer for every test.** It is the chain itself, and every test would need a running node. The samples' runners keep that role.
