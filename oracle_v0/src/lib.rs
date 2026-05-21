#![no_std]

//! Noeracle oracle_v0 — Soroban pull-oracle contract.
//!
//! `update_batch_ed25519_args` is the production pull-mode entrypoint: a
//! consumer bundles it into their own transaction to verify a freshly signed
//! price round inline. It checks the signer is a registered publisher, the
//! round is fresh, and round_id is monotonic. The other `update_*` entrypoints
//! exist to measure the Soroban host cost of alternative signature schemes and
//! are not part of the production path. `get_price` reads the latest entry.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype,
    crypto::bls12_381::{Bls12381G1Affine, Bls12381G2Affine},
    Address, Bytes, BytesN, Env, Vec,
};

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
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Publishers,
    PriceTemp(BytesN<8>),
    PricePers(BytesN<8>),
}

#[contracttype]
#[derive(Clone)]
pub struct PriceEntry {
    pub price: i128,
    pub timestamp: u64,
    pub round_id: u64,
}

// Reject signed prices whose timestamp is older than this many seconds.
const STALENESS_SECS: u64 = 60;

// Conservative TTLs: keep entries alive a bit longer than minimum so a
// keeper outage of a few minutes doesn't immediately archive everything.
const TEMP_THRESHOLD: u32 = 360;
const TEMP_EXTEND: u32 = 720;
const PERS_THRESHOLD: u32 = 60_480;
const PERS_EXTEND: u32 = 120_960;

#[contract]
pub struct OracleV0;

#[contractimpl]
impl OracleV0 {
    /// One-time setup: record the admin Address and the initial publisher
    /// Ed25519 key set. Errors if the contract is already initialized.
    pub fn init(
        env: Env,
        admin: Address,
        publishers: Vec<BytesN<32>>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Publishers, &publishers);
        Ok(())
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
        if now.saturating_sub(timestamp) > STALENESS_SECS {
            return Err(Error::StalePrice);
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

    // -------- Reads --------
    pub fn get_price(env: Env, asset: BytesN<8>) -> Option<PriceEntry> {
        env.storage().temporary().get(&DataKey::PriceTemp(asset))
    }

    pub fn get_price_pers(env: Env, asset: BytesN<8>) -> Option<PriceEntry> {
        env.storage().persistent().get(&DataKey::PricePers(asset))
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

#[cfg(test)]
mod test;
