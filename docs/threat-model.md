# Threat model — v0

> v0 is a hackathon prototype: a single self-operated signer, unaudited.
> **Not for production capital.**

## Trust assumptions

A consumer who verifies a Noeracle price trusts:

1. **The publisher set + Stellar consensus** — and nothing else. There is no
   bridge, sidechain, or foreign chain in the path. v0 has **one** publisher
   key; compromising it compromises the oracle. v1 moves to a 3-of-5
   threshold, so no single key is fatal.
2. **The attestation service for liveness** — if it is down, consumers
   cannot fetch fresh prices. It is stateless and restartable, and the
   on-chain `get_price` cache still serves the last value within its TTL.

## What the contract enforces

- **Publisher check** — only a registered publisher key is accepted.
- **Staleness** — a price signed more than 60 s ago is rejected.
- **Monotonic cache** — the stored price only moves forward; a replayed
  older round is a silent no-op and cannot drag the cache backward.

## Known v0 limitations

- **Single signer** — the load-bearing v0 risk; addressed by the v1 threshold.
- **Spot, last-trade prices — no TWAP.** A consumer that needs
  manipulation-resistant valuation (e.g. a lending market) must apply its own
  smoothing; do not use a single spot read as a liquidation trigger.
- A single admin key controls the publisher set, with no timelock.
- `get_price` is best-effort — see the [integration guide](integration.md).

## Reporting

Found an issue? Open one on the repository.
