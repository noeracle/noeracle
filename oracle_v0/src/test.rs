//! Tests for the hardened production paths — `update_batch_ed25519_args`
//! (temporary storage) and `update_batch_ed25519_persistent` (persistent
//! storage) — and the admin-gated publisher set: happy path, unknown
//! publisher, stale price, stale round, batch-length mismatch, double init,
//! init auth, storage isolation, and publisher rotation.

extern crate std;

use crate::{Error, OracleV0, OracleV0Client};
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Vec,
};

const BASE_TS: u64 = 1_000_000;
const ASSET_BTC: [u8; 8] = *b"BTCUSD\0\0";
const ASSET_ETH: [u8; 8] = *b"ETHUSD\0\0";

// The 40-byte signed message, byte-identical to the contract's build_msg:
// asset(8) || price(i128 BE) || timestamp(u64 BE) || round_id(u64 BE).
fn msg_bytes(asset: &[u8; 8], price: i128, ts: u64, round: u64) -> [u8; 40] {
    let mut m = [0u8; 40];
    m[0..8].copy_from_slice(asset);
    m[8..24].copy_from_slice(&price.to_be_bytes());
    m[24..32].copy_from_slice(&ts.to_be_bytes());
    m[32..40].copy_from_slice(&round.to_be_bytes());
    m
}

// Register the contract, mock auth, set a base ledger time, and init with a
// single freshly generated publisher key. Returns the client and that key.
fn setup(env: &Env) -> (OracleV0Client<'_>, SigningKey) {
    env.mock_all_auths();
    env.ledger().set_timestamp(BASE_TS);
    let contract_id = env.register(OracleV0, ());
    let client = OracleV0Client::new(env, &contract_id);

    let signer = SigningKey::generate(&mut OsRng);
    let admin = Address::generate(env);
    let mut publishers = Vec::new(env);
    publishers.push_back(BytesN::from_array(env, &signer.verifying_key().to_bytes()));
    client.init(&admin, &publishers);
    (client, signer)
}

// Sign a price round for the given (asset, price) items and return the
// arguments for update_batch_ed25519_args.
fn sign_round(
    env: &Env,
    signer: &SigningKey,
    items: &[([u8; 8], i128)],
    ts: u64,
    round: u64,
) -> (Vec<BytesN<8>>, Vec<i128>, BytesN<32>, Vec<BytesN<64>>) {
    let mut assets = Vec::new(env);
    let mut prices = Vec::new(env);
    let mut sigs = Vec::new(env);
    for (asset, price) in items {
        let sig = signer.sign(&msg_bytes(asset, *price, ts, round));
        assets.push_back(BytesN::from_array(env, asset));
        prices.push_back(*price);
        sigs.push_back(BytesN::from_array(env, &sig.to_bytes()));
    }
    let pubkey = BytesN::from_array(env, &signer.verifying_key().to_bytes());
    (assets, prices, pubkey, sigs)
}

#[test]
fn happy_path_stores_price() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let price = 65_432_10000000i128;
    let (assets, prices, pubkey, sigs) =
        sign_round(&env, &signer, &[(ASSET_BTC, price)], BASE_TS, 1);

    client.update_batch_ed25519_args(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);

    let stored = client
        .get_price(&BytesN::from_array(&env, &ASSET_BTC))
        .unwrap();
    assert_eq!(stored.price, price);
    assert_eq!(stored.timestamp, BASE_TS);
    assert_eq!(stored.round_id, 1);
}

#[test]
fn batch_stores_multiple_assets() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let (assets, prices, pubkey, sigs) = sign_round(
        &env,
        &signer,
        &[(ASSET_BTC, 65_000_0000000), (ASSET_ETH, 3_200_0000000)],
        BASE_TS,
        1,
    );

    client.update_batch_ed25519_args(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);

    let btc = client
        .get_price(&BytesN::from_array(&env, &ASSET_BTC))
        .unwrap();
    let eth = client
        .get_price(&BytesN::from_array(&env, &ASSET_ETH))
        .unwrap();
    assert_eq!(btc.price, 65_000_0000000);
    assert_eq!(eth.price, 3_200_0000000);
}

#[test]
fn rejects_unknown_publisher() {
    let env = Env::default();
    let (client, _signer) = setup(&env);
    let rogue = SigningKey::generate(&mut OsRng);
    let (assets, prices, pubkey, sigs) =
        sign_round(&env, &rogue, &[(ASSET_BTC, 100)], BASE_TS, 1);

    let res =
        client.try_update_batch_ed25519_args(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);
    assert_eq!(res, Err(Ok(Error::UnknownPublisher)));
}

#[test]
fn rejects_stale_price() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    // Round signed at BASE_TS, but the ledger has advanced 120s past it.
    env.ledger().set_timestamp(BASE_TS + 120);
    let (assets, prices, pubkey, sigs) =
        sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 1);

    let res =
        client.try_update_batch_ed25519_args(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);
    assert_eq!(res, Err(Ok(Error::StalePrice)));
}

#[test]
fn non_newer_round_is_silent_noop() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    // Round 5 establishes the cached price.
    let (a1, p1, pk1, s1) = sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 5);
    client.update_batch_ed25519_args(&a1, &p1, &BASE_TS, &5u64, &pk1, &s1);

    // A lagging round (3) and an equal round (5) must NOT fail the call — a
    // consumer's tx cannot be rejected because of cross-consumer ordering.
    let (a2, p2, pk2, s2) = sign_round(&env, &signer, &[(ASSET_BTC, 200)], BASE_TS, 3);
    client.update_batch_ed25519_args(&a2, &p2, &BASE_TS, &3u64, &pk2, &s2);
    let (a3, p3, pk3, s3) = sign_round(&env, &signer, &[(ASSET_BTC, 300)], BASE_TS, 5);
    client.update_batch_ed25519_args(&a3, &p3, &BASE_TS, &5u64, &pk3, &s3);

    // The cache still holds the round-5 price; lagging rounds were skipped.
    let stored = client
        .get_price(&BytesN::from_array(&env, &ASSET_BTC))
        .unwrap();
    assert_eq!(stored.price, 100);
    assert_eq!(stored.round_id, 5);

    // A strictly newer round advances the cache.
    let (a4, p4, pk4, s4) = sign_round(&env, &signer, &[(ASSET_BTC, 400)], BASE_TS, 6);
    client.update_batch_ed25519_args(&a4, &p4, &BASE_TS, &6u64, &pk4, &s4);
    let stored = client
        .get_price(&BytesN::from_array(&env, &ASSET_BTC))
        .unwrap();
    assert_eq!(stored.price, 400);
    assert_eq!(stored.round_id, 6);
}

#[test]
fn rejects_batch_length_mismatch() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let (assets, _prices, pubkey, sigs) =
        sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 1);
    let empty_prices: Vec<i128> = Vec::new(&env);

    let res = client
        .try_update_batch_ed25519_args(&assets, &empty_prices, &BASE_TS, &1u64, &pubkey, &sigs);
    assert_eq!(res, Err(Ok(Error::BatchLengthMismatch)));
}

#[test]
fn init_requires_admin_auth() {
    let env = Env::default();
    // Deliberately NO mock_all_auths: a freshly deployed instance must refuse
    // an init whose admin has not authorized the call (first-writer-wins
    // front-running protection).
    let contract_id = env.register(OracleV0, ());
    let client = OracleV0Client::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let publishers: Vec<BytesN<32>> = Vec::new(&env);
    assert!(client.try_init(&admin, &publishers).is_err());
}

#[test]
fn rejects_double_init() {
    let env = Env::default();
    let (client, _signer) = setup(&env); // setup already called init once
    let admin = Address::generate(&env);
    let publishers: Vec<BytesN<32>> = Vec::new(&env);
    let res = client.try_init(&admin, &publishers);
    assert_eq!(res, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn set_publishers_rotates_the_key_set() {
    let env = Env::default();
    let (client, old_signer) = setup(&env);

    let new_signer = SigningKey::generate(&mut OsRng);
    let mut publishers = Vec::new(&env);
    publishers.push_back(BytesN::from_array(
        &env,
        &new_signer.verifying_key().to_bytes(),
    ));
    client.set_publishers(&publishers);

    // The old key is no longer a registered publisher.
    let (a, p, pk, s) = sign_round(&env, &old_signer, &[(ASSET_BTC, 100)], BASE_TS, 1);
    assert_eq!(
        client.try_update_batch_ed25519_args(&a, &p, &BASE_TS, &1u64, &pk, &s),
        Err(Ok(Error::UnknownPublisher))
    );

    // The new key is accepted.
    let (a2, p2, pk2, s2) = sign_round(&env, &new_signer, &[(ASSET_BTC, 100)], BASE_TS, 1);
    client.update_batch_ed25519_args(&a2, &p2, &BASE_TS, &1u64, &pk2, &s2);
}

// ---------------------------------------------------------------------------
// update_batch_ed25519_persistent — same hardened checks, persistent storage.
// ---------------------------------------------------------------------------

#[test]
fn persistent_happy_path_stores_price() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let price = 65_432_10000000i128;
    let (assets, prices, pubkey, sigs) =
        sign_round(&env, &signer, &[(ASSET_BTC, price)], BASE_TS, 1);

    client.update_batch_ed25519_persistent(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);

    let stored = client
        .get_price_pers(&BytesN::from_array(&env, &ASSET_BTC))
        .unwrap();
    assert_eq!(stored.price, price);
    assert_eq!(stored.timestamp, BASE_TS);
    assert_eq!(stored.round_id, 1);
}

#[test]
fn persistent_batch_stores_multiple_assets() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let (assets, prices, pubkey, sigs) = sign_round(
        &env,
        &signer,
        &[(ASSET_BTC, 65_000_0000000), (ASSET_ETH, 3_200_0000000)],
        BASE_TS,
        1,
    );

    client.update_batch_ed25519_persistent(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);

    let btc = client
        .get_price_pers(&BytesN::from_array(&env, &ASSET_BTC))
        .unwrap();
    let eth = client
        .get_price_pers(&BytesN::from_array(&env, &ASSET_ETH))
        .unwrap();
    assert_eq!(btc.price, 65_000_0000000);
    assert_eq!(eth.price, 3_200_0000000);
}

#[test]
fn persistent_rejects_unknown_publisher() {
    let env = Env::default();
    let (client, _signer) = setup(&env);
    let rogue = SigningKey::generate(&mut OsRng);
    let (assets, prices, pubkey, sigs) =
        sign_round(&env, &rogue, &[(ASSET_BTC, 100)], BASE_TS, 1);

    let res = client
        .try_update_batch_ed25519_persistent(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);
    assert_eq!(res, Err(Ok(Error::UnknownPublisher)));
    // The rogue write must not have landed.
    assert!(client
        .get_price_pers(&BytesN::from_array(&env, &ASSET_BTC))
        .is_none());
}

#[test]
fn persistent_rejects_stale_price() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    env.ledger().set_timestamp(BASE_TS + 120);
    let (assets, prices, pubkey, sigs) =
        sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 1);

    let res = client
        .try_update_batch_ed25519_persistent(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);
    assert_eq!(res, Err(Ok(Error::StalePrice)));
}

#[test]
fn persistent_non_newer_round_is_silent_noop() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let (a1, p1, pk1, s1) = sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 5);
    client.update_batch_ed25519_persistent(&a1, &p1, &BASE_TS, &5u64, &pk1, &s1);

    // Lagging (3) and equal (5) rounds must not fail and must not overwrite.
    let (a2, p2, pk2, s2) = sign_round(&env, &signer, &[(ASSET_BTC, 200)], BASE_TS, 3);
    client.update_batch_ed25519_persistent(&a2, &p2, &BASE_TS, &3u64, &pk2, &s2);
    let (a3, p3, pk3, s3) = sign_round(&env, &signer, &[(ASSET_BTC, 300)], BASE_TS, 5);
    client.update_batch_ed25519_persistent(&a3, &p3, &BASE_TS, &5u64, &pk3, &s3);

    let stored = client
        .get_price_pers(&BytesN::from_array(&env, &ASSET_BTC))
        .unwrap();
    assert_eq!(stored.price, 100);
    assert_eq!(stored.round_id, 5);

    // A strictly newer round advances the cache.
    let (a4, p4, pk4, s4) = sign_round(&env, &signer, &[(ASSET_BTC, 400)], BASE_TS, 6);
    client.update_batch_ed25519_persistent(&a4, &p4, &BASE_TS, &6u64, &pk4, &s4);
    let stored = client
        .get_price_pers(&BytesN::from_array(&env, &ASSET_BTC))
        .unwrap();
    assert_eq!(stored.price, 400);
    assert_eq!(stored.round_id, 6);
}

#[test]
fn persistent_rejects_batch_length_mismatch() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let (assets, _prices, pubkey, sigs) =
        sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 1);
    let empty_prices: Vec<i128> = Vec::new(&env);

    let res = client.try_update_batch_ed25519_persistent(
        &assets,
        &empty_prices,
        &BASE_TS,
        &1u64,
        &pubkey,
        &sigs,
    );
    assert_eq!(res, Err(Ok(Error::BatchLengthMismatch)));
}

#[test]
fn persistent_write_does_not_touch_temporary_cache() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let (assets, prices, pubkey, sigs) =
        sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 1);

    client.update_batch_ed25519_persistent(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);

    // The two storage tiers are independent caches: a persistent round must
    // not appear via get_price, and vice versa.
    assert!(client
        .get_price(&BytesN::from_array(&env, &ASSET_BTC))
        .is_none());
    assert!(client
        .get_price_pers(&BytesN::from_array(&env, &ASSET_BTC))
        .is_some());
}

// Decode an N-byte value from a hex string (test helper).
fn hex_bytes<const N: usize>(s: &str) -> [u8; N] {
    let b = s.as_bytes();
    let mut out = [0u8; N];
    for i in 0..N {
        let hi = (b[2 * i] as char).to_digit(16).unwrap() as u8;
        let lo = (b[2 * i + 1] as char).to_digit(16).unwrap() as u8;
        out[i] = (hi << 4) | lo;
    }
    out
}

// Cross-language conformance: a message + signature produced by the JS keeper
// encoder (scripts/keeper/message.mjs) must verify against this contract's
// build_msg. The same golden vector is pinned on the JS side by
// scripts/keeper/message.test.mjs. If the 40-byte format drifts in either
// language, one of the two tests fails.
#[test]
fn conformance_with_js_keeper_encoder() {
    // Golden vector — asset "BTCUSD", price 6543210000000, ts 1700000000,
    // round 1, signed with the test key 0x01 repeated 32 times.
    const PUBKEY: &str =
        "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c";
    const SIGNATURE: &str = "effc695c3dd70bc5da1bcea2475739340951a1fce74a6fd9e3c8bebae3147e7334b9d7f0bdb4d0770c94c8d7ad80094ddef708c30eddcb7a5b5e7a0f081e8200";

    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_700_000_000);
    let contract_id = env.register(OracleV0, ());
    let client = OracleV0Client::new(&env, &contract_id);

    let pubkey: BytesN<32> = BytesN::from_array(&env, &hex_bytes::<32>(PUBKEY));
    let admin = Address::generate(&env);
    let mut publishers = Vec::new(&env);
    publishers.push_back(pubkey.clone());
    client.init(&admin, &publishers);

    let mut assets = Vec::new(&env);
    assets.push_back(BytesN::from_array(&env, b"BTCUSD\0\0"));
    let mut prices = Vec::new(&env);
    prices.push_back(6_543_210_000_000i128);
    let mut sigs = Vec::new(&env);
    sigs.push_back(BytesN::from_array(&env, &hex_bytes::<64>(SIGNATURE)));

    // A one-byte disagreement between build_msg and the JS encoder would make
    // this Ed25519 verification fail.
    client.update_batch_ed25519_args(
        &assets, &prices, &1_700_000_000u64, &1u64, &pubkey, &sigs,
    );

    let stored = client
        .get_price(&BytesN::from_array(&env, b"BTCUSD\0\0"))
        .unwrap();
    assert_eq!(stored.price, 6_543_210_000_000);
    assert_eq!(stored.round_id, 1);
}

// The persistent batch path must accept the exact same golden vector — both
// production entrypoints share one 40-byte signed format, so off-chain
// signing code never branches on the storage tier.
#[test]
fn conformance_persistent_with_js_keeper_encoder() {
    const PUBKEY: &str =
        "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c";
    const SIGNATURE: &str = "effc695c3dd70bc5da1bcea2475739340951a1fce74a6fd9e3c8bebae3147e7334b9d7f0bdb4d0770c94c8d7ad80094ddef708c30eddcb7a5b5e7a0f081e8200";

    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_700_000_000);
    let contract_id = env.register(OracleV0, ());
    let client = OracleV0Client::new(&env, &contract_id);

    let pubkey: BytesN<32> = BytesN::from_array(&env, &hex_bytes::<32>(PUBKEY));
    let admin = Address::generate(&env);
    let mut publishers = Vec::new(&env);
    publishers.push_back(pubkey.clone());
    client.init(&admin, &publishers);

    let mut assets = Vec::new(&env);
    assets.push_back(BytesN::from_array(&env, b"BTCUSD\0\0"));
    let mut prices = Vec::new(&env);
    prices.push_back(6_543_210_000_000i128);
    let mut sigs = Vec::new(&env);
    sigs.push_back(BytesN::from_array(&env, &hex_bytes::<64>(SIGNATURE)));

    client.update_batch_ed25519_persistent(
        &assets, &prices, &1_700_000_000u64, &1u64, &pubkey, &sigs,
    );

    let stored = client
        .get_price_pers(&BytesN::from_array(&env, b"BTCUSD\0\0"))
        .unwrap();
    assert_eq!(stored.price, 6_543_210_000_000);
    assert_eq!(stored.round_id, 1);
}
