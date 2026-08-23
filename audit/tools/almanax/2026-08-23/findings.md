# Almanax raw findings — noeracle @ b30f1d0, scope oracle_v0/ (2026-08-23)

## CRITICAL — Contract Lifecycle & State: Anyone can front-run init and seize admin
`oracle_v0/src/lib.rs` L101-L113. init stores the provided admin after only
checking admin.require_auth(); any account can call init first, pass itself as
admin, and permanently become the contract admin. Especially realistic given
scripts/deploy_oracle_v0.sh deploys and then invokes init as a separate
transaction, leaving a front-run window. Recommendation: deploy and initialize
atomically (constructor / factory), or bind admin to the deploy flow.

## MEDIUM — Signature & Authentication: Signed price messages are replayable across contracts
`oracle_v0/src/lib.rs` L664-L677 (build_msg). The 40-byte message carries no
domain separation (contract ID, network, protocol prefix); a signature for one
deployment verifies on another sharing the publisher key, within the staleness
window. Recommendation: add a domain prefix + env.current_contract_address()
(and network identifier) to build_msg and the off-chain signer.

## MEDIUM — Input and Parameter Validation: Future timestamps bypass staleness checks
`oracle_v0/src/lib.rs` L198-L202. now.saturating_sub(timestamp) returns 0 for
future timestamps, so a future-dated attestation is accepted as fresh and the
stored entry can appear fresh longer than intended. Recommendation: enforce
timestamp <= now + allowed_clock_skew.

## LOW — Arithmetic and Financial Logic: Twap can panic if stored prices overflow
`oracle_v0/src/lib.rs` L448-L457. twap sums ring prices with
checked_add().unwrap(); extreme publisher-signed values could overflow the sum
and trap, breaking the view for that asset. Recommendation: bound prices at
update time and/or return None on overflow.
