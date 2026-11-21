# Every program message is frozen unless exempt

A program that adopts the freeze role is frozen whole: every program message is freezable, and the program exempts the ones that run through a freeze. This was the default of v0.2, its auto mode, and it is the closest fit to the single annotation RFP-002 asks for. A forgotten exemption errs toward frozen. The exemption is shallow, as it was in v0.2: every freezable message asks for clearance wherever it runs and whoever sent it, the program's own actors included, so an exempt message cannot push a freezable one through a freeze. A message that only trusted senders send can be exempted to skip its clearance. The manual mode of v0.2, where only the messages a program named freezable were frozen, is not carried over, and a program that used it exempts its other messages instead. The price of the strict default is that every freezable message handled away from the config pays for a clearance, and its transaction names the config actor. A transfer that credits another actor of the program takes four executions where it took two, five with the check of the sender's marker (ADR-0003).

## Considered options

- **Only messages named freezable,** the manual mode of v0.2 and the default of its proposal. A message its program forgets to name runs through a freeze.
- **Both modes, chosen per program,** as v0.2 shipped them. Two behaviours to document and test for one requirement. Manual mode can come back if an integrator asks for it.
- **Clearing only the messages that enter the program,** from a client or another program, and letting that clearance cover the messages the program then sends itself. One execution less per such message, but an exempt message's handler could then run freezable messages at other actors during a freeze.
