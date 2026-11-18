# Running the token sample on a local dev sequencer

What this run proves on a real sequencer: the admin's `Init` from the deployer, a mint the config credits to the owner's actor, the freeze authority appointed by the admin's offer and the nominee's accept, then replaced, revoked and appointed again, and a mint by the admin to its own account. Then a transfer between two owners, which the config clears, the wallet's measure of a transfer with and without the config actor, and two transfers refused: one that leaves the config actor out of the transaction and one the sender did not sign. Then a freeze: a mint and a transfer are refused while the admin's notice, marked exempt, still runs, the admin replaces the freeze authority while the program is frozen, and the successor's release lets a mint and a transfer run again. Each refused call is refused in the wallet's simulation, with its code where the guard refuses it, then included on chain without taking effect. The sequencer's log shows the same text whenever the guest refused it.

## 1. Sequencer

In a LEZ checkout at tag `v0.4.0-rc3`, inside `nix develop`:

```sh
just clean
RISC0_DEV_MODE=1 just run-sequencer-standalone     # 127.0.0.1:3040, no proving
```

Leave it running. A refused transaction's text appears in this terminal.

## 2. Wallet

In a second terminal, same checkout, same `nix develop`:

```sh
export LEE_WALLET_HOME_DIR=$PWD/lez/wallet/configs/debug
just run-wallet check-health                 # sets the wallet password after `just clean`
just wallet-import-test-accounts             # two genesis-funded accounts, take the first as PAYER, without its Public/ prefix
```

## 3. Guest binary

From this repo, with the risc0 toolchain installed through `rzup`:

```sh
cargo build --release --manifest-path sample/methods/Cargo.toml
ls sample/methods/target/riscv-guest/sample-methods/sample/riscv32im-risc0-zkvm-elf/release/sample.bin
```

## 4. Run

With the same `LEE_WALLET_HOME_DIR`, and `r0vm` on the `PATH`: the wallet runs each call through it before sending.

```sh
cargo run --release -p sample-runner -- \
  sample/methods/target/riscv-guest/sample-methods/sample/riscv32im-risc0-zkvm-elf/release/sample.bin \
  <PAYER>
```

The runner prints the program, deployer, config, authority, successor and stranger account ids, one `included in block` line per step with the gas the wallet declared, and the state read back after a step: the config, a balance, or the sender's and the recipient's balances after a transfer. A step that must be refused first prints `refused in simulation with` and the refusal's text, with its prefix and code where the guard refuses it, then is sent anyway with reverts allowed: the wallet simulates it a second time, and the runner prints its `included in block` line. The runner stops at the first step that does not behave.

## 5. What to look for

- The deployment: one transaction per segment, then the header.
- `after init`: the admin slot held by the deployer, the freeze slot unset.
- `after the mint`: `Balance { amount: 5 }` at the authority's actor, where the config credited it.
- Fourteen refused steps, each declaring the whole 2,000,000 gas budget: `a stranger's mint` with `lez-authority/admin E103`, `the freeze role's init` with `lez-authority/freeze E137`, `a stranger's nomination` with `lez-freeze-authority/freeze E208`, `a stranger's accept` with `lez-authority/freeze E114`, `the replaced freeze authority's nomination` and `the revoked freeze authority's nomination` with `lez-freeze-authority/freeze E208`, `a freeze with no freeze authority` with `lez-authority/freeze E131`, `a transfer without the config actor` with the runtime's `which is neither a declared public actor nor private`, `an unsigned transfer` with the guest's `the sender must sign the transfer`, `the admin's freeze` with `lez-authority/freeze E103`, `a mint while frozen` and `a transfer while frozen` with `lez-freeze-authority/freeze E217`, `a stranger's notice while frozen` with `lez-authority/admin E103`, and `the replaced freeze authority's release` with `lez-authority/freeze E103`. The state read back right after each shows that it changed nothing.
- The freeze slot: offered to the authority, then held by the authority, held by the authority with the successor nominated, held by the successor, unset after the revoke, held by the authority again, and, while frozen, held by the authority with the successor nominated and then held by the successor.
- `after the second mint`: `Balance { amount: 8 }`. The config is as it was at the first mint. The two mints' gas differs by 97 in runs 15 and 16, and by 566 and 568 in runs 13 and 14, before the credit asked for clearance.
- `after the admin's mint to itself`: `Balance { amount: 2 }` at the actor of the seat that sent the mint.
- `a transfer` of 3 from the authority to the successor: `Balance { amount: 5 }` at the authority's actor and `Balance { amount: 3 }` at the successor's. Both actors ask the config for clearance, which the guest's host tests pin. While frozen, the credit's request at the recipient's actor refuses first, so the live run shows that one only. The two refused transfers after it and the one while frozen leave both balances as they were.
- `measure a transfer` and `measure it without the config actor`: the wallet's dry runs of a transfer of 1, neither sent, each with its wire bytes and its fee at the head. The first prints its cycles. The second reverts with the runtime's `UndeclaredActor`, so it prints the whole budget a failed action is charged in place of its cycles. Only the wire bytes compare: their difference is what naming the config actor adds to a transaction.
- `after the freeze`: `frozen: true` in the freeze slot, and it stays set through the notice and the replacement.
- `after the notice`: `notice: "frozen for review"` on the frozen config, posted by the admin through the freeze.
- `after the replacement while frozen`: the freeze slot held by the successor, still frozen.
- `after the release`: `frozen: false`, with the notice still there until `after the notice is cleared`.
- `after the mint after the release`: `Balance { amount: 6 }`. Then `a transfer after the release` of 2 from the successor's actor: `Balance { amount: 1 }` at the successor's and `Balance { amount: 8 }` at the authority's.
- In the sequencer terminal, the text of each refusal the guest made, with its code where the guard refused. The transfer without the config actor prints no refusal text there, since the runtime refuses it and not the guest. The guest's panic line also prints in the runner's terminal twice for each step the guest refuses, once for each simulation.

## If something fails

- `payer account id: InvalidBase58(..)`: pass the payer's id without the wallet's `Public/` prefix.
- `SegmentChain(CountMismatch { .. })`: the runner counts segments from the user ELF inside the program binary. Report the numbers.
- `UnadmittedPublicActor`: the wallet did not attach the config PDA's evidence. Report it, that is a finding about `PublicPda` mentions.
- `ConflictingAccountIdentity`: the wallet refused a step that names one account twice with different signing, before simulating it. Report the step.
- `SimulatedRevert` on a step that should pass: the error carries the refusal text. Report it.
- `SimulatedRevert` with `which is neither a declared public actor nor private` on a step that should pass: the runtime's `UndeclaredActor`. The step left out an actor its messages reach, such as the config a request for clearance goes to. Report the step.
- `Transaction not found in preconfigured amount of blocks`: the sequencer dropped the transaction, which is its fee screening or an invalid input, not a revert. Check the payer's balance.
- A panic in the runner about `r0vm`: put it on the `PATH`.
