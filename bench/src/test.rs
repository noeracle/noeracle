extern crate std;

use crate::{Bench, BenchClient};
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use soroban_sdk::{
    crypto::bls12_381::{Bls12381G1Affine, Bls12381G2Affine},
    Bytes, BytesN, Env, Vec,
};
use std::vec::Vec as StdVec;

const BLS_DST_G1: &[u8] = b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_";
const BLS_DST_G2: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

fn register(env: &Env) -> BenchClient<'_> {
    let id = env.register(Bench, ());
    BenchClient::new(env, &id)
}

fn print_cost(label: &str, env: &Env) {
    let b = env.cost_estimate().budget();
    std::println!(
        "{:<48} cpu={:>12}   mem={:>10}",
        label,
        b.cpu_instruction_cost(),
        b.memory_bytes_cost()
    );
}

// =============================================================================
//  Ed25519
// =============================================================================

fn gen_ed25519_signed(env: &Env, n: usize) -> (Vec<BytesN<32>>, Vec<Bytes>, Vec<BytesN<64>>) {
    let mut pks = Vec::new(env);
    let mut msgs = Vec::new(env);
    let mut sigs = Vec::new(env);
    let mut csprng = OsRng;
    for i in 0..n {
        let sk = SigningKey::generate(&mut csprng);
        let msg_str = std::format!("price:BTC:65432.10:1717254000:nonce={}", i);
        let msg_bytes_std: StdVec<u8> = msg_str.into_bytes();
        let sig = sk.sign(&msg_bytes_std);

        pks.push_back(BytesN::from_array(env, &sk.verifying_key().to_bytes()));
        msgs.push_back(Bytes::from_slice(env, &msg_bytes_std));
        sigs.push_back(BytesN::from_array(env, &sig.to_bytes()));
    }
    (pks, msgs, sigs)
}

fn bench_ed25519(label: &str, env: &Env, client: &BenchClient<'_>, n: usize) {
    let (pks, msgs, sigs) = gen_ed25519_signed(env, n);
    env.cost_estimate().budget().reset_default();
    let returned = client.ed25519_verify_n(&pks, &msgs, &sigs);
    assert_eq!(returned, n as u32);
    print_cost(label, env);
}

#[test]
fn ed25519_table() {
    let env = Env::default();
    let client = register(&env);
    std::println!("\n========== Ed25519 verify cost ==========");
    bench_ed25519("ed25519 verify  x1", &env, &client, 1);
    bench_ed25519("ed25519 verify  x3", &env, &client, 3);
    bench_ed25519("ed25519 verify  x5", &env, &client, 5);
    bench_ed25519("ed25519 verify  x7", &env, &client, 7);
    bench_ed25519("ed25519 verify x10", &env, &client, 10);
}

// =============================================================================
//  secp256k1  (ECDSA, recovery — EVM / Bitcoin curve)
// =============================================================================

fn gen_secp256k1_signed(
    env: &Env,
    n: usize,
) -> (Vec<BytesN<65>>, Vec<Bytes>, Vec<BytesN<64>>, Vec<u32>) {
    use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};
    use sha2::{Digest, Sha256};

    let mut pks = Vec::new(env);
    let mut msgs = Vec::new(env);
    let mut sigs = Vec::new(env);
    let mut recids = Vec::new(env);
    let mut csprng = rand::rngs::OsRng;

    for i in 0..n {
        let sk = SigningKey::random(&mut csprng);
        let vk = sk.verifying_key();
        let pk_sec1 = vk.to_encoded_point(false).as_bytes().to_vec(); // 65 bytes, uncompressed
        let mut pk_arr = [0u8; 65];
        pk_arr.copy_from_slice(&pk_sec1);

        let msg_str = std::format!("price:BTC:65432.10:1717254000:nonce={}", i);
        let msg = msg_str.into_bytes();
        let digest = Sha256::digest(&msg);

        // sign the prehash, get recovery id
        let (sig, recid): (Signature, RecoveryId) = sk.sign_prehash(&digest).unwrap();
        let sig_bytes = sig.to_bytes();
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);

        pks.push_back(BytesN::from_array(env, &pk_arr));
        msgs.push_back(Bytes::from_slice(env, &msg));
        sigs.push_back(BytesN::from_array(env, &sig_arr));
        recids.push_back(recid.to_byte() as u32);
    }
    (pks, msgs, sigs, recids)
}

fn bench_secp256k1(label: &str, env: &Env, client: &BenchClient<'_>, n: usize) {
    let (pks, msgs, sigs, recids) = gen_secp256k1_signed(env, n);
    env.cost_estimate().budget().reset_default();
    let returned = client.secp256k1_recover_n(&pks, &msgs, &sigs, &recids);
    assert_eq!(returned, n as u32);
    print_cost(label, env);
}

#[test]
fn secp256k1_table() {
    let env = Env::default();
    let client = register(&env);
    std::println!("\n========== secp256k1 recover+compare cost (incl sha256) ==========");
    bench_secp256k1("secp256k1 recover  x1", &env, &client, 1);
    bench_secp256k1("secp256k1 recover  x3", &env, &client, 3);
    bench_secp256k1("secp256k1 recover  x5", &env, &client, 5);
    bench_secp256k1("secp256k1 recover  x7", &env, &client, 7);
    bench_secp256k1("secp256k1 recover x10", &env, &client, 10);
}

// =============================================================================
//  secp256r1  (P-256 — WebAuthn / passkey curve)
// =============================================================================

fn gen_secp256r1_signed(env: &Env, n: usize) -> (Vec<BytesN<65>>, Vec<Bytes>, Vec<BytesN<64>>) {
    use p256::ecdsa::{signature::hazmat::PrehashSigner, Signature, SigningKey};
    use sha2::{Digest, Sha256};

    let mut pks = Vec::new(env);
    let mut msgs = Vec::new(env);
    let mut sigs = Vec::new(env);
    let mut csprng = rand::rngs::OsRng;

    for i in 0..n {
        let sk = SigningKey::random(&mut csprng);
        let vk = sk.verifying_key();
        let pk_sec1 = vk.to_encoded_point(false).as_bytes().to_vec(); // 65 bytes uncompressed
        let mut pk_arr = [0u8; 65];
        pk_arr.copy_from_slice(&pk_sec1);

        let msg_str = std::format!("price:BTC:65432.10:1717254000:nonce={}", i);
        let msg = msg_str.into_bytes();
        let digest = Sha256::digest(&msg);
        let raw_sig: Signature = sk.sign_prehash(&digest).unwrap();
        // Soroban enforces low-S malleability protection; normalize to low-S.
        let sig = raw_sig.normalize_s().unwrap_or(raw_sig);
        let sig_bytes = sig.to_bytes();
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);

        pks.push_back(BytesN::from_array(env, &pk_arr));
        msgs.push_back(Bytes::from_slice(env, &msg));
        sigs.push_back(BytesN::from_array(env, &sig_arr));
    }
    (pks, msgs, sigs)
}

fn bench_secp256r1(label: &str, env: &Env, client: &BenchClient<'_>, n: usize) {
    let (pks, msgs, sigs) = gen_secp256r1_signed(env, n);
    env.cost_estimate().budget().reset_default();
    let returned = client.secp256r1_verify_n(&pks, &msgs, &sigs);
    assert_eq!(returned, n as u32);
    print_cost(label, env);
}

#[test]
fn secp256r1_table() {
    let env = Env::default();
    let client = register(&env);
    std::println!("\n========== secp256r1 verify cost (incl sha256) ==========");
    bench_secp256r1("secp256r1 verify  x1", &env, &client, 1);
    bench_secp256r1("secp256r1 verify  x3", &env, &client, 3);
    bench_secp256r1("secp256r1 verify  x5", &env, &client, 5);
    bench_secp256r1("secp256r1 verify  x7", &env, &client, 7);
    bench_secp256r1("secp256r1 verify x10", &env, &client, 10);
}

// =============================================================================
//  sha256 baseline (so we can subtract it from secp256k1/r1 results)
// =============================================================================

#[test]
fn sha256_baseline() {
    let env = Env::default();
    let client = register(&env);
    std::println!("\n========== sha256 baseline (subtract from secp256k1/r1) ==========");
    for &n in &[1usize, 3, 5, 7, 10] {
        let mut msgs = Vec::new(&env);
        for i in 0..n {
            let m = std::format!("price:BTC:65432.10:1717254000:nonce={}", i);
            msgs.push_back(Bytes::from_slice(&env, m.as_bytes()));
        }
        env.cost_estimate().budget().reset_default();
        let _ = client.sha256_n(&msgs);
        print_cost(&std::format!("sha256 x{:>2}", n), &env);
    }
}

// =============================================================================
//  BLS12-381
// =============================================================================
//
//  We only need *valid* G1 / G2 points to make pairing_check execute the full
//  work (it short-circuits on subgroup/curve membership failures). We get them
//  cheaply via hash_to_g1 / hash_to_g2 — the host functions return points that
//  are guaranteed to be in the correct subgroup and on the curve.
//
//  Whether the pairing identity actually holds (i.e. the "signature" verifies)
//  does not affect the cost we are measuring — `pairing_check` always executes
//  the full Miller loop + final exponentiation.

fn g1_point(env: &Env, client: &BenchClient<'_>, tag: &str) -> Bls12381G1Affine {
    let msg = Bytes::from_slice(env, tag.as_bytes());
    let dst = Bytes::from_slice(env, BLS_DST_G1);
    client.bls_hash_to_g1(&msg, &dst)
}

fn g2_point(env: &Env, client: &BenchClient<'_>, tag: &str) -> Bls12381G2Affine {
    let msg = Bytes::from_slice(env, tag.as_bytes());
    let dst = Bytes::from_slice(env, BLS_DST_G2);
    client.bls_hash_to_g2(&msg, &dst)
}

#[test]
fn bls_building_blocks() {
    let env = Env::default();
    let client = register(&env);
    std::println!("\n========== BLS12-381 building blocks ==========");

    let a = g2_point(&env, &client, "publisher-A");
    let b = g2_point(&env, &client, "publisher-B");
    env.cost_estimate().budget().reset_default();
    let _ = client.bls_g2_add(&a, &b);
    print_cost("bls_g2_add (1 addition)", &env);

    let msg = Bytes::from_slice(&env, b"price:BTC:65432.10:1717254000");
    let dst = Bytes::from_slice(&env, BLS_DST_G1);
    env.cost_estimate().budget().reset_default();
    let _ = client.bls_hash_to_g1(&msg, &dst);
    print_cost("bls_hash_to_g1", &env);

    env.cost_estimate().budget().reset_default();
    let _ = client.bls_hash_to_g2(&msg, &dst);
    print_cost("bls_hash_to_g2", &env);
}

#[test]
fn bls_pairing_2input() {
    let env = Env::default();
    let client = register(&env);
    std::println!("\n========== BLS12-381 pairing_check (single sig verify shape) ==========");

    let sig = g1_point(&env, &client, "sig");
    let h_msg = g1_point(&env, &client, "msg");
    let neg_g2_gen = g2_point(&env, &client, "neg-g2-gen");
    let pubkey = g2_point(&env, &client, "publisher-A-pubkey");

    env.cost_estimate().budget().reset_default();
    let _ = client.bls_pairing_check_2(&sig, &h_msg, &neg_g2_gen, &pubkey);
    print_cost("pairing_check (2 G1, 2 G2)", &env);
}

fn bench_aggregate_same_msg(env: &Env, client: &BenchClient<'_>, n: usize) {
    let agg_sig = g1_point(env, client, "agg-sig");
    let h_msg = g1_point(env, client, "shared-msg");
    let neg_g2_gen = g2_point(env, client, "neg-g2-gen");

    let mut pks = Vec::new(env);
    for i in 0..n {
        pks.push_back(g2_point(env, client, &std::format!("publisher-{}", i)));
    }

    env.cost_estimate().budget().reset_default();
    let _ = client.bls_aggregate_same_msg(&agg_sig, &h_msg, &neg_g2_gen, &pks);
    print_cost(
        &std::format!("aggregate_same_msg (N={:>2})", n),
        env,
    );
}

#[test]
fn bls_aggregate_same_msg_table() {
    let env = Env::default();
    let client = register(&env);
    std::println!("\n========== BLS aggregate-same-msg (N-1 G2 adds + 1 pairing_check) ==========");
    bench_aggregate_same_msg(&env, &client, 1);
    bench_aggregate_same_msg(&env, &client, 3);
    bench_aggregate_same_msg(&env, &client, 5);
    bench_aggregate_same_msg(&env, &client, 7);
    bench_aggregate_same_msg(&env, &client, 10);
}

fn bench_multi_pairing(env: &Env, client: &BenchClient<'_>, n: usize) {
    let mut g1 = Vec::new(env);
    let mut g2 = Vec::new(env);
    for i in 0..n {
        g1.push_back(g1_point(env, client, &std::format!("g1-{}", i)));
        g2.push_back(g2_point(env, client, &std::format!("g2-{}", i)));
    }
    env.cost_estimate().budget().reset_default();
    let _ = client.bls_multi_pairing(&g1, &g2);
    print_cost(&std::format!("multi_pairing (N={:>2} pairs)", n), env);
}

#[test]
fn bls_multi_pairing_table() {
    let env = Env::default();
    let client = register(&env);
    std::println!("\n========== BLS multi-pairing (distinct-msg aggregation) ==========");
    bench_multi_pairing(&env, &client, 2);
    bench_multi_pairing(&env, &client, 3);
    bench_multi_pairing(&env, &client, 5);
    bench_multi_pairing(&env, &client, 7);
    bench_multi_pairing(&env, &client, 10);
}
