import { useState, useEffect } from "react";
import {
  ResponsiveContainer,
  AreaChart,
  Area,
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
} from "recharts";
import { getMetrics } from "../api/client";
import type { MetricSeries } from "../api/types";
import { useRefreshInterval } from "./RefreshPicker";

const CHARTS: { title: string; color: string; kind: "area" | "line" }[] = [
  { title: "Latency", color: "#4a90d9", kind: "area" },
  { title: "Throughput", color: "#2ecc71", kind: "area" },
  { title: "Error Rate", color: "#f85149", kind: "line" },
  { title: "Connections", color: "#d29922", kind: "area" },
];

function formatTime(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function MetricChart({ series, color, kind }: { series: MetricSeries; color: string; kind: "area" | "line" }) {
  if (!series.points.length) {
    return <div style={{ height: 150, background: "#1a1a2e", borderRadius: 4 }} />;
  }

  const data = series.points.map((p) => ({ timestamp: p.timestamp, value: p.value }));
  const tooltipStyle = { background: "#1a1a2e", border: "1px solid #333", fontSize: "0.8rem" };

  return (
    <ResponsiveContainer width="100%" height={150}>
      {kind === "area" ? (
        <AreaChart data={data}>
          <CartesianGrid strokeDasharray="3 3" stroke="#2a2a3e" />
          <XAxis dataKey="timestamp" tickFormatter={formatTime} stroke="#666" fontSize={11} />
          <YAxis stroke="#666" fontSize={11} width={40} />
          <Tooltip labelFormatter={(v) => formatTime(Number(v))} contentStyle={tooltipStyle} />
          <Legend />
          <Area type="monotone" dataKey="value" name={series.name} stroke={color} fill={color} fillOpacity={0.25} />
        </AreaChart>
      ) : (
        <LineChart data={data}>
          <CartesianGrid strokeDasharray="3 3" stroke="#2a2a3e" />
          <XAxis dataKey="timestamp" tickFormatter={formatTime} stroke="#666" fontSize={11} />
          <YAxis stroke="#666" fontSize={11} width={40} />
          <Tooltip labelFormatter={(v) => formatTime(Number(v))} contentStyle={tooltipStyle} />
          <Legend />
          <Line type="monotone" dataKey="value" name={series.name} stroke={color} dot={false} />
        </LineChart>
      )}
    </ResponsiveContainer>
  );
}

export default function MetricsCharts() {
  const [metrics, setMetrics] = useState<MetricSeries[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const { interval } = useRefreshInterval();

  const fetchData = async () => {
    try {
      setMetrics(await getMetrics());
      setError(null);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Failed to load");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
    if (interval === 0) return;
    const id = setInterval(fetchData, interval * 1000);
    return () => clearInterval(id);
  }, [interval]);

  if (loading) return <div style={{ padding: "2rem", color: "#666" }}>Loading...</div>;
  if (error) return <div style={{ padding: "2rem", color: "#f85149" }}>Error: {error}</div>;

  // Group series by metric name so charts stay stable regardless of API ordering.
  const byName = new Map<string, MetricSeries>();
  for (const s of metrics) byName.set(s.name, s);

  return (
    <div>
      <h2>Metrics</h2>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "1rem" }}>
        {CHARTS.map((c) => {
          const series = byName.get(c.title) ?? { name: c.title, points: [] };
          return (
            <div key={c.title} style={{ border: "1px solid #333", borderRadius: 8, padding: "1rem" }}>
              <h3>{c.title}</h3>
              <p style={{ color: "#999", fontSize: "0.85rem" }}>
                {series.points.length ? `${series.points.length} data points` : "No data"}
              </p>
              <MetricChart series={series} color={c.color} kind={c.kind} />
            </div>
          );
        })}
      </div>
    </div>
  );
}
