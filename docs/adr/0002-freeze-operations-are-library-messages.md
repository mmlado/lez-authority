# The freeze operations are library messages

The freeze authority's operations, freezing and releasing the program or one account, change state the program must not touch. They travel as library messages behind the byte 255, as the role operations do, and the guard runs them at the config under the freeze role's rules. The handler never sees them, the guard refuses a handler that changed the freeze slot, and a freeze never refuses them, so a release always gets through. The engine gains operations that a role defines for itself, appended to its library messages so that existing encodings stay. The integrator writes no code for them. SPEL's IDL v2 does not describe library messages yet, so until one fix covers both roles, a client builds them with the library, as it builds the admin's.

## Considered options

- **Variants of the program's own enum,** each needing the freeze authority and handled with helpers from the freeze crate. No engine change, and SPEL's IDL generator sees them with the rest of the enum. But the guard cannot tell those arms from the others, so any arm could change the freeze slot, and every program grows four variants and four arms, each marked exempt so that a release runs while frozen.
