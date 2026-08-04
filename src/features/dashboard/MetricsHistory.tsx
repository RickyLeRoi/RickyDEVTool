import { useEffect, useMemo, useState } from "react";
import { api } from "../../lib/api";
import type { MetricSample } from "../../lib/types";

const RANGES = [
  { hours: 1, label: "1h" },
  { hours: 6, label: "6h" },
  { hours: 24, label: "24h" },
];

const SERIES = [
  { key: "cpuPct", label: "CPU", color: "var(--accent)" },
  { key: "memPct", label: "RAM", color: "var(--accent2)" },
  { key: "diskPct", label: "Disco", color: "var(--ok)" },
] as const;

type SeriesKey = (typeof SERIES)[number]["key"];

const HIDDEN_KEY = "rdt-metrics-hidden";

function loadHidden(): SeriesKey[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? "[]");
    const keys = SERIES.map((s) => s.key) as readonly string[];
    return Array.isArray(parsed) ? parsed.filter((k) => keys.includes(k)) : [];
  } catch {
    return [];
  }
}

const W = 560;
const H = 160;
const PAD_L = 6;
const PAD_R = 10;
const PAD_T = 8;
const PAD_B = 6;

function yFor(pct: number) {
  return PAD_T + (1 - pct / 100) * (H - PAD_T - PAD_B);
}
function topPct(pct: number) {
  return (yFor(pct) / H) * 100;
}
const MAX_POINTS = 400;
const GAP_MS = 5 * 60_000;

function downsample(samples: MetricSample[]): MetricSample[] {
  if (samples.length <= MAX_POINTS) return samples;
  const bucketSize = Math.ceil(samples.length / MAX_POINTS);
  const out: MetricSample[] = [];
  for (let i = 0; i < samples.length; i += bucketSize) {
    const bucket = samples.slice(i, i + bucketSize);
    const avg = (pick: (s: MetricSample) => number | null) => {
      const vals = bucket.map(pick).filter((v): v is number => v != null);
      return vals.length ? vals.reduce((a, b) => a + b, 0) / vals.length : null;
    };
    out.push({
      ts: bucket[Math.floor(bucket.length / 2)].ts,
      cpuPct: avg((s) => s.cpuPct) ?? 0,
      memPct: avg((s) => s.memPct) ?? 0,
      diskPct: avg((s) => s.diskPct),
    });
  }
  return out;
}

function fmtTime(ms: number) {
  return new Date(ms).toLocaleTimeString("it-IT", { hour: "2-digit", minute: "2-digit" });
}

export function MetricsHistory() {
  const [hours, setHours] = useState(24);
  const [samples, setSamples] = useState<MetricSample[] | null>(null);
  const [hidden, setHidden] = useState<SeriesKey[]>(loadHidden);

  const load = async (h: number) => {
    const r = await api<{ samples: MetricSample[]; hours: number }>(
      `/api/metrics/history?hours=${h}`,
    );
    if (r.ok) setSamples(r.data.samples);
  };

  useEffect(() => {
    load(hours);
    const id = setInterval(() => load(hours), 30_000);
    return () => clearInterval(id);
  }, [hours]);

  const toggleSeries = (key: SeriesKey) => {
    setHidden((prev) => {
      const next = prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key];
      localStorage.setItem(HIDDEN_KEY, JSON.stringify(next));
      return next;
    });
  };

  const points = useMemo(() => (samples ? downsample(samples) : []), [samples]);

  const geometry = useMemo(() => {
    if (points.length < 2) return null;
    const t0 = points[0].ts;
    const t1 = points[points.length - 1].ts;
    const span = Math.max(1, t1 - t0);
    const x = (ts: number) => PAD_L + ((ts - t0) / span) * (W - PAD_L - PAD_R);
    const y = (pct: number) => PAD_T + (1 - Math.min(pct, 100) / 100) * (H - PAD_T - PAD_B);
    return { t0, t1, x, y };
  }, [points]);

  const last = samples && samples.length > 0 ? samples[samples.length - 1] : null;
  const visible = SERIES.filter((s) => !hidden.includes(s.key));

  return (
    <section className="metrics-history">
      <div className="metrics-head">
        <span className="gauge-title">Storico {hours}h</span>
        <div className="segmented">
          {RANGES.map((r) => (
            <button
              key={r.hours}
              className={hours === r.hours ? "active" : ""}
              onClick={() => setHours(r.hours)}
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>

      {}
      <div className="metrics-legend">
        {SERIES.map((s) => {
          const v = last ? (last[s.key] as number | null) : null;
          const off = hidden.includes(s.key);
          return (
            <button
              key={s.key}
              type="button"
              className={`metrics-legend-item ${off ? "off" : ""}`}
              aria-pressed={!off}
              title={off ? `Mostra ${s.label}` : `Nascondi ${s.label}`}
              onClick={() => toggleSeries(s.key)}
            >
              <span className="metrics-swatch" style={{ background: s.color }} />
              {s.label}
              {v != null && <span className="dim"> {v.toFixed(0)}%</span>}
            </button>
          );
        })}
      </div>

      {!geometry ? (
        <div className="empty">
          {samples === null
            ? "Carico lo storico…"
            : "Lo storico si popola man mano (un campione ogni 30s)."}
        </div>
      ) : (
        <>
          <div className="metrics-plot">
            <div className="metrics-yaxis" aria-hidden>
              {[100, 50, 0].map((v) => (
                <span key={v} style={{ top: `${topPct(v)}%` }}>
                  {v}
                </span>
              ))}
            </div>
            <svg
              className="metrics-chart"
              viewBox={`0 0 ${W} ${H}`}
              role="img"
              aria-label={`Storico metriche ${hours} ore`}
            >
              {[0, 25, 50, 75, 100].map((g) => (
                <line
                  key={g}
                  x1={PAD_L}
                  y1={geometry.y(g)}
                  x2={W - PAD_R}
                  y2={geometry.y(g)}
                  stroke="var(--border)"
                  strokeWidth="1"
                />
              ))}
              {visible.map((s) => {
                const segments: string[][] = [];
                let current: string[] = [];
                let prevTs: number | null = null;
                for (const p of points) {
                  const v = p[s.key] as number | null;
                  const gap = prevTs != null && p.ts - prevTs > GAP_MS;
                  if (v == null || gap) {
                    if (current.length > 0) segments.push(current);
                    current = [];
                  }
                  if (v != null) {
                    current.push(`${geometry.x(p.ts).toFixed(1)},${geometry.y(v).toFixed(1)}`);
                    prevTs = p.ts;
                  } else {
                    prevTs = null;
                  }
                }
                if (current.length > 0) segments.push(current);
                return segments.map((seg, i) => (
                  <polyline
                    key={`${s.key}-${i}`}
                    points={seg.join(" ")}
                    fill="none"
                    stroke={s.color}
                    strokeWidth="1.5"
                    strokeLinejoin="round"
                    vectorEffect="non-scaling-stroke"
                  />
                ));
              })}
            </svg>
          </div>
          {
}
          <div className="metrics-xaxis" aria-hidden>
            <span>{fmtTime(geometry.t0)}</span>
            <span>{fmtTime((geometry.t0 + geometry.t1) / 2)}</span>
            <span>{fmtTime(geometry.t1)}</span>
          </div>
        </>
      )}
      {visible.length === 0 && (
        <div className="dim metrics-empty-note">Tutte le serie sono nascoste.</div>
      )}
    </section>
  );
}
