/** Base class for every error thrown by the Noeracle SDK. */
export class NoeracleError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NoeracleError";
  }
}

/** The attestation service was unreachable or returned an error status. */
export class AttestationServiceError extends NoeracleError {
  constructor(message: string) {
    super(message);
    this.name = "AttestationServiceError";
  }
}

/** A requested asset has no current attestation. */
export class AssetUnavailableError extends NoeracleError {
  constructor(public readonly asset: string) {
    super(`no current attestation for asset: ${asset}`);
    this.name = "AssetUnavailableError";
  }
}

/** Fetched attestations span multiple rounds and cannot share one update op. */
export class InconsistentRoundError extends NoeracleError {
  constructor() {
    super(
      "fetched attestations span multiple rounds; fetch fewer assets, or " +
        "build one update op per round",
    );
    this.name = "InconsistentRoundError";
  }
}
