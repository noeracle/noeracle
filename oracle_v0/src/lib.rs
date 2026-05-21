#![no_std]

//! Noeracle oracle_v0 — deployable contract for end-to-end XLM cost measurement.
//!
//! Each `update_price_*` entrypoint matches one of the protocol shapes we are
//! comparing in the bench (Ed25519 args, Ed25519 stored, Ed25519 persistent,
//! BLS aggregate same-msg, secp256k1 recover, auth entry). `get_price` reads
//! the latest entry.

use soroban_sdk::{
    contract, contractimpl, contracttype,
    crypto::bls12_381::{Bls12381G1Affine, Bls12381G2Affine},
    Address, Bytes, BytesN, Env, Vec,
};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
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
    pub fn init(env: Env, publishers: Vec<BytesN<32>>) {
        env.storage().instance().set(&DataKey::Publishers, &publishers);
    }

    pub fn set_publishers(env: Env, publishers: Vec<BytesN<32>>) {
        env.storage().instance().set(&DataKey::Publishers, &publishers);
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
    ) {
        let n = assets.len();
        if prices.len() != n || sigs.len() != n {
            panic!("batch length mismatch");
        }
        let crypto = env.crypto();
        for i in 0..n {
            let asset = assets.get_unchecked(i);
            let price = prices.get_unchecked(i);
            let msg = build_msg(&env, &asset, price, timestamp, round_id);
            crypto.ed25519_verify(&pubkey, &msg, &sigs.get_unchecked(i));
            write_temp(&env, asset, PriceEntry { price, timestamp, round_id });
        }
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
