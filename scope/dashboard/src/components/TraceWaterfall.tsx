import { useState, useEffect } from "react";
import { getTraces } from "../api/client";
import type { TraceSpan } from "../api/types";
import { useRefreshInterval } from "./RefreshPicker";

const SPAN_COLORS: Record<string, string> = {
  nginx: "#4a90d9",
  api: "#7b68ee",
  db: "#2ecc71",
  cache: "#e74c3c",
};

function getColor(service: string): string {
  return SPAN_COLORS[service] ?? `hsl(${(service.charCodeAt(0) * 37) % 360}, 50%, 55%)`;
}

export default function TraceWaterfall() {
  const [spans, setSpans] = useState<TraceSpan[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const { interval } = useRefreshInterval();

  const fetchData = async () => {
    try {
      setSpans(await getTraces());
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

  const maxEnd = Math.max(...spans.map((s) => s.start + s.duration), 1);

  return (
    <div>
      <h2>Trace Waterfall</h2>
      <div style={{ fontFamily: "monospace" }}>
        {spans.map((span) => (
          <div key={span.id} style={{ display: "flex", alignItems: "center", marginBottom: 4 }}>
            <div style={{ width: 200, textAlign: "right", paddingRight: 8, fontSize: "0.85rem" }}>
              {span.name}
            </div>
            <div style={{ flex: 1, position: "relative", height: 24 }}>
              <div
                style={{
                  position: "absolute",
                  left: `${(span.start / maxEnd) * 100}%`,
                  width: `${(span.duration / maxEnd) * 100}%`,
                  height: "100%",
                  background: getColor(span.service),
                  borderRadius: 3,
                  display: "flex",
                  alignItems: "center",
                  paddingLeft: 4,
                  fontSize: "0.75rem",
                  color: "#fff",
                }}
              >
                {span.duration}ms
              </div>
            </div>
            <div style={{ width: 60, fontSize: "0.8rem", color: "#999", paddingLeft: 4 }}>
              {span.service}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
