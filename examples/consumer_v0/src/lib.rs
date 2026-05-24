#![no_std]

//! Noeracle consumer_v0 — minimal Soroban consumer demonstrating the inline
//! oracle-verification pattern.
//!
//! `open_position` takes the six `update_batch_ed25519_args` arguments
//! alongside the position arguments and calls the Noeracle oracle inside
//! its own body. Signature verification, staleness, and round monotonicity
//! are enforced by the oracle inside the same transaction — if any of those
//! fail, the consumer's transaction reverts as a whole.
//!
//! Fork this contract as the starting point for a perpetuals DEX, lending
//! market, oracle-priced AMM, or any consumer that needs sub-second
//! execution-time freshness.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype,
    Address, BytesN, Env, IntoVal, Symbol, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NoAssets = 3,
    BatchLengthMismatch = 4,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Oracle,
    Position(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub trader: Address,
    pub asset: BytesN<8>,
    pub size: i128,
    pub entry_price: i128,
    pub entry_round: u64,
    pub entry_timestamp: u64,
}

// Persistent storage TTL — positions outlive temporary price entries.
const POS_THRESHOLD: u32 = 60_480; // ~3.5 days
const POS_EXTEND: u32 = 120_960; // ~7 days

#[contract]
pub struct ConsumerV0;

#[contractimpl]
impl ConsumerV0 {
    /// One-time setup. Records the Noeracle oracle contract address that
    /// this consumer will verify prices against.
    pub fn init(env: Env, oracle: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Oracle) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Oracle, &oracle);
        Ok(())
    }

    /// Opens a position priced against the just-verified oracle price.
    ///
    /// The six `update_batch_ed25519_args` arguments (`assets`, `prices`,
    /// `timestamp`, `round_id`, `pubkey`, `sigs`) are forwarded directly to
    /// the Noeracle oracle. If signature verification, the staleness window,
    /// or the registered-publisher check fails, the cross-contract call
    /// aborts and this whole transaction reverts — no halfway state.
    ///
    /// Only the first asset/price pair is used as the entry price. Richer
    /// integrations could quote multiple legs of a single trade in one call.
    pub fn open_position(
        env: Env,
        trader: Address,
        size: i128,
        assets: Vec<BytesN<8>>,
        prices: Vec<i128>,
        timestamp: u64,
        round_id: u64,
        pubkey: BytesN<32>,
        sigs: Vec<BytesN<64>>,
    ) -> Result<Position, Error> {
        trader.require_auth();

        let n = assets.len();
        if n == 0 {
            return Err(Error::NoAssets);
        }
        if prices.len() != n || sigs.len() != n {
            return Err(Error::BatchLengthMismatch);
        }

        let oracle: Address = env
            .storage()
            .instance()
            .get(&DataKey::Oracle)
            .ok_or(Error::NotInitialized)?;

        env.invoke_contract::<()>(
            &oracle,
            &Symbol::new(&env, "update_batch_ed25519_args"),
            (
                assets.clone(),
                prices.clone(),
                timestamp,
                round_id,
                pubkey,
                sigs,
            )
                .into_val(&env),
        );

        let entry_asset = assets.get_unchecked(0);
        let entry_price = prices.get_unchecked(0);

        let position = Position {
            trader: trader.clone(),
            asset: entry_asset,
            size,
            entry_price,
            entry_round: round_id,
            entry_timestamp: timestamp,
        };

        let key = DataKey::Position(trader);
        env.storage().persistent().set(&key, &position);
        env.storage()
            .persistent()
            .extend_ttl(&key, POS_THRESHOLD, POS_EXTEND);

        Ok(position)
    }

    /// Read a trader's stored position, or `None` if none.
    pub fn get_position(env: Env, trader: Address) -> Option<Position> {
        env.storage().persistent().get(&DataKey::Position(trader))
    }
}
