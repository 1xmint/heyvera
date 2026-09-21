# Working on Cortex

The repository's own [AGENTS.md](../AGENTS.md) covers how a change lands. This
file says what Cortex is for, so that work is checked against it before it is
proposed.

## What this is for

Cortex is an agentic coding service that software teams buy at the repository,
not at a chat box: an issue assigned to Cortex comes back as a pull request with
a verification receipt attached. Organisations pay for completed, verified work
in credits (a monthly allotment plus packs); a run that fails verification is
refunded. The web app is mission control for that loop, not the product.

Deciding documents (these win when the code and the intent disagree):
- `cortex/plan/SURFACE.md` — which surfaces exist, their order, and what is
  deliberately not built
- `cortex/plan/CREDITS.md` — what a credit is and how work is charged
- `cortex/plan/VERIFIER.md` — what a verified result means
- `cortex/plan/EXECUTION-STATE.md` — where we are and what comes next

Always true:
- Subscriptions are the whole business.
- No surface shows a number the credit ledger cannot back; the quoted price
  and the charged price are the same number.
- Code left over from the older bring-your-own-key chat product is removed in
  the frontend audit, not repaired or restyled.

Not doing:
- The standalone budget pages and their four routes (`/api/budget/settings`,
  `/usage`, `/warnings`, `/warnings/{id}/acknowledge`). Spend limits live in the
  Ledger and Admin panes described in `SURFACE.md`; a missing budget route is
  not a gap to fill.
- A first-party IDE plugin in 2026.
- The GitHub App's delivery loop before the verifier gives real verdicts.
- WebSockets in the frontend (server-sent events only).
