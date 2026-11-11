# Routing, hop by hop

What the guard does at each actor for each kind of message, with the refusal that stops a wrong step. Notation: `P` is the program, whose id is its loader header account. `C` is the config account, `for_public_pda(P, seed)`. `(X, P)` is the actor of `P` on account `X`. Codes are those of the [refusal table](../README.md#refusals).

Every role routes the same way. Its operations travel as library messages, `Wire::op::<R>(op)`, which start with the byte 255 and name the role, and its `Policy` decides who may install, nominate and renounce. The checks below are the defaults, which a role that answers to nobody keeps, as the admin does. The program's own messages keep the program's encoding.

Every client transaction names its root actor, every actor it reaches, its signers, and a fee declaration whose payer signs. All hops here are sync, so a refusal anywhere drops every effect of the transaction and still charges the fee. The runtime fills `from` and `is_authorized` at each hop, and the guard reads identity from nothing else.

## Init

```text
client   root (D, P)    actors [(D, P), (C, P)]    signers {D}    message Wire::op::<R>(Op::Init)
                        D is the program's own account P, or the genesis key

hop 1    (D, P) seat    an operation is forwarded, never run here
                        is_authorized                              else E104
                        forward to (C, P): Forwarded { via: None, inner: the message as it arrived }

hop 2    (C, P) config  from an actor of P                         else E119
                        caller = { account: from.account_id, via: None }
                        the operation names a role of the program  else E108
                        the account is empty, so only the root role's Init is accepted   else E101
                        caller.account == P  (Deployer gate)       else E117
                        or caller.account == key  (Genesis gate)   else E115
                        caller.via is None                         else E117 / E115
                        slot = Held { holder: { D, via: None }, nominee: None }
                        the config is written: the config's default with that slot
```

`Init` on a non-empty config reaches the install with a decoded slot: a held slot is E118, a renounced one E102. An operation sent straight to the config is E122. The first write to `C` needs the PDA's evidence in the transaction, `PublicAccountEvidence::Pda { program, seed }`, which the LEZ wallet supplies from a `PublicPda` mention.

## An open message

```text
client   root (C, P)    actors [(C, P)]    signers {}    message an unmarked variant

hop 1    (C, P) config  caller = None
                        state present and decodes                  else E101 / E109
                        handler(input, message)
                        the handler's write decodes, every slot unchanged   else E109 / E126
                        no message the handler sends is a library message to (C, P)   else E135
```

An unmarked message sent to any other actor runs there over that actor's own state, as in a program without the guard, and nobody need sign. So wherever the handler runs, the guard refuses a response that sends `(C, P)` a library message (E135): the config would take it for a hop from that actor, a holder's seat included.

## A message that needs a holder

```text
client   root (A, P)    actors [(A, P), (C, P)]    signers {A}    message a variant marked #[only(R)]

hop 1    (A, P) seat    is_authorized                              else E104
                        forward to (C, P): Forwarded { via: None, inner }

hop 2    (C, P) config  from an actor of P                         else E119
                        caller = { A, via: None }
                        inner decodes, and is not itself a forward  else E108
                        state present and decodes                  else E101 / E109
                        for each role R the message needs, R's slot:
                            Unset                                  E131
                            Renounced                              E102
                            holder.account != A                    E103
                            holder.via != caller.via               E120
                        handler(input, message)
                        the handler's write decodes, every slot unchanged   else E109 / E126
                        no message the handler sends is a library message to (C, P)   else E135
```

Sent straight to the config, the message is E122. The handler sees `input.from` as `(A, P)` and `input.message` as the hop.

## A message the config sends on, to a target

```text
client   root (A, P)    actors [(A, P), (C, P), (T, P)]    signers {A}

hop 1    (A, P) seat    as above
hop 2    (C, P) config  as above, and the handler's response sends a #[from_config] message to (T, P)

hop 3    (T, P) target  from == Some((C, P)), by exact actor       else E123
                        handler(input, message) over the target's own state
                        no message the handler sends is a library message to (C, P)   else E135
```

"From an actor of P" is not enough at hop 3, because any actor of `P` passes it: a stranger's seat could send the same message to a target. The target compares the whole actor. A `#[from_config]` message from a client, from another actor, or at the config itself, is E123. Every actor reached must be in the transaction's actor list, so a client lists the target too. An unmarked message the config sends runs at the target as well, but so would the same message from anyone, which is what the mark is for.

## Nominate and cancel

```text
client   root (A, P)    actors [(A, P), (C, P)]    signers {A}    message Wire::op::<R>(Op::Nominate { next: N })

hop 1    (A, P) seat    as for a message that needs a holder
hop 2    (C, P) config  may_nominate, by default the holder is A    else E131 / E102 / E103 / E120
                        nominee = Some(N), or None when N is the all-zero id
                        the config is written
```

Nothing is checked about `N`. It is a bare id, it need not sign or be present, and a mistyped one can never accept. A later nomination replaces an earlier one.

## Accept, by a key

```text
client   root (N, P)    actors [(N, P), (C, P)]    signers {N}    message Wire::op::<R>(Op::Accept { pda_seed: None })

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
                        Wire::op::<R>(Op::Accept { pda_seed: Some(s) }), pda_seeds {s}

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
client   root (A, P)    actors [(A, P), (C, P)]    signers {A}    message Wire::op::<R>(Op::Renounce)

hop 1    (A, P) seat    as for a message that needs a holder
hop 2    (C, P) config  may_renounce, by default the holder is A    else E131 / E102 / E103 / E120
                        slot = Renounced by default, the nominee is gone
```

Afterwards every message that needs the role is E102, and so are `Accept`, `Nominate`, `Renounce` and `Init`.

## A PDA holder acts

```text
client   root (R, G) as G requires    actors [(R, G), (N, P), (C, P)] and any targets

hop 0    (R, G)         G's own checks, then send sync to (N, P): the message, pda_seeds {s}
hop 1    (N, P) seat    is_authorized, by the grant                else E104
                        forward: Forwarded { via: Some(G), inner }
hop 2    (C, P) config  the declared holder: { N, via: Some(G) }   else E103 / E120
                        the rest as for a message that needs a holder
```

A PDA holds the role only after an accept, because `Init` installs a top-level account. The holder's `via` is fixed at accept, so `G` and only `G` can act for `N`, and `N` could not act at top level even if it had a key (E120). Grants travel on sync messages only, so `G` must send the message by a sync call, not an async one. The grant comes with the call, so the handler, and every program it calls, holds `N`'s authority for that call. A program that received the grant cannot act for `N` here: it reaches `N`'s seat with itself as via (E120). Make `N` an account that holds no assets and no other role.

Verified on host in both repos. Not yet run on a sequencer: it needs a second program to play `G`.

## The client side

| Field | Rule |
| --- | --- |
| Root actor | the sender's seat `(sender, P)` for an operation or a message that needs a holder, the owning program's actor for a PDA holder, and for any other message the actor it is meant for, the config `(C, P)` for config state |
| Message | an operation as `Wire::op::<R>(op)`, any other message encoded as the program encodes it |
| Actors | every `(account, program)` pair the transaction reaches, root included: the seat, the config, each target |
| Signers | the sender of an operation or of a message that needs a holder. An open message needs no signer besides the fee payer |
| Evidence | the config account's PDA evidence on its first write. A target that is a key account the wallet holds was admitted as a non-signing public account |
| Fee | a fee declaration whose payer signs and has a nonce. The wallet runs the call first and declares the cycles it measured plus 25 percent, see the admin repo's overhead numbers. It does not send a call that reverts unless the client allows it, and such a call pays its whole gas limit |

The LEZ wallet library builds all of this from account mentions. The admin repo's runner is the reference client.
