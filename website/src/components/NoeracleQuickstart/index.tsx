import { useCallback, useEffect, useRef, useState } from 'react';
import BrowserOnly from '@docusaurus/BrowserOnly';
import styles from './styles.module.css';

const ENDPOINT = 'https://api.noeracle.org';
const ASSET = 'BTC/USD';

type Attestation = {
  asset: string;
  tag: string;
  price: string;
  price_human: number;
  timestamp: number;
  round_id: number;
  sources: number;
  publisher: string;
  message: string;
  signature: string;
};

function formatUsd(n: number): string {
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(n);
}

function formatUtc(unixS: number): string {
  return new Date(unixS * 1000).toISOString().replace('T', ' ').replace(/\.\d+Z$/, ' UTC');
}

function truncMid(hex: string, head = 6, tail = 4): string {
  if (hex.length <= head + tail + 2) return hex;
  return `${hex.slice(0, head)}…${hex.slice(-tail)}`;
}

function ageMs(timestamp: number, nowMs: number): number {
  return Math.max(0, nowMs - timestamp * 1000);
}

function Inner() {
  const [att, setAtt] = useState<Attestation | null>(null);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState<string | null>(null);
  const [live, setLive] = useState(false);
  const [trend, setTrend] = useState<'up' | 'down' | 'flat'>('flat');
  const [now, setNow] = useState(() => Date.now());
  const prevPriceRef = useRef<number | null>(null);
  const evtSourceRef = useRef<EventSource | null>(null);

  const fetchOnce = useCallback(async () => {
    setLoading(true);
    setErr(null);
    try {
      const res = await fetch(`${ENDPOINT}/v1/latest`, { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const body = (await res.json()) as { assets?: Record<string, Attestation> };
      const next = body.assets?.[ASSET];
      if (!next) throw new Error(`asset ${ASSET} not in snapshot`);
      updatePrice(next);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  const updatePrice = useCallback((next: Attestation) => {
    setAtt((prev) => {
      const prevPrice = prevPriceRef.current;
      if (prevPrice !== null && next.price_human !== prevPrice) {
        setTrend(next.price_human > prevPrice ? 'up' : 'down');
        // Decay back to flat after 1.4s so the highlight feels alive but not loud
        window.setTimeout(() => setTrend('flat'), 1400);
      }
      prevPriceRef.current = next.price_human;
      return next;
    });
  }, []);

  // Initial fetch.
  useEffect(() => {
    fetchOnce();
  }, [fetchOnce]);

  // Live age tick (every 250 ms while live, every 1s otherwise)
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), live ? 250 : 1000);
    return () => window.clearInterval(id);
  }, [live]);

  // SSE subscription
  useEffect(() => {
    if (!live) {
      evtSourceRef.current?.close();
      evtSourceRef.current = null;
      return;
    }
    const es = new EventSource(`${ENDPOINT}/v1/stream`);
    evtSourceRef.current = es;
    es.addEventListener('prices', (e) => {
      try {
        const parsed = JSON.parse((e as MessageEvent).data) as {
          assets?: Record<string, Attestation>;
        };
        const next = parsed.assets?.[ASSET];
        if (next) {
          updatePrice(next);
          setErr(null);
        }
      } catch {
        // ignore malformed
      }
    });
    es.onerror = () => {
      setErr('stream interrupted — auto-reconnecting…');
    };
    return () => {
      es.close();
      evtSourceRef.current = null;
    };
  }, [live, updatePrice]);

  const ageMsValue = att ? ageMs(att.timestamp, now) : 0;

  return (
    <div className={styles.card}>
      <div className={styles.topRow}>
        <span className={styles.pair}>{ASSET}</span>
        <span className={`${styles.live} ${live ? styles.liveOn : ''}`}>
          <span className={styles.liveDot} />
          {live ? 'LIVE' : att ? `ROUND ${att.round_id}` : '—'}
        </span>
      </div>

      <div className={`${styles.priceWrap} ${styles[`trend_${trend}`]}`}>
        {loading && !att ? (
          <span className={styles.placeholder}>…</span>
        ) : err && !att ? (
          <span className={styles.errInline}>error</span>
        ) : att ? (
          <span className={styles.price}>{formatUsd(att.price_human)}</span>
        ) : (
          <span className={styles.placeholder}>—</span>
        )}
      </div>

      <div className={styles.meta}>
        <span className={styles.metaGroup}>
          <span className={styles.metaLabel}>signed</span>
          <span className={styles.metaValue}>
            {att ? formatUtc(att.timestamp) : '—'}
          </span>
        </span>
        <span className={styles.metaGroup}>
          <span className={styles.metaLabel}>age</span>
          <span
            className={`${styles.metaValue} ${
              ageMsValue > 2000 ? styles.metaStale : ''
            }`}
          >
            {att ? `${ageMsValue.toFixed(0)} ms` : '—'}
          </span>
        </span>
        <span className={styles.metaGroup}>
          <span className={styles.metaLabel}>sources</span>
          <span className={styles.metaValue}>{att?.sources ?? '—'}</span>
        </span>
        <span className={styles.metaGroup}>
          <span className={styles.metaLabel}>round</span>
          <span className={styles.metaValue}>{att?.round_id ?? '—'}</span>
        </span>
      </div>

      <div className={styles.sigRow}>
        <span className={styles.sigGroup}>
          <span className={styles.metaLabel}>publisher</span>
          <code className={styles.sig}>
            {att ? truncMid(att.publisher, 8, 6) : '—'}
          </code>
        </span>
        <span className={styles.sigGroup}>
          <span className={styles.metaLabel}>signature</span>
          <code className={styles.sig}>
            {att ? truncMid(att.signature, 8, 6) : '—'}
          </code>
        </span>
      </div>

      {err && att && <div className={styles.errBanner}>{err}</div>}

      <div className={styles.actions}>
        <button
          className={styles.btn}
          onClick={fetchOnce}
          disabled={loading || live}
          aria-label="Fetch a fresh price now"
        >
          {loading ? 'fetching…' : 'fetch latest'}
        </button>
        <button
          className={`${styles.btn} ${live ? styles.btnActive : ''}`}
          onClick={() => setLive((v) => !v)}
          aria-pressed={live}
          aria-label="Toggle live SSE subscription"
        >
          {live ? 'stop stream' : 'start stream'}
        </button>
        <a
          className={styles.link}
          href={`${ENDPOINT}/v1/latest`}
          target="_blank"
          rel="noreferrer"
        >
          /v1/latest ↗
        </a>
      </div>
    </div>
  );
}

export function NoeracleQuickstart() {
  return (
    <BrowserOnly
      fallback={
        <div className={styles.card}>
          <div className={styles.topRow}>
            <span className={styles.pair}>{ASSET}</span>
            <span className={styles.live}>
              <span className={styles.liveDot} />
              —
            </span>
          </div>
          <div className={styles.priceWrap}>
            <span className={styles.placeholder}>…</span>
          </div>
        </div>
      }
    >
      {() => <Inner />}
    </BrowserOnly>
  );
}

export default NoeracleQuickstart;
