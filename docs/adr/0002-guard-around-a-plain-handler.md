# The guard wraps the program's own handler

The library does not own a program's message enum or its dispatch. A program writes its own enum, hosts two library variants in it, and writes a plain handler that receives a context and calls `require` in the arms that need the holder. The guard forwards at seats, unwraps the hop at the config, runs the hosted operations, calls the handler, and refuses a result that changed the slot by any means other than the context. A first version owned the wire format and verified the holder before calling into the program through a trait, which made the admin structural: the program was an `impl` of the library's trait, clients spoke the library's envelope, and removing the admin meant rewriting the program. We traded the library-side proof for a program that stays a LEZ program, with the slot check keeping the holder out of the handler's reach.

## Considered options

- **Library-owned envelope and trait dispatch.** Built and rejected. Safer by one forgotten call, at the cost of dictating the program's shape and of trait changes for every later role.
