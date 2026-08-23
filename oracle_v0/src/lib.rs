#![no_std]

//! Noeracle oracle_v0 — Soroban pull-oracle contract.
//!
//! Production pull-mode entrypoints (both hardened: registered-publisher
//! check, staleness bound, monotonic round_id):
//!
//! - `update_batch_ed25519_args` — writes temporary storage, read via
//!   `get_price`. For consumers that verify-then-read inside one transaction.
//! - `update_batch_ed25519_persistent` — writes persistent storage, read via
//!   `get_price_pers`. For consumers that read between keeper heartbeats
//!   (e.g. a perp market reading through a SEP-40 shim).
//! - `update_quorum_ed25519_persistent` — M-of-N variant of the persistent
//!   path: at least `quorum()` distinct registered publishers co-sign one
//!   synchronized round and the per-asset MEDIAN of their prices is stored.
//!
//! Every persistent write also appends to a per-asset history ring (the
//! `RING_CAP` most recent monotonic rounds), served by the `prices` and
//! `twap` views.
//!
//! Initialization happens in `__constructor`, atomically inside the deploy
//! transaction — there is no post-deploy init window to front-run.
//!
//! The remaining `update_*` entrypoints exist only to measure the Soroban
//! host cost of alternative signature schemes. They perform NO publisher or
//! freshness checks and are compiled only with the `bench` feature — never
//! deploy a bench build to a network where the oracle is consumed.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Bytes, BytesN, Env, Vec,
};

#[cfg(feature = "bench")]
use soroban_sdk::crypto::bls12_381::{Bls12381G1Affine, Bls12381G2Affine};

/// Errors returned by the production pull-mode entrypoint and admin calls.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    BatchLengthMismatch = 3,
    UnknownPublisher = 4,
    StalePrice = 5,
    InvalidQuorum = 6,
    DuplicatePublisher = 7,
    QuorumNotMet = 8,
    InvalidPrice = 9,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Publishers,
    PriceTemp(BytesN<8>),
    PricePers(BytesN<8>),
    Quorum,
    PriceRing(BytesN<8>),
}

#[contracttype]
#[derive(Clone)]
pub struct PriceEntry {
    pub price: i128,
    pub timestamp: u64,
    pub round_id: u64,
}

/// One publisher's signed contribution to a quorum submission: `prices[i]`
/// and `sigs[i]` align with the shared `assets` vector of the call. Rounds
/// are transient call data — only the per-asset median is ever stored.
#[contracttype]
#[derive(Clone)]
pub struct PublisherRound {
    pub pubkey: BytesN<32>,
    pub prices: Vec<i128>,
    pub sigs: Vec<BytesN<64>>,
}

// Reject signed prices whose timestamp is older than this many seconds.
const STALENESS_SECS: u64 = 60;
// Reject signed prices stamped further in the FUTURE than this (ALX-3:
// `saturating_sub` alone reads a far-future timestamp as age 0). A small
// allowance is required because the keeper stamps wall-clock signing time
// and the ledger close time it is compared against can trail it by seconds.
const FUTURE_SKEW_SECS: u64 = 30;
// Bounds on any accepted price (1e7-scaled USD). Non-positive prices are
// nonsense for a spot oracle, and the cap keeps a RING_CAP-entry sum
// arithmetically unable to overflow i128 (ALX-4): 32 * 1e30 << i128::MAX.
const MAX_PRICE: i128 = 1_000_000_000_000_000_000_000_000_000_000;

// Conservative TTLs: keep entries alive a bit longer than minimum so a
// keeper outage of a few minutes doesn't immediately archive everything.
const TEMP_THRESHOLD: u32 = 360;
const TEMP_EXTEND: u32 = 720;
const PERS_THRESHOLD: u32 = 60_480;
const PERS_EXTEND: u32 = 120_960;

// Per-asset history ring capacity: the most recent RING_CAP monotonic rounds
// are kept in persistent storage for the prices()/twap() views.
const RING_CAP: u32 = 32;

#[contract]
pub struct OracleV0;

#[contractimpl]
impl OracleV0 {
    /// Deploy-time setup: record the admin Address and the initial publisher
    /// Ed25519 key set. Runs atomically inside the deploy transaction, so a
    /// fresh instance can never be claimed by a third party (ALX-1: the
    /// former separate `init` entrypoint left a front-run window between
    /// deploy and initialization; it has been removed outright). The admin
    /// must still authorize, so a deployer cannot install someone ELSE as
    /// admin without their signature.
    pub fn __constructor(env: Env, admin: Address, publishers: Vec<BytesN<32>>) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Publishers, &publishers);
    }

    /// Replace the publisher Ed25519 key set. Admin-authenticated.
    pub fn set_publishers(env: Env, publishers: Vec<BytesN<32>>) -> Result<(), Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DataKey::Publishers, &publishers);
        Ok(())
    }

    /// Set the minimum number of distinct registered publishers whose signed
    /// rounds `update_quorum_ed25519_persistent` must carry. Admin-
    /// authenticated. Must satisfy 1 <= quorum <= publishers.len() at set
    /// time; an unset quorum behaves as 1, so existing single-publisher
    /// deployments are unchanged until the admin raises the bar. (A later
    /// `set_publishers` may shrink the key set below this threshold — the
    /// quorum path then cannot meet quorum until the admin re-syncs.)
    pub fn set_quorum(env: Env, quorum: u32) -> Result<(), Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        let publishers: Vec<BytesN<32>> =
            env.storage().instance().get(&DataKey::Publishers).unwrap();
        if quorum < 1 || quorum > publishers.len() {
            return Err(Error::InvalidQuorum);
        }
        env.storage().instance().set(&DataKey::Quorum, &quorum);
        Ok(())
    }

    /// Current quorum threshold (1 when never configured).
    pub fn get_quorum(env: Env) -> u32 {
        quorum(&env)
    }

    /// Swap the running contract's WASM for an already-uploaded blob.
    /// Admin-authenticated (same gate as `set_publishers`). Shipping this is
    /// deliberately the LAST forced redeploy: every future contract change
    /// can land through `upgrade` instead of a fresh deploy + init ceremony +
    /// consumer repoint (shim / router / keeper env).
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    // -------- Ed25519 batch (single publisher, multiple assets, one tx) --------
    //
    // One transaction lands an entire price round. Each asset is signed
    // independently over the same 40-byte format as `update_ed25519_args`
    // (asset || price || timestamp || round_id), so off-chain signing code
    // is identical across the single-asset and batched paths.
    pub fn update_batch_ed25519_args(
        env: Env,
        assets: Vec<BytesN<8>>,
        prices: Vec<i128>,
        timestamp: u64,
        round_id: u64,
        pubkey: BytesN<32>,
        sigs: Vec<BytesN<64>>,
    ) -> Result<(), Error> {
        let n = assets.len();
        if prices.len() != n || sigs.len() != n {
            return Err(Error::BatchLengthMismatch);
        }

        // The signing key must be a registered publisher.
        if !env.storage().instance().has(&DataKey::Publishers) {
            return Err(Error::NotInitialized);
        }
        let publishers: Vec<BytesN<32>> =
            env.storage().instance().get(&DataKey::Publishers).unwrap();
        if !is_publisher(&publishers, &pubkey) {
            return Err(Error::UnknownPublisher);
        }

        // Reject rounds signed more than STALENESS_SECS ago.
        let now = env.ledger().timestamp();
        if now.saturating_sub(timestamp) > STALENESS_SECS
            || timestamp > now.saturating_add(FUTURE_SKEW_SECS)
        {
            return Err(Error::StalePrice);
        }

        // Bound every price before any signature work (ALX-4).
        for i in 0..n {
            let p = prices.get_unchecked(i);
            if p <= 0 || p > MAX_PRICE {
                return Err(Error::InvalidPrice);
            }
        }
        let crypto = env.crypto();
        for i in 0..n {
            let asset = assets.get_unchecked(i);
            let price = prices.get_unchecked(i);
            let msg = build_msg(&env, &asset, price, timestamp, round_id);
            crypto.ed25519_verify(&pubkey, &msg, &sigs.get_unchecked(i));

            // The per-asset cache is kept monotonic: advance it only when
            // this round is newer than what is stored. A lagging or replayed
            // round is a silent no-op — it must never fail the consumer's
            // transaction, because transaction landing order does not track
            // round order across independent pull consumers. Staleness is
            // already bounded by the STALENESS_SECS check above.
            let prev: Option<PriceEntry> = env
                .storage()
                .temporary()
                .get(&DataKey::PriceTemp(asset.clone()));
            let is_newer = match prev {
                Some(prev) => round_id > prev.round_id,
                None => true,
            };
            if is_newer {
                write_temp(&env, asset, PriceEntry { price, timestamp, round_id });
            }
        }
        Ok(())
    }

    // -------- Ed25519 batch, persistent storage (production) --------
    //
    // Same hardened checks as `update_batch_ed25519_args`, but the round is
    // written to persistent storage and read back via `get_price_pers`, so a
    // consumer that reads between keeper heartbeats (rather than bundling the
    // update into its own transaction) keeps working across temporary-entry
    // expiry. Each asset is signed independently over the same 40-byte format.
    pub fn update_batch_ed25519_persistent(
        env: Env,
        assets: Vec<BytesN<8>>,
        prices: Vec<i128>,
        timestamp: u64,
        round_id: u64,
        pubkey: BytesN<32>,
        sigs: Vec<BytesN<64>>,
    ) -> Result<(), Error> {
        let n = assets.len();
        if prices.len() != n || sigs.len() != n {
            return Err(Error::BatchLengthMismatch);
        }

        // This single-publisher path shares PricePers storage with the
        // quorum path. Once the admin raises the quorum above 1 it MUST
        // close — otherwise one compromised registered key could still set
        // the stored price outright, bypassing the M-of-N median entirely.
        // At the default quorum of 1 the two paths are equivalent, so
        // existing single-publisher deployments are unchanged.
        if quorum(&env) > 1 {
            return Err(Error::QuorumNotMet);
        }

        // The signing key must be a registered publisher.
        if !env.storage().instance().has(&DataKey::Publishers) {
            return Err(Error::NotInitialized);
        }
        let publishers: Vec<BytesN<32>> =
            env.storage().instance().get(&DataKey::Publishers).unwrap();
        if !is_publisher(&publishers, &pubkey) {
            return Err(Error::UnknownPublisher);
        }

        // Reject rounds signed more than STALENESS_SECS ago.
        let now = env.ledger().timestamp();
        if now.saturating_sub(timestamp) > STALENESS_SECS
            || timestamp > now.saturating_add(FUTURE_SKEW_SECS)
        {
            return Err(Error::StalePrice);
        }

        // Bound every price before any signature work (ALX-4).
        for i in 0..n {
            let p = prices.get_unchecked(i);
            if p <= 0 || p > MAX_PRICE {
                return Err(Error::InvalidPrice);
            }
        }
        let crypto = env.crypto();
        for i in 0..n {
            let asset = assets.get_unchecked(i);
            let price = prices.get_unchecked(i);
            let msg = build_msg(&env, &asset, price, timestamp, round_id);
            crypto.ed25519_verify(&pubkey, &msg, &sigs.get_unchecked(i));

            // Monotonic advance with silent no-op on lagging or replayed
            // rounds — same rationale as the temporary-storage batch path: a
            // consumer's transaction must never fail on cross-consumer
            // ordering. Staleness is already bounded above.
            let prev: Option<PriceEntry> = env
                .storage()
                .persistent()
                .get(&DataKey::PricePers(asset.clone()));
            let is_newer = match prev {
                Some(prev) => round_id > prev.round_id,
                None => true,
            };
            if is_newer {
                write_pers(&env, asset, PriceEntry { price, timestamp, round_id });
            }
        }
        Ok(())
    }

    // -------- Ed25519 quorum batch, persistent storage (production) --------
    //
    // M-of-N hardening of the persistent path: a relayer submits ONE
    // synchronized round — every publisher signs the same (assets, timestamp,
    // round_id) — carrying at least `quorum()` distinct registered publisher
    // rounds, and the per-asset MEDIAN of the submitted prices is stored. A
    // single compromised publisher key can then move the stored price by at
    // most one rank instead of setting it outright.
    //
    // Signatures are verified with `build_msg` VERBATIM — the same 40-byte
    // asset || price || timestamp || round_id encoding the single-publisher
    // paths use and the JS golden-vector conformance tests pin — so off-chain
    // signing code is identical across all three paths; only the price values
    // differ per publisher.
    pub fn update_quorum_ed25519_persistent(
        env: Env,
        assets: Vec<BytesN<8>>,
        timestamp: u64,
        round_id: u64,
        rounds: Vec<PublisherRound>,
    ) -> Result<(), Error> {
        let n = assets.len();
        // Every publisher round must align with the shared asset vector.
        for round in rounds.iter() {
            if round.prices.len() != n || round.sigs.len() != n {
                return Err(Error::BatchLengthMismatch);
            }
        }

        // Reject rounds signed more than STALENESS_SECS ago.
        let now = env.ledger().timestamp();
        if now.saturating_sub(timestamp) > STALENESS_SECS
            || timestamp > now.saturating_add(FUTURE_SKEW_SECS)
        {
            return Err(Error::StalePrice);
        }

        // Bound every submitted price before any signature work (ALX-4).
        for round in rounds.iter() {
            for i in 0..n {
                let p = round.prices.get_unchecked(i);
                if p <= 0 || p > MAX_PRICE {
                    return Err(Error::InvalidPrice);
                }
            }
        }
        // Every signing key must be a registered publisher, each at most once.
        if !env.storage().instance().has(&DataKey::Publishers) {
            return Err(Error::NotInitialized);
        }
        let publishers: Vec<BytesN<32>> =
            env.storage().instance().get(&DataKey::Publishers).unwrap();
        let m = rounds.len();
        for i in 0..m {
            let round = rounds.get_unchecked(i);
            if !is_publisher(&publishers, &round.pubkey) {
                return Err(Error::UnknownPublisher);
            }
            for j in (i + 1)..m {
                if rounds.get_unchecked(j).pubkey == round.pubkey {
                    return Err(Error::DuplicatePublisher);
                }
            }
        }

        // The submission must carry at least the configured quorum.
        if m < quorum(&env) {
            return Err(Error::QuorumNotMet);
        }

        // Verify EVERY signature (per publisher per asset). One bad signature
        // reverts the whole submission — same semantics as the single-
        // publisher paths, where any failed verify aborts the transaction.
        let crypto = env.crypto();
        for round in rounds.iter() {
            for i in 0..n {
                let asset = assets.get_unchecked(i);
                let msg = build_msg(
                    &env,
                    &asset,
                    round.prices.get_unchecked(i),
                    timestamp,
                    round_id,
                );
                crypto.ed25519_verify(&round.pubkey, &msg, &round.sigs.get_unchecked(i));
            }
        }

        // Per asset: store the median of the M publisher prices behind the
        // same monotonic round_id check (newer-only, silent no-op on lag) as
        // the single-publisher persistent path. write_pers also appends the
        // median entry to the asset's history ring.
        for i in 0..n {
            let asset = assets.get_unchecked(i);
            let mut px: Vec<i128> = Vec::new(&env);
            for round in rounds.iter() {
                px.push_back(round.prices.get_unchecked(i));
            }
            let price = median(&env, &px);

            let prev: Option<PriceEntry> = env
                .storage()
                .persistent()
                .get(&DataKey::PricePers(asset.clone()));
            let is_newer = match prev {
                Some(prev) => round_id > prev.round_id,
                None => true,
            };
            if is_newer {
                write_pers(&env, asset, PriceEntry { price, timestamp, round_id });
            }
        }
        Ok(())
    }

    // -------- Reads --------
    pub fn get_price(env: Env, asset: BytesN<8>) -> Option<PriceEntry> {
        env.storage().temporary().get(&DataKey::PriceTemp(asset))
    }

    pub fn get_price_pers(env: Env, asset: BytesN<8>) -> Option<PriceEntry> {
        env.storage().persistent().get(&DataKey::PricePers(asset))
    }

    /// The last `records` rounds from the asset's history ring, latest-first.
    /// Returns up to min(records, stored) entries; `None` when the asset has
    /// no history at all (a `records` of 0 with history is an empty page).
    pub fn prices(env: Env, asset: BytesN<8>, records: u32) -> Option<Vec<PriceEntry>> {
        let ring: Vec<PriceEntry> =
            env.storage().persistent().get(&DataKey::PriceRing(asset))?;
        let len = ring.len();
        let take = if records < len { records } else { len };
        let mut out = Vec::new(&env);
        for i in 0..take {
            out.push_back(ring.get_unchecked(len - 1 - i));
        }
        Some(out)
    }

    /// Arithmetic mean of the last min(records, stored) ring prices. This is
    /// a simple mean over stored ROUNDS, not a time-weighted average —
    /// adequate for a consumer's median-of-last-k / smoothing eligibility
    /// checks, not for interval-exact TWAP accounting. `None` when the asset
    /// has no history or `records` is 0.
    pub fn twap(env: Env, asset: BytesN<8>, records: u32) -> Option<i128> {
        if records == 0 {
            return None;
        }
        let ring: Vec<PriceEntry> =
            env.storage().persistent().get(&DataKey::PriceRing(asset))?;
        let len = ring.len();
        if len == 0 {
            return None;
        }
        let take = if records < len { records } else { len };
        let mut sum: i128 = 0;
        for i in (len - take)..len {
            // Prices are 1e7-scaled USD values, so a RING_CAP-entry sum sits
            // far below i128::MAX; checked add still traps on absurd inputs
            // rather than wrapping.
            sum = sum.checked_add(ring.get_unchecked(i).price)?;
        }
        Some(sum / take as i128)
    }
}

// ---------------------------------------------------------------------------
// Benchmark-only entrypoints (`bench` feature, off by default).
//
// These exist to measure the Soroban host cost of alternative signature
// schemes and storage types. They perform NO publisher, staleness, or
// monotonicity checks — a caller can write any price with any key. Production
// builds exclude them; never deploy a bench build to a network where the
// oracle is consumed.
// ---------------------------------------------------------------------------
#[cfg(feature = "bench")]
#[contractimpl]
impl OracleV0 {
    // -------- Ed25519 (publisher pubkeys passed as args) --------
    pub fn update_ed25519_args(
        env: Env,
        asset: BytesN<8>,
        price: i128,
        timestamp: u64,
        round_id: u64,
        pubkeys: Vec<BytesN<32>>,
        sigs: Vec<BytesN<64>>,
    ) {
        let msg = build_msg(&env, &asset, price, timestamp, round_id);
        let n = pubkeys.len();
        for i in 0..n {
            env.crypto().ed25519_verify(
                &pubkeys.get_unchecked(i),
                &msg,
                &sigs.get_unchecked(i),
            );
        }
        write_temp(&env, asset, PriceEntry { price, timestamp, round_id });
    }

    // -------- Ed25519 (publisher pubkeys from instance storage) --------
    pub fn update_ed25519_stored(
        env: Env,
        asset: BytesN<8>,
        price: i128,
        timestamp: u64,
        round_id: u64,
        sigs: Vec<BytesN<64>>,
    ) {
        let pubkeys: Vec<BytesN<32>> = env
            .storage()
            .instance()
            .get(&DataKey::Publishers)
            .unwrap();
        let msg = build_msg(&env, &asset, price, timestamp, round_id);
        let n = sigs.len();
        for i in 0..n {
            env.crypto().ed25519_verify(
                &pubkeys.get_unchecked(i),
                &msg,
                &sigs.get_unchecked(i),
            );
        }
        write_temp(&env, asset, PriceEntry { price, timestamp, round_id });
    }

    // -------- Ed25519, Persistent storage (for storage-type comparison) --------
    pub fn update_ed25519_persistent(
        env: Env,
        asset: BytesN<8>,
        price: i128,
        timestamp: u64,
        round_id: u64,
        pubkeys: Vec<BytesN<32>>,
        sigs: Vec<BytesN<64>>,
    ) {
        let msg = build_msg(&env, &asset, price, timestamp, round_id);
        for i in 0..pubkeys.len() {
            env.crypto().ed25519_verify(
                &pubkeys.get_unchecked(i),
                &msg,
                &sigs.get_unchecked(i),
            );
        }
        let entry = PriceEntry { price, timestamp, round_id };
        let key = DataKey::PricePers(asset);
        env.storage().persistent().set(&key, &entry);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERS_THRESHOLD, PERS_EXTEND);
    }

    // -------- BLS aggregate (same message) --------
    pub fn update_bls_agg(
        env: Env,
        asset: BytesN<8>,
        price: i128,
        timestamp: u64,
        round_id: u64,
        agg_sig: Bls12381G1Affine,
        h_msg: Bls12381G1Affine,
        neg_g2_gen: Bls12381G2Affine,
        pubkeys: Vec<Bls12381G2Affine>,
    ) {
        let bls = env.crypto().bls12_381();
        let n = pubkeys.len();
        let mut agg_pk = pubkeys.get_unchecked(0);
        for i in 1..n {
            agg_pk = bls.g2_add(&agg_pk, &pubkeys.get_unchecked(i));
        }
        let mut g1 = Vec::new(&env);
        g1.push_back(agg_sig);
        g1.push_back(h_msg);
        let mut g2 = Vec::new(&env);
        g2.push_back(neg_g2_gen);
        g2.push_back(agg_pk);
        if !bls.pairing_check(g1, g2) {
            panic!("bls verify failed");
        }
        write_temp(&env, asset, PriceEntry { price, timestamp, round_id });
    }

    // -------- secp256k1 recover-and-compare --------
    pub fn update_secp256k1(
        env: Env,
        asset: BytesN<8>,
        price: i128,
        timestamp: u64,
        round_id: u64,
        expected_pubkeys: Vec<BytesN<65>>,
        sigs: Vec<BytesN<64>>,
        recovery_ids: Vec<u32>,
    ) {
        let msg = build_msg(&env, &asset, price, timestamp, round_id);
        let crypto = env.crypto();
        let digest = crypto.sha256(&msg);
        for i in 0..expected_pubkeys.len() {
            let recovered = crypto.secp256k1_recover(
                &digest,
                &sigs.get_unchecked(i),
                recovery_ids.get_unchecked(i),
            );
            if recovered != expected_pubkeys.get_unchecked(i) {
                panic!("secp256k1 mismatch");
            }
        }
        write_temp(&env, asset, PriceEntry { price, timestamp, round_id });
    }

    // -------- Auth-entry path (each publisher Address must require_auth) --------
    pub fn update_via_auth(
        env: Env,
        asset: BytesN<8>,
        price: i128,
        timestamp: u64,
        round_id: u64,
        publisher_addrs: Vec<Address>,
    ) {
        for addr in publisher_addrs.iter() {
            addr.require_auth();
        }
        write_temp(&env, asset, PriceEntry { price, timestamp, round_id });
    }
}

// Quorum threshold with a default of 1: deployments that never call
// set_quorum keep today's single-publisher behavior.
fn quorum(env: &Env) -> u32 {
    env.storage().instance().get(&DataKey::Quorum).unwrap_or(1)
}

// Median of a non-empty price vector (callers guarantee at least one entry —
// quorum is validated >= 1). Insertion sort into ascending order: the count
// is publisher-quorum sized, so O(m^2) is negligible. Odd count takes the
// middle; even count takes the arithmetic mean of the two middles via
// checked i128 addition — prices are 1e7-scaled USD values, far below
// i128::MAX / 2, so the checked add can only trap on absurd inputs (never
// wrap).
fn median(env: &Env, prices: &Vec<i128>) -> i128 {
    let mut sorted: Vec<i128> = Vec::new(env);
    for p in prices.iter() {
        let mut idx = sorted.len();
        for j in 0..sorted.len() {
            if p < sorted.get_unchecked(j) {
                idx = j;
                break;
            }
        }
        sorted.insert(idx, p);
    }
    let m = sorted.len();
    if m % 2 == 1 {
        sorted.get_unchecked(m / 2)
    } else {
        let a = sorted.get_unchecked(m / 2 - 1);
        let b = sorted.get_unchecked(m / 2);
        a.checked_add(b).unwrap() / 2
    }
}

fn is_publisher(publishers: &Vec<BytesN<32>>, pubkey: &BytesN<32>) -> bool {
    for i in 0..publishers.len() {
        let p = publishers.get_unchecked(i);
        if &p == pubkey {
            return true;
        }
    }
    false
}

fn build_msg(
    env: &Env,
    asset: &BytesN<8>,
    price: i128,
    timestamp: u64,
    round_id: u64,
) -> Bytes {
    let mut msg = Bytes::new(env);
    msg.extend_from_array(&asset.to_array());
    msg.extend_from_array(&price.to_be_bytes());
    msg.extend_from_array(&timestamp.to_be_bytes());
    msg.extend_from_array(&round_id.to_be_bytes());
    msg
}

fn write_temp(env: &Env, asset: BytesN<8>, entry: PriceEntry) {
    let key = DataKey::PriceTemp(asset);
    env.storage().temporary().set(&key, &entry);
    env.storage()
        .temporary()
        .extend_ttl(&key, TEMP_THRESHOLD, TEMP_EXTEND);
}

fn write_pers(env: &Env, asset: BytesN<8>, entry: PriceEntry) {
    let key = DataKey::PricePers(asset.clone());
    env.storage().persistent().set(&key, &entry);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERS_THRESHOLD, PERS_EXTEND);
    push_ring(env, asset, entry);
}

// Append a round to the asset's history ring, evicting the oldest entry once
// the ring exceeds RING_CAP. Lives inside write_pers so BOTH persistent
// writers — the single-publisher batch path and the quorum median path —
// feed history, and only rounds that passed the monotonic round_id check
// ever land here (write_pers is called strictly behind that check).
fn push_ring(env: &Env, asset: BytesN<8>, entry: PriceEntry) {
    let key = DataKey::PriceRing(asset);
    let mut ring: Vec<PriceEntry> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    ring.push_back(entry);
    if ring.len() > RING_CAP {
        // At RING_CAP = 32 the O(n) shift from remove(0) is negligible.
        ring.remove(0);
    }
    env.storage().persistent().set(&key, &ring);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERS_THRESHOLD, PERS_EXTEND);
}

#[cfg(test)]
mod test;
