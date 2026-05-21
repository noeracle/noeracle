// Per-asset price aggregation: drop stale samples, drop outliers, then take
// an exchange-weighted average of what remains.

// Weighted average of [{ price, weight }]. Returns null if total weight is 0.
export function weightedAverage(samples) {
  const totalWeight = samples.reduce((sum, s) => sum + s.weight, 0);
  if (totalWeight === 0) return null;
  const sum = samples.reduce((acc, s) => acc + s.price * s.weight, 0);
  return sum / totalWeight;
}

// samples: [{ exchange, price, ts }]
// Returns { price, sources } or null if no usable sample remains.
export function aggregate(samples, { stalenessMs, outlierStddev, weights, now }) {
  const fresh = samples.filter((s) => now - s.ts <= stalenessMs);
  if (fresh.length === 0) return null;

  // Reject gross outliers (manipulation, fat-finger prints) before averaging,
  // using an unweighted mean/stddev only to decide membership.
  let kept = fresh;
  if (fresh.length >= 3) {
    const prices = fresh.map((s) => s.price);
    const mean = prices.reduce((a, b) => a + b, 0) / prices.length;
    const variance =
      prices.reduce((a, b) => a + (b - mean) ** 2, 0) / prices.length;
    const stddev = Math.sqrt(variance);
    if (stddev > 0) {
      kept = fresh.filter(
        (s) => Math.abs(s.price - mean) <= outlierStddev * stddev,
      );
    }
  }
  if (kept.length === 0) return null;

  // Exchange-weighted average of the survivors.
  const price = weightedAverage(
    kept.map((s) => ({ price: s.price, weight: weights[s.exchange] ?? 1 })),
  );
  if (price === null) return null;
  return { price, sources: kept.length };
}
