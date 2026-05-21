export type Network = "testnet" | "mainnet";

export interface NoeracleConfig {
  /** Stellar network the oracle contract is deployed on. Default: "testnet". */
  network?: Network;
  /** Attestation service base URL. Default: the public Noeracle service. */
  attestationUrl?: string;
  /** Default oracle contract id, used when `toUpdateOp` is called with none. */
  contractId?: string;
}

/** A signed price attestation, exactly as served by the attestation service. */
export interface Attestation {
  asset: string;
  tag: string;
  /** Integer price scaled by 1e7, as a decimal string. */
  price: string;
  price_human: number;
  /** Unix seconds the round was signed. */
  timestamp: number;
  round_id: number;
  /** Number of exchange sources behind the median. */
  sources: number;
  /** Publisher Ed25519 public key, 32-byte hex. */
  publisher: string;
  /** The signed 40-byte message, hex. */
  message: string;
  /** Ed25519 signature over `message`, 64-byte hex. */
  signature: string;
}

/** A decoded, consumer-friendly view of one price. */
export interface PriceEntry {
  asset: string;
  /** Integer price scaled by 1e7 (Stellar precision). */
  price: bigint;
  /** Human-readable price. */
  priceHuman: number;
  /** Unix seconds the price was signed. */
  timestamp: number;
  roundId: number;
  /** Number of exchange sources behind the median. */
  sources: number;
}
