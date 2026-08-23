//! Tests for the hardened production paths — `update_batch_ed25519_args`
//! (temporary storage), `update_batch_ed25519_persistent` (persistent
//! storage), and `update_quorum_ed25519_persistent` (M-of-N median) — plus
//! the admin surface (publisher set, quorum config, upgrade) and the history
//! ring views (`prices`/`twap`): happy paths, unknown/duplicate publisher,
//! quorum not met, per-asset median (odd/even), one-bad-signature revert,
//! stale price, future timestamp, price bounds, batch-length mismatch, constructor auth,
//! storage isolation, ring capacity/ordering, and publisher rotation.

extern crate std;

use crate::{Error, OracleV0, OracleV0Client, PublisherRound};
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
    let signer = SigningKey::generate(&mut OsRng);
    let admin = Address::generate(env);
    let mut publishers = Vec::new(env);
    publishers.push_back(BytesN::from_array(env, &signer.verifying_key().to_bytes()));
    // Constructor-initialized: admin + publisher set land atomically at deploy.
    let contract_id = env.register(OracleV0, (admin.clone(), publishers.clone()));
    let client = OracleV0Client::new(env, &contract_id);
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

// Register the contract, mock auth, set a base ledger time, and init with
// `n` freshly generated publisher keys. Returns the client and the keys.
fn setup_multi(env: &Env, n: usize) -> (OracleV0Client<'_>, std::vec::Vec<SigningKey>) {
    env.mock_all_auths();
    env.ledger().set_timestamp(BASE_TS);
    let admin = Address::generate(env);
    let mut signers = std::vec::Vec::new();
    let mut publishers = Vec::new(env);
    for _ in 0..n {
        let signer = SigningKey::generate(&mut OsRng);
        publishers.push_back(BytesN::from_array(env, &signer.verifying_key().to_bytes()));
        signers.push(signer);
    }
    let contract_id = env.register(OracleV0, (admin.clone(), publishers.clone()));
    let client = OracleV0Client::new(env, &contract_id);
    (client, signers)
}

// Sign one synchronized quorum round: every signer signs the shared
// (assets, ts, round) over its own per-asset prices, using the exact same
// 40-byte msg_bytes format as the single-publisher paths.
// `prices_per_signer` is indexed [signer][asset]. Returns the shared assets
// vec and the PublisherRound list for update_quorum_ed25519_persistent.
fn sign_quorum_round(
    env: &Env,
    signers: &[SigningKey],
    assets: &[[u8; 8]],
    prices_per_signer: &[&[i128]],
    ts: u64,
    round: u64,
) -> (Vec<BytesN<8>>, Vec<PublisherRound>) {
    let mut assets_vec = Vec::new(env);
    for asset in assets {
        assets_vec.push_back(BytesN::from_array(env, asset));
    }
    let mut rounds = Vec::new(env);
    for (signer, prices) in signers.iter().zip(prices_per_signer.iter()) {
        let mut prices_vec = Vec::new(env);
        let mut sigs = Vec::new(env);
        for (asset, price) in assets.iter().zip(prices.iter()) {
            let sig = signer.sign(&msg_bytes(asset, *price, ts, round));
            prices_vec.push_back(*price);
            sigs.push_back(BytesN::from_array(env, &sig.to_bytes()));
        }
        rounds.push_back(PublisherRound {
            pubkey: BytesN::from_array(env, &signer.verifying_key().to_bytes()),
            prices: prices_vec,
            sigs,
        });
    }
    (assets_vec, rounds)
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
fn constructor_requires_admin_auth() {
    let env = Env::default();
    // Deliberately NO mock_all_auths: a deploy whose admin has not
    // authorized the constructor must fail — a deployer cannot install
    // someone else as admin without their signature.
    let admin = Address::generate(&env);
    let publishers: Vec<BytesN<32>> = Vec::new(&env);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        env.register(OracleV0, (admin.clone(), publishers.clone()));
    }));
    assert!(result.is_err());
}

#[test]
fn future_timestamp_rejected_beyond_skew() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    // 31 s ahead of ledger time: outside FUTURE_SKEW_SECS.
    let ts = BASE_TS + 31;
    let (a, p, pk, s) = sign_round(&env, &signer, &[(ASSET_BTC, 100)], ts, 1);
    let res = client.try_update_batch_ed25519_args(&a, &p, &ts, &1u64, &pk, &s);
    assert_eq!(res, Err(Ok(Error::StalePrice)));
    // 10 s ahead: inside the clock-skew allowance, accepted.
    let ts_ok = BASE_TS + 10;
    let (a2, p2, pk2, s2) = sign_round(&env, &signer, &[(ASSET_BTC, 100)], ts_ok, 2);
    client.update_batch_ed25519_args(&a2, &p2, &ts_ok, &2u64, &pk2, &s2);
}

#[test]
fn persistent_future_timestamp_rejected_beyond_skew() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let ts = BASE_TS + 31;
    let (a, p, pk, s) = sign_round(&env, &signer, &[(ASSET_BTC, 100)], ts, 1);
    let res = client.try_update_batch_ed25519_persistent(&a, &p, &ts, &1u64, &pk, &s);
    assert_eq!(res, Err(Ok(Error::StalePrice)));
}

#[test]
fn quorum_future_timestamp_rejected_beyond_skew() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 2);
    client.set_quorum(&2u32);
    let ts = BASE_TS + 31;
    let (assets, rounds) = sign_quorum_round(
        &env, &signers, &[ASSET_BTC], &[&[100], &[102]], ts, 1,
    );
    let res = client.try_update_quorum_ed25519_persistent(&assets, &ts, &1u64, &rounds);
    assert_eq!(res, Err(Ok(Error::StalePrice)));
}

#[test]
fn rejects_nonpositive_price() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    for bad in [0i128, -1i128] {
        let (a, p, pk, s) = sign_round(&env, &signer, &[(ASSET_BTC, bad)], BASE_TS, 1);
        let res = client.try_update_batch_ed25519_args(&a, &p, &BASE_TS, &1u64, &pk, &s);
        assert_eq!(res, Err(Ok(Error::InvalidPrice)));
        let res = client.try_update_batch_ed25519_persistent(&a, &p, &BASE_TS, &1u64, &pk, &s);
        assert_eq!(res, Err(Ok(Error::InvalidPrice)));
    }
}

#[test]
fn rejects_price_above_cap() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let too_big: i128 = 1_000_000_000_000_000_000_000_000_000_001;
    let (a, p, pk, s) = sign_round(&env, &signer, &[(ASSET_BTC, too_big)], BASE_TS, 1);
    let res = client.try_update_batch_ed25519_persistent(&a, &p, &BASE_TS, &1u64, &pk, &s);
    assert_eq!(res, Err(Ok(Error::InvalidPrice)));
}

#[test]
fn quorum_rejects_out_of_bounds_price() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 2);
    client.set_quorum(&2u32);
    // One publisher submits a non-positive price: the whole round rejects
    // before any signature work.
    let (assets, rounds) = sign_quorum_round(
        &env, &signers, &[ASSET_BTC], &[&[100], &[0]], BASE_TS, 1,
    );
    let res = client.try_update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);
    assert_eq!(res, Err(Ok(Error::InvalidPrice)));
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

#[test]
fn quorum_defaults_to_one_and_is_admin_configurable() {
    let env = Env::default();
    let (client, _signer) = setup(&env); // one registered publisher
    assert_eq!(client.get_quorum(), 1);
    client.set_quorum(&1u32);
    assert_eq!(client.get_quorum(), 1);
}

#[test]
fn set_quorum_rejects_out_of_range() {
    let env = Env::default();
    let (client, _signer) = setup(&env); // one registered publisher
    assert_eq!(client.try_set_quorum(&0u32), Err(Ok(Error::InvalidQuorum)));
    // More signers than registered publishers can never be gathered.
    assert_eq!(client.try_set_quorum(&2u32), Err(Ok(Error::InvalidQuorum)));
}

#[test]
fn set_quorum_requires_admin_auth() {
    let env = Env::default();
    let (client, _signer) = setup(&env);
    // Drop the blanket auth mock installed by setup().
    env.set_auths(&[]);
    assert!(client.try_set_quorum(&1u32).is_err());
}

#[test]
fn upgrade_requires_admin_auth() {
    let env = Env::default();
    let (client, _signer) = setup(&env);
    // Drop the blanket auth mock installed by setup(): an upgrade whose admin
    // has not authorized the call must fail before any wasm swap is attempted.
    env.set_auths(&[]);
    let new_wasm_hash = BytesN::from_array(&env, &[7u8; 32]);
    assert!(client.try_upgrade(&new_wasm_hash).is_err());
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

// ---------------------------------------------------------------------------
// History ring — every persistent write appends; prices()/twap() serve it.
// ---------------------------------------------------------------------------

#[test]
fn history_views_are_none_without_persistent_writes() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let asset = BytesN::from_array(&env, &ASSET_BTC);
    assert!(client.prices(&asset, &5u32).is_none());
    assert!(client.twap(&asset, &5u32).is_none());

    // The temporary-storage path must not feed the ring either.
    let (a, p, pk, s) = sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 1);
    client.update_batch_ed25519_args(&a, &p, &BASE_TS, &1u64, &pk, &s);
    assert!(client.prices(&asset, &5u32).is_none());
    assert!(client.twap(&asset, &5u32).is_none());
}

#[test]
fn persistent_write_pushes_ring_entry() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let asset = BytesN::from_array(&env, &ASSET_BTC);
    let (a, p, pk, s) = sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 1);
    client.update_batch_ed25519_persistent(&a, &p, &BASE_TS, &1u64, &pk, &s);

    let hist = client.prices(&asset, &5u32).unwrap();
    assert_eq!(hist.len(), 1);
    let entry = hist.get_unchecked(0);
    assert_eq!(entry.price, 100);
    assert_eq!(entry.timestamp, BASE_TS);
    assert_eq!(entry.round_id, 1);
}

#[test]
fn lagging_round_does_not_push_ring_entry() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let asset = BytesN::from_array(&env, &ASSET_BTC);
    let (a1, p1, pk1, s1) = sign_round(&env, &signer, &[(ASSET_BTC, 100)], BASE_TS, 5);
    client.update_batch_ed25519_persistent(&a1, &p1, &BASE_TS, &5u64, &pk1, &s1);

    // Lagging (3) and equal (5) rounds are silent no-ops for the cache — and
    // must leave no trace in the history ring.
    let (a2, p2, pk2, s2) = sign_round(&env, &signer, &[(ASSET_BTC, 200)], BASE_TS, 3);
    client.update_batch_ed25519_persistent(&a2, &p2, &BASE_TS, &3u64, &pk2, &s2);
    let (a3, p3, pk3, s3) = sign_round(&env, &signer, &[(ASSET_BTC, 300)], BASE_TS, 5);
    client.update_batch_ed25519_persistent(&a3, &p3, &BASE_TS, &5u64, &pk3, &s3);

    let hist = client.prices(&asset, &10u32).unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist.get_unchecked(0).round_id, 5);
    assert_eq!(hist.get_unchecked(0).price, 100);
}

#[test]
fn ring_caps_at_32_latest_first() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let asset = BytesN::from_array(&env, &ASSET_BTC);
    for round in 1..=40u64 {
        let price = round as i128 * 10;
        let (a, p, pk, s) = sign_round(&env, &signer, &[(ASSET_BTC, price)], BASE_TS, round);
        client.update_batch_ed25519_persistent(&a, &p, &BASE_TS, &round, &pk, &s);
    }

    // 40 monotonic rounds, capacity 32: rounds 1..=8 were evicted.
    let hist = client.prices(&asset, &50u32).unwrap();
    assert_eq!(hist.len(), 32);
    assert_eq!(hist.get_unchecked(0).round_id, 40);
    assert_eq!(hist.get_unchecked(0).price, 400);
    assert_eq!(hist.get_unchecked(31).round_id, 9);
    assert_eq!(hist.get_unchecked(31).price, 90);
}

#[test]
fn prices_truncates_to_requested_records() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let asset = BytesN::from_array(&env, &ASSET_BTC);
    for round in 1..=3u64 {
        let price = round as i128 * 100;
        let (a, p, pk, s) = sign_round(&env, &signer, &[(ASSET_BTC, price)], BASE_TS, round);
        client.update_batch_ed25519_persistent(&a, &p, &BASE_TS, &round, &pk, &s);
    }

    let hist = client.prices(&asset, &2u32).unwrap();
    assert_eq!(hist.len(), 2);
    assert_eq!(hist.get_unchecked(0).round_id, 3);
    assert_eq!(hist.get_unchecked(1).round_id, 2);
    // records == 0 with existing history is an empty page, not None — None is
    // reserved for "no history at all".
    assert_eq!(client.prices(&asset, &0u32).unwrap().len(), 0);
}

#[test]
fn twap_is_mean_over_last_records() {
    let env = Env::default();
    let (client, signer) = setup(&env);
    let asset = BytesN::from_array(&env, &ASSET_BTC);
    for (round, price) in [(1u64, 100i128), (2, 200), (3, 300)] {
        let (a, p, pk, s) = sign_round(&env, &signer, &[(ASSET_BTC, price)], BASE_TS, round);
        client.update_batch_ed25519_persistent(&a, &p, &BASE_TS, &round, &pk, &s);
    }

    assert_eq!(client.twap(&asset, &3u32), Some(200)); // (100+200+300)/3
    assert_eq!(client.twap(&asset, &2u32), Some(250)); // (200+300)/2
    assert_eq!(client.twap(&asset, &1u32), Some(300));
    // records beyond stored history clamps to what is stored.
    assert_eq!(client.twap(&asset, &10u32), Some(200));
    // records == 0 is meaningless — None, never a fabricated value.
    assert_eq!(client.twap(&asset, &0u32), None);
}

// ---------------------------------------------------------------------------
// update_quorum_ed25519_persistent — M-of-N median path.
// ---------------------------------------------------------------------------

#[test]
fn quorum_2_of_3_stores_median_and_ring_entry() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 3);
    client.set_quorum(&2u32);
    let asset = BytesN::from_array(&env, &ASSET_BTC);

    // Two of the three registered publishers submit; even count -> mean of
    // the two middles: (100 + 200) / 2 = 150.
    let (assets, rounds) = sign_quorum_round(
        &env,
        &signers[0..2],
        &[ASSET_BTC],
        &[&[100], &[200]],
        BASE_TS,
        1,
    );
    client.update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);

    let stored = client.get_price_pers(&asset).unwrap();
    assert_eq!(stored.price, 150);
    assert_eq!(stored.timestamp, BASE_TS);
    assert_eq!(stored.round_id, 1);

    // The median round landed in the history ring too...
    let hist = client.prices(&asset, &10u32).unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist.get_unchecked(0).price, 150);
    // ...and the temporary-storage cache stays untouched.
    assert!(client.get_price(&asset).is_none());
}

#[test]
fn quorum_default_of_one_accepts_single_round() {
    let env = Env::default();
    // No set_quorum call: an unconfigured deployment behaves exactly like
    // the single-publisher path (quorum defaults to 1).
    let (client, signers) = setup_multi(&env, 2);
    let asset = BytesN::from_array(&env, &ASSET_BTC);

    let (assets, rounds) =
        sign_quorum_round(&env, &signers[0..1], &[ASSET_BTC], &[&[100]], BASE_TS, 1);
    client.update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);

    assert_eq!(client.get_price_pers(&asset).unwrap().price, 100);
}

#[test]
fn quorum_rejects_below_threshold() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 3);
    client.set_quorum(&2u32);
    let asset = BytesN::from_array(&env, &ASSET_BTC);

    let (assets, rounds) =
        sign_quorum_round(&env, &signers[0..1], &[ASSET_BTC], &[&[100]], BASE_TS, 1);
    let res = client.try_update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);
    assert_eq!(res, Err(Ok(Error::QuorumNotMet)));
    assert!(client.get_price_pers(&asset).is_none());
}

#[test]
fn quorum_above_one_closes_the_single_publisher_batch_path() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 3);
    let asset = BytesN::from_array(&env, &ASSET_BTC);

    // A perfectly valid single-publisher batch...
    let (assets, prices, pubkey, sigs) =
        sign_round(&env, &signers[0], &[(ASSET_BTC, 100)], BASE_TS, 1);

    // ...is rejected outright while quorum > 1: PricePers is shared, so the
    // batch path would otherwise let ONE compromised key bypass the M-of-N
    // median entirely.
    client.set_quorum(&2u32);
    let res = client.try_update_batch_ed25519_persistent(
        &assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs,
    );
    assert_eq!(res, Err(Ok(Error::QuorumNotMet)));
    assert!(client.get_price_pers(&asset).is_none());

    // Lowering the quorum back to 1 re-opens it unchanged.
    client.set_quorum(&1u32);
    client.update_batch_ed25519_persistent(&assets, &prices, &BASE_TS, &1u64, &pubkey, &sigs);
    assert_eq!(client.get_price_pers(&asset).unwrap().price, 100);
}

#[test]
fn quorum_rejects_duplicate_publisher() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 3);
    client.set_quorum(&2u32);

    // The same registered key signing twice must not count toward quorum.
    let dup = [signers[0].clone(), signers[0].clone()];
    let (assets, rounds) =
        sign_quorum_round(&env, &dup, &[ASSET_BTC], &[&[100], &[100]], BASE_TS, 1);
    let res = client.try_update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);
    assert_eq!(res, Err(Ok(Error::DuplicatePublisher)));
}

#[test]
fn quorum_rejects_unknown_publisher() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 2);
    client.set_quorum(&2u32);

    let rogue = SigningKey::generate(&mut OsRng);
    let keys = [signers[0].clone(), rogue];
    let (assets, rounds) =
        sign_quorum_round(&env, &keys, &[ASSET_BTC], &[&[100], &[200]], BASE_TS, 1);
    let res = client.try_update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);
    assert_eq!(res, Err(Ok(Error::UnknownPublisher)));
}

#[test]
fn quorum_rejects_batch_length_mismatch() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 2);
    client.set_quorum(&2u32);

    let (assets, rounds) = sign_quorum_round(
        &env,
        &signers[0..2],
        &[ASSET_BTC],
        &[&[100], &[200]],
        BASE_TS,
        1,
    );
    // Strip one publisher's prices so it no longer aligns with assets.
    let mut bad = rounds.get_unchecked(1);
    bad.prices = Vec::new(&env);
    let mut rounds = rounds;
    rounds.set(1, bad);

    let res = client.try_update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);
    assert_eq!(res, Err(Ok(Error::BatchLengthMismatch)));
}

#[test]
fn quorum_rejects_stale_round() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 2);
    client.set_quorum(&2u32);
    // Round signed at BASE_TS, but the ledger has advanced 120s past it.
    env.ledger().set_timestamp(BASE_TS + 120);

    let (assets, rounds) = sign_quorum_round(
        &env,
        &signers[0..2],
        &[ASSET_BTC],
        &[&[100], &[200]],
        BASE_TS,
        1,
    );
    let res = client.try_update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);
    assert_eq!(res, Err(Ok(Error::StalePrice)));
}

#[test]
fn quorum_median_odd_takes_middle() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 3);
    client.set_quorum(&3u32);
    let asset = BytesN::from_array(&env, &ASSET_BTC);

    // Deliberately unsorted distinct prices: the middle by VALUE (200) must
    // win, not the middle by submission order.
    let (assets, rounds) = sign_quorum_round(
        &env,
        &signers[0..3],
        &[ASSET_BTC],
        &[&[300], &[100], &[200]],
        BASE_TS,
        1,
    );
    client.update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);

    assert_eq!(client.get_price_pers(&asset).unwrap().price, 200);
}

#[test]
fn quorum_median_even_takes_mean_of_middles() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 2);
    client.set_quorum(&2u32);
    let asset = BytesN::from_array(&env, &ASSET_BTC);

    // Unsorted even count: (150 + 250) / 2 = 200.
    let (assets, rounds) = sign_quorum_round(
        &env,
        &signers[0..2],
        &[ASSET_BTC],
        &[&[250], &[150]],
        BASE_TS,
        1,
    );
    client.update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);

    assert_eq!(client.get_price_pers(&asset).unwrap().price, 200);
}

#[test]
fn quorum_median_is_per_asset() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 3);
    client.set_quorum(&3u32);

    // Two assets in one submission: each takes its own median across the
    // three publishers, aligned by index with the shared assets vec.
    let (assets, rounds) = sign_quorum_round(
        &env,
        &signers[0..3],
        &[ASSET_BTC, ASSET_ETH],
        &[&[300, 30], &[100, 10], &[200, 20]],
        BASE_TS,
        1,
    );
    client.update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);

    let btc = client
        .get_price_pers(&BytesN::from_array(&env, &ASSET_BTC))
        .unwrap();
    let eth = client
        .get_price_pers(&BytesN::from_array(&env, &ASSET_ETH))
        .unwrap();
    assert_eq!(btc.price, 200);
    assert_eq!(eth.price, 20);
}

#[test]
fn quorum_one_bad_signature_reverts_everything() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 3);
    client.set_quorum(&2u32);
    let asset = BytesN::from_array(&env, &ASSET_BTC);

    let (assets, rounds) = sign_quorum_round(
        &env,
        &signers[0..3],
        &[ASSET_BTC],
        &[&[100], &[200], &[300]],
        BASE_TS,
        1,
    );
    // Tamper with the THIRD publisher's price after signing. Quorum (2)
    // would still be met by the two intact rounds, but every submitted
    // signature is verified — one bad signature reverts the whole call.
    let mut bad = rounds.get_unchecked(2);
    let mut tampered = Vec::new(&env);
    tampered.push_back(999_999i128);
    bad.prices = tampered;
    let mut rounds = rounds;
    rounds.set(2, bad);

    let res = client.try_update_quorum_ed25519_persistent(&assets, &BASE_TS, &1u64, &rounds);
    assert!(res.is_err());
    // Nothing landed: no price, no ring entry.
    assert!(client.get_price_pers(&asset).is_none());
    assert!(client.prices(&asset, &10u32).is_none());
}

#[test]
fn quorum_lagging_round_is_silent_noop_without_ring_entry() {
    let env = Env::default();
    let (client, signers) = setup_multi(&env, 2);
    client.set_quorum(&2u32);
    let asset = BytesN::from_array(&env, &ASSET_BTC);

    // Round 5 lands: median (100 + 200) / 2 = 150.
    let (a5, r5) = sign_quorum_round(
        &env,
        &signers[0..2],
        &[ASSET_BTC],
        &[&[100], &[200]],
        BASE_TS,
        5,
    );
    client.update_quorum_ed25519_persistent(&a5, &BASE_TS, &5u64, &r5);

    // A lagging round (3) and an equal round (5) must succeed as silent
    // no-ops: cache unchanged, no history entry.
    let (a3, r3) = sign_quorum_round(
        &env,
        &signers[0..2],
        &[ASSET_BTC],
        &[&[400], &[500]],
        BASE_TS,
        3,
    );
    client.update_quorum_ed25519_persistent(&a3, &BASE_TS, &3u64, &r3);
    let (a5b, r5b) = sign_quorum_round(
        &env,
        &signers[0..2],
        &[ASSET_BTC],
        &[&[600], &[700]],
        BASE_TS,
        5,
    );
    client.update_quorum_ed25519_persistent(&a5b, &BASE_TS, &5u64, &r5b);

    let stored = client.get_price_pers(&asset).unwrap();
    assert_eq!(stored.price, 150);
    assert_eq!(stored.round_id, 5);
    let hist = client.prices(&asset, &10u32).unwrap();
    assert_eq!(hist.len(), 1);

    // A strictly newer round advances the cache and pushes history.
    let (a6, r6) = sign_quorum_round(
        &env,
        &signers[0..2],
        &[ASSET_BTC],
        &[&[300], &[400]],
        BASE_TS,
        6,
    );
    client.update_quorum_ed25519_persistent(&a6, &BASE_TS, &6u64, &r6);
    let stored = client.get_price_pers(&asset).unwrap();
    assert_eq!(stored.price, 350);
    assert_eq!(stored.round_id, 6);
    assert_eq!(client.prices(&asset, &10u32).unwrap().len(), 2);
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
    let pubkey: BytesN<32> = BytesN::from_array(&env, &hex_bytes::<32>(PUBKEY));
    let admin = Address::generate(&env);
    let mut publishers = Vec::new(&env);
    publishers.push_back(pubkey.clone());
    let contract_id = env.register(OracleV0, (admin.clone(), publishers.clone()));
    let client = OracleV0Client::new(&env, &contract_id);

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
    let pubkey: BytesN<32> = BytesN::from_array(&env, &hex_bytes::<32>(PUBKEY));
    let admin = Address::generate(&env);
    let mut publishers = Vec::new(&env);
    publishers.push_back(pubkey.clone());
    let contract_id = env.register(OracleV0, (admin.clone(), publishers.clone()));
    let client = OracleV0Client::new(&env, &contract_id);

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
