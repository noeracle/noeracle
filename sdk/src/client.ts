import { Contract, nativeToScVal, xdr } from "@stellar/stellar-sdk";
import type {
  Attestation,
  Network,
  NoeracleConfig,
  PriceEntry,
  Subscription,
} from "./types.js";
import {
  AssetUnavailableError,
  AttestationServiceError,
  InconsistentRoundError,
  NoeracleError,
} from "./errors.js";
import { streamSse } from "./stream.js";

const DEFAULT_ATTESTATION_URL = "https://noeracle.fly.dev";

/** Fresh signed prices, ready to bundle into a Stellar transaction. */
export class Fresh {
  constructor(
    /** The raw signed attestations, one per requested asset, in order. */
    readonly attestations: Attestation[],
    private readonly defaultContractId?: string,
  ) {}

  /** Decoded, consumer-friendly price views. */
  get prices(): PriceEntry[] {
    return this.attestations.map((a) => ({
      asset: a.asset,
      price: BigInt(a.price),
      priceHuman: a.price_human,
      timestamp: a.timestamp,
      roundId: a.round_id,
      sources: a.sources,
    }));
  }

  /** The decoded price for one asset; throws if it was not fetched. */
  price(asset: string): PriceEntry {
    const entry = this.prices.find((p) => p.asset === asset);
    if (!entry) throw new AssetUnavailableError(asset);
    return entry;
  }

  /**
   * Build the `update_batch_ed25519_args` operation to prepend to your
   * transaction. It verifies the publisher signature(s) on chain and writes
   * the price(s). All fetched attestations must share one round.
   */
  toUpdateOp(contractId?: string): xdr.Operation {
    const id = contractId ?? this.defaultContractId;
    if (!id) throw new NoeracleError("toUpdateOp: a contract id is required");
    if (this.attestations.length === 0) {
      throw new NoeracleError("toUpdateOp: no attestations to submit");
    }

    const first = this.attestations[0]!;
    const sameRound = this.attestations.every(
      (a) => a.timestamp === first.timestamp && a.round_id === first.round_id,
    );
    if (!sameRound) throw new InconsistentRoundError();

    const hex = (s: string): Buffer => Buffer.from(s, "hex");
    const bytes = (b: Buffer): xdr.ScVal => xdr.ScVal.scvBytes(b);

    // The on-chain asset tag is the first 8 bytes of the signed message.
    const assets = xdr.ScVal.scvVec(
      this.attestations.map((a) => bytes(hex(a.message).subarray(0, 8))),
    );
    const prices = xdr.ScVal.scvVec(
      this.attestations.map((a) =>
        nativeToScVal(BigInt(a.price), { type: "i128" }),
      ),
    );
    const sigs = xdr.ScVal.scvVec(
      this.attestations.map((a) => bytes(hex(a.signature))),
    );
    const timestamp = nativeToScVal(BigInt(first.timestamp), { type: "u64" });
    const roundId = nativeToScVal(BigInt(first.round_id), { type: "u64" });
    const pubkey = bytes(hex(first.publisher));

    return new Contract(id).call(
      "update_batch_ed25519_args",
      assets,
      prices,
      timestamp,
      roundId,
      pubkey,
      sigs,
    );
  }
}

/** Client for the Noeracle pull-based price oracle. */
export class Noeracle {
  readonly network: Network;
  private readonly attestationUrl: string;
  private readonly contractId?: string;

  constructor(config: NoeracleConfig = {}) {
    this.network = config.network ?? "testnet";
    this.attestationUrl = (
      config.attestationUrl ?? DEFAULT_ATTESTATION_URL
    ).replace(/\/+$/, "");
    this.contractId = config.contractId;
  }

  /**
   * Fetch the latest signed price attestation for each asset. All assets are
   * read from a single consistent snapshot of the attestation service, so a
   * multi-asset result shares one round.
   */
  async fetchLatest(assets: string[]): Promise<Fresh> {
    if (assets.length === 0) {
      throw new NoeracleError("fetchLatest: no assets requested");
    }

    let snapshot: Record<string, Attestation>;
    try {
      const res = await fetch(`${this.attestationUrl}/v1/latest`);
      if (!res.ok) {
        throw new AttestationServiceError(
          `attestation service returned HTTP ${res.status}`,
        );
      }
      const body = (await res.json()) as {
        assets: Record<string, Attestation>;
      };
      snapshot = body.assets ?? {};
    } catch (err) {
      if (err instanceof NoeracleError) throw err;
      throw new AttestationServiceError(
        `attestation service unreachable: ${(err as Error).message}`,
      );
    }

    const picked = assets.map((asset) => {
      const att = snapshot[asset];
      if (!att) throw new AssetUnavailableError(asset);
      return att;
    });
    return new Fresh(picked, this.contractId);
  }

  /**
   * Subscribe to a live stream of fresh prices. `onUpdate` fires with a Fresh
   * for the requested assets each time the service signs a new round. The
   * connection reconnects automatically; call `.close()` on the returned
   * handle to stop.
   */
  subscribe(
    assets: string[],
    onUpdate: (fresh: Fresh) => void,
    onError?: (err: NoeracleError) => void,
  ): Subscription {
    if (assets.length === 0) {
      throw new NoeracleError("subscribe: no assets requested");
    }
    const controller = new AbortController();
    streamSse(
      `${this.attestationUrl}/v1/stream`,
      {
        onEvent: (event, data) => {
          if (event !== "prices") return;
          let snapshot: Record<string, Attestation>;
          try {
            const parsed = JSON.parse(data) as {
              assets?: Record<string, Attestation>;
            };
            snapshot = parsed.assets ?? {};
          } catch {
            onError?.(
              new AttestationServiceError("stream sent malformed data"),
            );
            return;
          }
          const picked = assets
            .map((a) => snapshot[a])
            .filter((a): a is Attestation => a !== undefined);
          if (picked.length > 0) {
            onUpdate(new Fresh(picked, this.contractId));
          }
        },
        onError: (err) => {
          onError?.(
            new AttestationServiceError(`stream error: ${err.message}`),
          );
        },
      },
      controller.signal,
    );
    return { close: () => controller.abort() };
  }
}
