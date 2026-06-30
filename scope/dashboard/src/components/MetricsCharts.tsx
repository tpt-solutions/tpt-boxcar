import { useState, useEffect } from "react";
import { getMetrics } from "../api/client";
import type { MetricSeries } from "../api/types";

function MiniLineChart({ series, color }: { series: MetricSeries; color: string }) {
  if (!series.points.length) return <div style={{ height: 150, background: "#1a1a2e", borderRadius: 4 }} />;
  const maxVal = Math.max(...series.points.map((p) => p.value));
  const minVal = Math.min(...series.points.map((p) => p.value));
  const range = maxVal - minVal || 1;
  const w = 500;
  const h = 150;
  const points = series.points
    .map((p, i) => `${(i / (series.points.length - 1)) * w},${h - ((p.value - minVal) / range) * (h - 20) - 10}`)
    .join(" ");
  return (
    <svg width="100%" viewBox={`0 0 ${w} ${h}`} style={{ background: "#1a1a2e", borderRadius: 4 }}>
      <polyline fill="none" stroke={color} strokeWidth={2} points={points} />
    </svg>
  );
}

export default function MetricsCharts() {
  const [metrics, setMetrics] = useState<MetricSeries[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

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
    const id = setInterval(fetchData, 5_000);
    return () => clearInterval(id);
  }, []);

  if (loading) return <div style={{ padding: "2rem", color: "#666" }}>Loading...</div>;
  if (error) return <div style={{ padding: "2rem", color: "#f85149" }}>Error: {error}</div>;

  const charts = [
    { title: "Latency", color: "#4a90d9" },
    { title: "Throughput", color: "#2ecc71" },
    { title: "Error Rate", color: "#f85149" },
    { title: "Connections", color: "#d29922" },
  ];

  return (
    <div>
      <h2>Metrics</h2>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "1rem" }}>
        {charts.map((c, i) => (
          <div key={c.title} style={{ border: "1px solid #333", borderRadius: 8, padding: "1rem" }}>
            <h3>{c.title}</h3>
            <p style={{ color: "#999", fontSize: "0.85rem" }}>
              {metrics[i]?.points.length ? `${metrics[i].points.length} data points` : "No data"}
            </p>
            <MiniLineChart series={metrics[i] ?? { name: c.title, points: [] }} color={c.color} />
          </div>
        ))}
      </div>
    </div>
  );
}
