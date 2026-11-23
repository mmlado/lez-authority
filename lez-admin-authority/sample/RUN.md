# Running the sample on a local dev sequencer

What this run proves on a real sequencer: the deployer gate, the admin check, the three hops to a target, the handover and renounce, and that each refused call is refused with its code in the wallet's simulation, then included on chain without taking effect, with the same code in the sequencer's log.

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

The runner prints the program, deployer, config, stranger and nominee account ids, one `included in block` line per step with the gas the wallet declared, and the config read back after each step. A step the guard must refuse first prints `refused in simulation with E<code>`, then is sent anyway with reverts allowed: the wallet simulates it a second time, and the runner prints its `included in block` line. The runner stops at the first step that does not behave.

## 5. What to look for

- The deployment: one transaction per segment, then the header.
- Five refused steps: `stranger init` with E117, `stranger`, `nominee before accept` and `old admin` with E103, `after renounce` with E102. Each declares the whole 2,000,000 gas budget, and its read-back is unchanged.
- `after init`: the admin slot held by the deployer, both values 0.
- `the target's state`: `TargetState { value: 5 }` on the stranger's account.
- In the sequencer terminal, each refused transaction's text with its code. The guest's panic line also prints in the runner's terminal twice per refused step, once for each simulation.

## If something fails

- `payer account id: InvalidBase58(..)`: pass the payer's id without the wallet's `Public/` prefix.
- `SegmentChain(CountMismatch { .. })`: the runner counts segments from the user ELF inside the program binary. Report the numbers.
- `UnadmittedPublicActor`: the wallet did not attach the config PDA's evidence. Report it, that is a finding about `PublicPda` mentions.
- `SimulatedRevert` on a step that should pass: the error carries the refusal text. Report it.
- `Transaction not found in preconfigured amount of blocks`: the sequencer dropped the transaction, which is its fee screening or an invalid input, not a revert. Check the payer's balance.
- A panic in the runner about `r0vm`: put it on the `PATH`.
