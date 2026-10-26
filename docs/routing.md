# Routing, hop by hop

What the guard does at each actor for each kind of message, with the refusal that stops a wrong step. Notation: `P` is the program, whose id is its loader header account. `C` is the config account, `for_public_pda(P, seed)`. `(X, P)` is the actor of `P` on account `X`. Codes are those of the [refusal table](../README.md#refusals).

Every client transaction names its root actor, every actor it reaches, its signers, and a fee declaration whose payer signs. All hops here are sync, so a refusal anywhere drops every effect of the transaction and still charges the fee. The runtime fills `from` and `is_authorized` at each hop, and the guard reads identity from nothing else.

## Init

```text
client   root (D, P)    actors [(D, P), (C, P)]    signers {D}    message Op::Init
                        D is the program's own account P, or the genesis key

hop 1    (D, P) seat    not a forward                              else E121
                        is_authorized                              else E104
                        forward to (C, P): Forwarded { via: None, inner }

hop 2    (C, P) config  not a hub message                          else E123
                        from an actor of P                         else E119
                        caller = { account: from.account_id, via: None }
                        the account is empty, so only Init is accepted   else E101
                        caller.account == P  (Deployer gate)       else E117
                        or caller.account == key  (Genesis gate)   else E115
                        caller.via is None                         else E117 / E115
                        slot = Held { holder: { D, via: None }, nominee: None }
                        the config is written: Config::default() with that slot
```

`Init` on a non-empty config reaches the install with a decoded slot: a held slot is E118, a renounced one E102. The first write to `C` needs the PDA's evidence in the transaction, `PublicAccountEvidence::Pda { program, seed }`, which the LEZ wallet supplies from a `PublicPda` mention.

## An open message

```text
client   root (C, P)    actors [(C, P)]    signers {}    message an open variant

hop 1    (C, P) config  not a hub message, not a forward
                        caller = None
                        state present and decodes                  else E101 / E109
                        handler(ctx, config, message)              a require here is E122
                        slot unchanged                             else E126
                        the config is written
```

An open message may also travel through a seat. The handler then sees a caller, and a `require` would pass for the holder, but nothing forces the hop.

## A gated message

```text
client   root (A, P)    actors [(A, P), (C, P)]    signers {A}    message a gated variant

hop 1    (A, P) seat    not a forward                              else E121
                        is_authorized                              else E104
                        forward to (C, P): Forwarded { via: None, inner }

hop 2    (C, P) config  from an actor of P                         else E119
                        caller = { A, via: None }
                        inner decodes, and is not itself a forward or a hub message   else E108
                        state present and decodes                  else E101 / E109
                        handler(ctx, config, message)
                          ctx.require(&slot):
                            slot Unset                             E131
                            slot Renounced                         E102
                            holder.account != A                    E103
                            holder.via != caller.via               E120
                        slot unchanged                             else E126
                        the config is written
```

## A gated message with a target, the hub

```text
client   root (A, P)    actors [(A, P), (C, P), (T, P)]    signers {A}

hop 1    (A, P) seat    as above
hop 2    (C, P) config  as above, and in the handler after require:
                          ctx.send(T, &message)                    needs require first, else E132
                        the config is written, then Hub { inner } is sent sync to (T, P)

hop 3    (T, P) target  the message is a hub message
                        from == Some((C, P)), by exact actor       else E123
                        the program has a target handler           else E123
                        inner decodes                              else E108
                        the target's state decodes, default when empty   else E133
                        target_handler(state, message), the state is written
```

"From an actor of P" is not enough at hop 3, because any actor of `P` passes it: a stranger's seat could forward a hub message to a target. The target compares the whole actor. A hub message that reaches the config, or a seat, is E123 as well. Every actor reached must be in the transaction's actor list, so a client lists the target too.

## Nominate and cancel

```text
client   root (A, P)    actors [(A, P), (C, P)]    signers {A}    message Op::Nominate { next: N }

hop 1    (A, P) seat    as for a gated message
hop 2    (C, P) config  require(slot) for A                        else E131 / E102 / E103 / E120
                        nominee = Some(N), or None when N is the all-zero id
                        the config is written
```

Nothing is checked about `N`. It is a bare id, it need not sign or be present, and a mistyped one can never accept. A later nomination replaces an earlier one.

## Accept, by a key

```text
client   root (N, P)    actors [(N, P), (C, P)]    signers {N}    message Op::Accept { pda_seed: None }

hop 1    (N, P) seat    is_authorized                              else E104
hop 2    (C, P) config  caller = { N, via: None }
                        slot Unset                                 E131
                        slot Renounced                             E102
                        no nominee                                 E113
                        nominee != N                               E114
                        via None and no seed                       ok, anything else E107
                        slot = Held { holder: { N, via: None }, nominee: None }
```

## Accept, by a PDA

`N = for_public_pda(G, s)`: a PDA of program `G` with seed `s`. The owning program's actor `(R, G)` runs first and sends the accept to the nominee's seat with the seed as a grant.

```text
client   root (R, G) as G requires    actors [(R, G), (N, P), (C, P)]    signers as G requires

hop 0    (R, G)         G's own checks, then send sync to (N, P):
                        Op::Accept { pda_seed: Some(s) }, pda_seeds {s}

hop 1    (N, P) seat    is_authorized, by the grant                else E104
                        forward: Forwarded { via: Some(G), inner }

hop 2    (C, P) config  caller = { N, via: Some(G) }
                        nominee == N                               else E113 / E114
                        for_public_pda(G, s) == N                  else E107
                        slot = Held { holder: { N, via: Some(G) }, nominee: None }
```

The seed proof keeps a program that merely inherited the grant from accepting on the PDA's behalf: `via` is the program that reached the seat, and the seed must derive the nominee under that program. A key nominee reached through a program has `via: Some(..)` and no seed, which is E107 too.

## Renounce

```text
client   root (A, P)    actors [(A, P), (C, P)]    signers {A}    message Op::Renounce

hop 1    (A, P) seat    as for a gated message
hop 2    (C, P) config  require(slot) for A                        else E131 / E102 / E103 / E120
                        slot = Renounced, the nominee is gone
```

Afterwards every `require` is E102, and so are `Accept`, `Nominate`, `Renounce` and `Init`.

## A PDA holder acts

```text
client   root (R, G) as G requires    actors [(R, G), (N, P), (C, P)] and any targets

hop 0    (R, G)         G's own checks, then send sync to (N, P): the gated message, pda_seeds {s}
hop 1    (N, P) seat    is_authorized, by the grant                else E104
                        forward: Forwarded { via: Some(G), inner }
hop 2    (C, P) config  require: holder == { N, via: Some(G) }     else E103 / E120
                        the rest as for a gated message
```

A PDA holds the role only after an accept, because `Init` installs a top-level account. The holder's `via` is fixed at accept, so `G` and only `G` can act for `N`, and `N` could not act at top level even if it had a key (E120). Grants travel on sync messages only, so `G` must send the gated message by a sync call, not an async one.

Verified on host in both repos. Not yet run on a sequencer: it needs a second program to play `G`.

## The client side

| Field | Rule |
| --- | --- |
| Root actor | the sender's seat `(sender, P)` for anything that needs the holder, the config `(C, P)` for an open message, the owning program's actor for a PDA holder |
| Actors | every `(account, program)` pair the transaction reaches, root included: the seat, the config, each target |
| Signers | the sender of a gated message or an operation. An open message needs no signer besides the fee payer |
| Evidence | the config account's PDA evidence on its first write. A target that is a key account the wallet holds was admitted as a non-signing public account |
| Fee | a fee declaration whose payer signs and has a nonce. The wallet's default `gas_limit` of 2,000,000 cycles covered three hops, see the admin repo's overhead numbers |

The LEZ wallet library builds all of this from account mentions. The admin repo's runner is the reference client.
