# Running the token sample on a local dev sequencer

What this run proves on a real sequencer: the admin's `Init` from the deployer, a mint the config credits to the owner's actor, the freeze authority appointed by the admin's offer and the nominee's accept, then replaced, revoked and appointed again, a mint by the admin to its own account, and that each refused call is refused with its code in the wallet's simulation, then included on chain without taking effect, with the same code in the sequencer's log.

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
just wallet-import-test-accounts             # two genesis-funded accounts, take the first as PAYER
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

The runner prints the program, deployer, config, authority, successor and stranger account ids, one `included in block` line per step with the gas the wallet declared, and the state read back after a step: the config, or a balance. A step the guard must refuse first prints `refused in simulation with` and the refusal's prefix and code, then goes on chain anyway. The runner stops at the first step that does not behave.

## 5. What to look for

- The deployment: one transaction per segment, then the header.
- `after init`: the admin slot held by the deployer, the freeze slot unset.
- `after the mint`: `Balance { amount: 5 }` at the authority's actor, where the config credited it.
- Six refused steps, each declaring the whole 2,000,000 gas budget: `a stranger's mint` with `lez-authority/admin E103`, `the freeze role's init` with `lez-authority/freeze E137`, `a stranger's nomination` with `lez-freeze-authority/freeze E208`, `a stranger's accept` with `lez-authority/freeze E114`, and `the replaced freeze authority's nomination` and `the revoked freeze authority's nomination` with `lez-freeze-authority/freeze E208`. The state read back right after each shows that it changed nothing.
- The freeze slot: offered to the authority, then held by the authority, held by the authority with the successor nominated, held by the successor, unset after the revoke, and held by the authority again.
- `after the second mint`: `Balance { amount: 8 }`. The config is as it was at the first mint, so the two mints' gas differs by what the balance costs.
- `after the admin's mint to itself`: `Balance { amount: 2 }` at the actor of the seat that sent the mint.
- In the sequencer terminal, each refused transaction's text with its code. The guest's panic line also prints in the runner's terminal, once per simulation.

## If something fails

- `SegmentChain(CountMismatch { .. })`: the runner counts segments from the user ELF inside the program binary. Report the numbers.
- `UnadmittedPublicActor`: the wallet did not attach the config PDA's evidence. Report it, that is a finding about `PublicPda` mentions.
- `ConflictingAccountIdentity`: the wallet refused a step that names one account twice with different signing, before simulating it. Report the step.
- `SimulatedRevert` on a step that should pass: the error carries the refusal text. Report it.
- `Transaction not found in preconfigured amount of blocks`: the sequencer dropped the transaction, which is its fee screening or an invalid input, not a revert. Check the payer's balance.
- A panic in the runner about `r0vm`: put it on the `PATH`.
