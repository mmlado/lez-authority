# Roles compose through traits

A program may hold more than one role. The next one known is a freeze authority that answers to the admin: the admin installs it and may nominate for it, and its renounce leaves the slot for the admin to fill again. Each role is a type with a name, `Role`, and its rules, `Policy<C>`, and the program's config gives it a slot through `HasSlot<R>`. `Policy` is generic over the config, so a role's rules can read another role's slot, and the compiler refuses a config that lacks it. The program's messages declare which roles' holders they need through `Gated`, and a client's operation names its role. The guard takes the program's roles as a tuple, `(A,)` or `(A, B)`, and the first one's `Init` creates the config. Adding a role adds impls and changes no existing one, the cost [ADR-0002](0002-guard-around-a-plain-handler.md) named against the design it rejected. Derives write the impls from markers, `#[slot(R)]`, `#[only(R)]` and `#[from_config]`, and do nothing else, so composition does not depend on them. We kept the traits as the design and the derives as a convenience over them.

Not decided here: how a freeze gates ordinary messages. A defaulted method on `Policy` or on `Gated` can be added later without breaking this shape.

## Considered options

- **Derives as the design, traits hidden.** Shorter for the integrator, but a program that cannot or will not use a proc macro would have no way in, and the composition would then rest on what each derive emits. Rejected: the derives emit only trait impls a program could write itself.
- **One rewriting attribute per role crate,** as the v0.2 SPEL extensions had one macro each. Two of them must agree on who owns the dispatch, which in v0.2 took an extension mechanism upstream. Rejected: the one attribute that rewrites the handler takes every role ([ADR-0004](0004-the-guard-in-front-of-a-plain-handler.md)).
- **One role per guard, with a second guard nested in the handler.** The hop runs once per message, so a nested guard cannot route. Rejected.
- **Checks the handler calls, one trait per role crate on a shared context.** Built, then replaced by declared holders ([ADR-0004](0004-the-guard-in-front-of-a-plain-handler.md)): the handler holds no checks.
