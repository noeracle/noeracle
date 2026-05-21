#![no_std]

//! Noeracle benchmark contract: measures Soroban host-fn cost of the
//! signature primitives a pull-based oracle would have to verify on every
//! consume-fresh-price call.
//!
//! All entrypoints do the *minimum* work around the crypto call so the
//! budget delta is dominated by the host function itself.

use soroban_sdk::{
    contract, contractimpl,
    crypto::bls12_381::{Bls12381Fr, Bls12381G1Affine, Bls12381G2Affine},
    Bytes, BytesN, Env, Vec,
};

#[contract]
pub struct Bench;

#[contractimpl]
impl Bench {
    // ---------- Ed25519 ----------

    /// Verify N independent Ed25519 signatures over (possibly distinct) messages.
    pub fn ed25519_verify_n(
        env: Env,
        pubkeys: Vec<BytesN<32>>,
        messages: Vec<Bytes>,
        sigs: Vec<BytesN<64>>,
    ) -> u32 {
        let n = pubkeys.len();
        for i in 0..n {
            env.crypto().ed25519_verify(
                &pubkeys.get_unchecked(i),
                &messages.get_unchecked(i),
                &sigs.get_unchecked(i),
            );
        }
        n
    }

    // ---------- secp256k1 (ECDSA, recovery model — EVM / Bitcoin curve) ----------

    /// Verify N secp256k1 sigs by recover-and-compare.
    /// `messages` are raw msgs; contract sha256's them on-chain (publisher
    /// would sign sha256(msg) off-chain).
    pub fn secp256k1_recover_n(
        env: Env,
        expected_pubkeys: Vec<BytesN<65>>,
        messages: Vec<Bytes>,
        sigs: Vec<BytesN<64>>,
        recovery_ids: Vec<u32>,
    ) -> u32 {
        let n = expected_pubkeys.len();
        let crypto = env.crypto();
        for i in 0..n {
            let digest = crypto.sha256(&messages.get_unchecked(i));
            let recovered = crypto.secp256k1_recover(
                &digest,
                &sigs.get_unchecked(i),
                recovery_ids.get_unchecked(i),
            );
            if recovered != expected_pubkeys.get_unchecked(i) {
                panic!("secp256k1 mismatch");
            }
        }
        n
    }

    // ---------- secp256r1 (ECDSA P-256 — WebAuthn / passkey curve) ----------

    pub fn secp256r1_verify_n(
        env: Env,
        pubkeys: Vec<BytesN<65>>,
        messages: Vec<Bytes>,
        sigs: Vec<BytesN<64>>,
    ) -> u32 {
        let n = pubkeys.len();
        let crypto = env.crypto();
        for i in 0..n {
            let digest = crypto.sha256(&messages.get_unchecked(i));
            crypto.secp256r1_verify(
                &pubkeys.get_unchecked(i),
                &digest,
                &sigs.get_unchecked(i),
            );
        }
        n
    }

    /// Measure sha256 cost in isolation so we can subtract it from above.
    pub fn sha256_n(env: Env, messages: Vec<Bytes>) -> u32 {
        let n = messages.len();
        let crypto = env.crypto();
        for i in 0..n {
            let _ = crypto.sha256(&messages.get_unchecked(i));
        }
        n
    }

    // ---------- BLS12-381 ----------

    /// Single BLS sig verify form: e(sig, -G2_gen) * e(H(m), pubkey) == 1.
    /// Caller supplies precomputed inputs so we measure pairing_check alone.
    pub fn bls_pairing_check_2(
        env: Env,
        sig: Bls12381G1Affine,
        h_msg: Bls12381G1Affine,
        neg_g2_gen: Bls12381G2Affine,
        pubkey: Bls12381G2Affine,
    ) -> bool {
        let bls = env.crypto().bls12_381();
        let mut g1 = Vec::new(&env);
        g1.push_back(sig);
        g1.push_back(h_msg);
        let mut g2 = Vec::new(&env);
        g2.push_back(neg_g2_gen);
        g2.push_back(pubkey);
        bls.pairing_check(g1, g2)
    }

    /// Aggregate BLS verify over N pubkeys signing the SAME message.
    /// Cost = (N-1) G2 adds + 1 two-pair pairing_check.
    /// This is the structure a multi-publisher oracle would use for same-asset prices.
    pub fn bls_aggregate_same_msg(
        env: Env,
        agg_sig: Bls12381G1Affine,
        h_msg: Bls12381G1Affine,
        neg_g2_gen: Bls12381G2Affine,
        pubkeys: Vec<Bls12381G2Affine>,
    ) -> bool {
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
        bls.pairing_check(g1, g2)
    }

    /// Multi-pairing for N pubkeys signing DISTINCT messages.
    /// Cost dominated by N+1-input pairing_check.
    pub fn bls_multi_pairing(
        env: Env,
        g1: Vec<Bls12381G1Affine>,
        g2: Vec<Bls12381G2Affine>,
    ) -> bool {
        env.crypto().bls12_381().pairing_check(g1, g2)
    }

    // ---------- Building blocks (so we can size them individually) ----------

    pub fn bls_g2_add(env: Env, a: Bls12381G2Affine, b: Bls12381G2Affine) -> Bls12381G2Affine {
        env.crypto().bls12_381().g2_add(&a, &b)
    }

    pub fn bls_hash_to_g1(env: Env, msg: Bytes, dst: Bytes) -> Bls12381G1Affine {
        env.crypto().bls12_381().hash_to_g1(&msg, &dst)
    }

    pub fn bls_hash_to_g2(env: Env, msg: Bytes, dst: Bytes) -> Bls12381G2Affine {
        env.crypto().bls12_381().hash_to_g2(&msg, &dst)
    }

    pub fn bls_g1_mul(env: Env, p: Bls12381G1Affine, s: Bls12381Fr) -> Bls12381G1Affine {
        env.crypto().bls12_381().g1_mul(&p, &s)
    }

    pub fn bls_g2_mul(env: Env, p: Bls12381G2Affine, s: Bls12381Fr) -> Bls12381G2Affine {
        env.crypto().bls12_381().g2_mul(&p, &s)
    }
}

#[cfg(test)]
mod test;
