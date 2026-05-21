// Per-asset price aggregation: drop stale samples, drop outliers, take the
// median of what remains.

export function median(nums) {
  const s = [...nums].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 === 1 ? s[m] : (s[m - 1] + s[m]) / 2;
}

// samples: [{ exchange, price, ts }]
// Returns { price, sources } or null if no usable sample remains.
export function aggregate(samples, { stalenessMs, outlierStddev, now }) {
  const fresh = samples.filter((s) => now - s.ts <= stalenessMs);
  if (fresh.length === 0) return null;

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
  return { price: median(kept.map((s) => s.price)), sources: kept.length };
}
