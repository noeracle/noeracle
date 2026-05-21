//! Tests for the hardened `update_batch_ed25519_args` production path and the
//! admin-gated publisher set: happy path, unknown publisher, stale price,
//! stale round, batch-length mismatch, double init, and publisher rotation.

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
fn rejects_stale_round() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let (a1, p1, pk1, s1) = sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 5);
    client.update_batch_ed25519_args(&a1, &p1, &BASE_TS, &5u64, &pk1, &s1);

    // The same round_id again is not strictly greater — must be rejected.
    let (a2, p2, pk2, s2) = sign_round(&env, &signer, &[(ASSET_BTC, 200)], BASE_TS, 5);
    let res = client.try_update_batch_ed25519_args(&a2, &p2, &BASE_TS, &5u64, &pk2, &s2);
    assert_eq!(res, Err(Ok(Error::StaleRound)));
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
