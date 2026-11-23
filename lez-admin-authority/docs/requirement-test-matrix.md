# Requirement-to-test matrix, RFP-001 on LEZ 0.4

Every hard requirement of [RFP-001](https://github.com/logos-co/rfp/blob/main/RFP-001-admin-authority-lib.md) and every scenario of the proposal, with what satisfies it in this repo and in [lez-authority](https://github.com/mmlado/lez-authority). Test names are exact. `sample` names a test in [`sample/src/main.rs`](../sample/src/main.rs), `minimal` one in [`sample-minimal/src/main.rs`](../sample-minimal/src/main.rs), `engine` one in lez-authority's unit tests under `src/`, the macro tests in its `tests/macros.rs`, and `runner` a step of [`sample/runner`](../sample/runner/src/main.rs) as run on a local dev sequencer at LEZ `v0.4.0-rc3` on 2026-11-11. Regenerate the coverage claim with `cargo test --workspace --exclude sample-runner --locked` here and `cargo test --workspace --locked` in lez-authority.

## Hard requirements, Functionality

| # | Requirement | Satisfied by | Kind |
| --- | --- | --- | --- |
| F1 | Admin authority is set at program initialisation | sample `init_through_the_seat_installs_the_sender_as_admin`, `with_a_genesis_key_that_key_installs_itself`, engine `the_deployer_installs_itself_and_the_slot_must_stay_that_way`, `with_a_genesis_key_only_that_key_installs`, runner `init` | test, run |
| F2 | Admin authority can transfer to a new signer | sample `the_admin_nominates_and_nothing_moves_yet`, `the_nominee_accepts_and_the_old_admin_is_out`, `a_pda_nominee_accepts_through_its_program_with_the_seed`, engine `nominate_records_replaces_and_cancels`, `a_key_nominee_accepts_at_top_level`, runner `nominate`, `accept`, `new admin setter`, `old admin` | test, run |
| F3 | Admin authority can revoke, renouncing control | sample `renounce_empties_the_slot_and_keeps_the_values`, `a_renounced_admin_refuses_every_change`, `init_after_renounce_is_refused`, `renounce_drops_the_nomination_too`, engine `accept_after_renounce_is_refused`, `unset_and_renounced_slots_refuse_everyone`, runner `renounce`, `after renounce` | test, run |
| F4 | Only the admin calls privileged instructions | sample `the_admin_sets_the_admin_value`, `a_signed_stranger_cannot_set_the_admin_value`, `an_admin_message_sent_straight_to_the_config_is_refused`, `a_change_entered_from_another_program_is_refused`, `a_pda_admin_cannot_act_at_top_level`, minimal `a_stranger_is_refused`, engine `a_key_holder_acts_at_top_level_only`, runner `stranger`, `nominee before accept` | test, run |

## Hard requirements, Usability

| # | Requirement | Satisfied by | Kind |
| --- | --- | --- | --- |
| U1 | Integrated so that programs enable it with minimal boilerplate | The program stays a plain LEZ program: a slot field marked `#[slot(Admin)]`, `#[only(Admin)]` on each admin-only message, and the `admin` attribute on its handler. The derives and the attribute write what a program may also write by hand. `sample-minimal` and `ci/consumer-fixture` are the whole integration. The RFP names SPEL: the 0.4 libraries are SPEL-independent by request, level 2 makes roles compose through traits, with derives over them, and level 3 (SPEL) is planned, not started | sample, design |
| U2 | One admin at a time | `AuthoritySlot::Held` carries one `Holder`. sample `the_nominee_accepts_and_the_old_admin_is_out` asserts the old admin loses access the moment the new one has it | design, test |
| U3 | An end-to-end usage example in the docs | README integration steps and transaction shapes, `sample`, [`sample/RUN.md`](../sample/RUN.md) with the runner | doc, sample |

## Hard requirements, Performance

| # | Requirement | Satisfied by | Kind |
| --- | --- | --- | --- |
| P1 | Document the overhead of the check on a gated instruction | [`docs/overhead.md`](overhead.md), cycle counts per hop and per operation from `sample/methods/tests/overhead.rs`, and the transaction shape in the README | doc, measurement |

## Hard requirements, Supportability

| # | Requirement | Satisfied by | Kind |
| --- | --- | --- | --- |
| S1 | CI green on the default branch | `.github/workflows/ci.yml` (fmt, clippy, tests, consumer check), `audit.yml`, `consumer-build.yml` (guests and the runner), in both repos | ci |
| S2 | Every hard requirement has a test | this matrix | doc |
| S3 | A README with dependency and integration steps | README, Adding as a dependency and Integration steps | doc |
| S4 | A sample program that imports the library | `sample`, `sample-minimal`, `ci/consumer-fixture` | sample |

## Soft requirement, Reliability

| # | Requirement | Satisfied by | Kind |
| --- | --- | --- | --- |
| R1 | The admin can only be set to a valid new signer, on-curve key or deployed PDA | On 0.4 the nominee installs itself by accepting through its own seat, so an account that cannot authorize a transaction never becomes admin: sample `an_unsigned_sender_is_refused_at_its_seat`, `only_the_nominee_accepts`, `a_pda_nominee_without_the_seed_is_refused`, engine `a_pda_nominee_with_the_wrong_seed_is_refused`, `a_nominee_reached_through_a_program_without_a_seed_is_refused`, `accept_without_a_nomination_is_refused`. A PDA is reachable only through its program, so its accept is the proof that it is deployed | test |

## Proposal scenarios

| Scenario | Satisfied by |
| --- | --- |
| Initialization | sample `init_through_the_seat_installs_the_sender_as_admin`, runner `init` |
| Re-initialization rejection | sample `a_replayed_init_is_refused_too`, `init_after_renounce_is_refused`, engine `install_refuses_a_held_slot` |
| Admin gated-call success | sample `the_admin_sets_the_admin_value`, minimal `the_deployer_initializes_anyone_sets_the_open_value_only_the_admin_the_other`, runner `admin setter` |
| Non-admin gated-call rejection | sample `a_signed_stranger_cannot_set_the_admin_value`, runner `stranger`, with `lez-authority/admin E103: the signing account is not the admin` in the wallet's simulation and in the sequencer log |
| Transfer success | sample `the_nominee_accepts_and_the_old_admin_is_out`, runner `accept` |
| Non-admin transfer rejection | sample `a_stranger_cannot_nominate`, `only_the_nominee_accepts`, engine `only_the_holder_nominates`, runner `nominee before accept` |
| Renounce success | sample `renounce_empties_the_slot_and_keeps_the_values`, runner `renounce` |
| Protected instructions rejected after renounce | sample `a_renounced_admin_refuses_every_change`, runner `after renounce`, with `E102: the admin is renounced` in the simulation and in the log |

## The 0.4 routing surface

Behaviour with no v0.2 counterpart, because LEZ 0.4 routes messages between actors.

| Behaviour | Satisfied by |
| --- | --- |
| The seat forwards only when the sender authorized, naming the program that reached it | sample `the_seat_forwards_naming_the_program_that_reached_it`, `an_unsigned_sender_is_refused_at_its_seat`, `a_program_without_the_seed_is_refused_at_the_seat` |
| The config accepts a forward only from an actor of the program, and only at the config | sample `a_forged_forward_sent_straight_to_the_config_is_refused`, `a_forward_sent_to_a_seat_is_refused` |
| Only a seat builds a hop: the guard refuses a handler's response that sends the config a library message, wherever the handler runs | engine `a_handler_cannot_forge_a_hop`, `a_handler_cannot_forge_a_hop_through_an_async_message`, `a_handler_at_the_config_cannot_send_it_a_library_message` |
| A message that needs the admin cannot skip the seat | sample `init_sent_straight_to_the_config_is_refused`, `an_admin_message_sent_straight_to_the_config_is_refused`, engine `require_refuses_a_message_that_skipped_the_seat`, `an_operation_sent_straight_to_the_config_is_refused` |
| The init gate: the deployer, or the genesis key, at top level | sample `init_by_another_account_is_refused`, `init_by_the_deployer_through_a_program_is_refused`, `with_a_genesis_key_the_deployer_is_refused`, engine `another_account_cannot_install`, `the_deployer_reached_through_a_program_cannot_install`, runner `stranger init` with `E117` in the simulation and in the log |
| An empty config accepts only `Init` | sample `before_init_even_an_open_message_is_refused`, `before_init_the_admin_value_is_locked`, engine `only_the_first_role_creates_the_config` |
| Open messages need no seat, and run where they are sent | sample `anyone_sets_the_open_value`, `an_open_message_sent_to_a_seat_runs_there`, engine `an_open_message_runs_where_it_is_sent`, runner `open setter` |
| The admin's `via` is part of the admin | sample `a_pda_admin_acts_through_its_program`, `a_pda_admin_cannot_act_at_top_level`, `a_change_entered_from_another_program_is_refused`, engine `a_pda_holder_acts_through_its_program_only` |
| A nomination can be cancelled and replaced | sample `a_nomination_can_be_cancelled_and_replaced` |
| The hub: the config reaches a target, which runs a `#[from_config]` message from the config alone | sample `the_admin_sets_a_target_value_through_the_hub`, `a_stranger_cannot_reach_a_target`, `a_hub_message_sent_straight_to_a_target_is_refused`, `a_hub_message_from_another_actor_of_the_program_is_refused`, `a_hub_message_at_the_config_is_refused`, `a_target_value_sent_through_a_seat_is_refused`, engine `the_config_reaches_another_actor_of_the_program`, `a_config_only_message_from_anyone_else_is_refused`, runner `target setter` |
| An admin-only message may call another program | sample `the_admin_s_message_may_call_another_program`, engine `a_vouched_message_may_call_another_program` |
| The program stays a plain LEZ program: the attribute and the derives write what a program could write by hand | lez-authority `a_handler_under_the_attribute_answers_as_the_hand_expansion_does`, `every_derived_message_declares_what_its_hand_written_twin_does`, `each_marked_field_is_its_role_s_slot` |
| A handler cannot change the slot | sample `a_handler_that_overwrites_the_slot_is_refused` |
| The codes are stable | engine `codes_are_stable` |

## Live runs

The runner's fourteen steps on a local dev sequencer, LEZ `v0.4.0-rc3`, `RISC0_DEV_MODE=1`, last run 2026-11-11: deploy, a stranger's `Init` refused, `Init`, the open setter, the admin setter, the target setter through the config, a stranger refused, nominate, the nominee refused before accepting, accept, the new admin's setter, the old admin refused, renounce, a call after renounce refused. Every read-back matched. The runner found each refusal's code in the wallet's simulation, then sent the call anyway, and the five refusals appeared in the sequencer log with their codes at the blocks that included them: E117 once, E103 three times, E102 once.

Not yet run live: a PDA admin, which needs a second program. Covered on host in both repos.

Every row above names at least one passing test. No known gaps.
