import { useState, useEffect } from "react";
import { getServices } from "../api/client";
import type { ServiceNode } from "../api/types";

const HEALTH_COLOR: Record<string, string> = {
  healthy: "#2ecc71",
  degraded: "#d29922",
  down: "#f85149",
};

function layoutNodes(nodes: ServiceNode[]): { x: number; y: number; node: ServiceNode }[] {
  const positions = nodes.map((node, i) => ({
    x: 120 + (i % 3) * 160,
    y: 80 + Math.floor(i / 3) * 120,
    node,
  }));
  return positions;
}

export default function ServiceGraph() {
  const [services, setServices] = useState<ServiceNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = async () => {
    try {
      setServices(await getServices());
      setError(null);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Failed to load");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
    const id = setInterval(fetchData, 10_000);
    return () => clearInterval(id);
  }, []);

  if (loading) return <div style={{ padding: "2rem", color: "#666" }}>Loading...</div>;
  if (error) return <div style={{ padding: "2rem", color: "#f85149" }}>Error: {error}</div>;

  const positioned = layoutNodes(services);

  return (
    <div>
      <h2>Service Dependency Graph</h2>
      <svg width="600" height="500" style={{ border: "1px solid #333", borderRadius: 8, background: "#0d1117" }}>
        {positioned.map((a) =>
          a.node.dependencies
            .map((dep) => positioned.find((b) => b.node.name === dep))
            .filter(Boolean)
            .map((b) => {
              const dx = b!.x - a.x;
              const dy = b!.y - a.y;
              const dist = Math.sqrt(dx * dx + dy * dy);
              const offset = 25;
              return (
                <line
                  key={`${a.node.name}-${b!.node.name}`}
                  x1={a.x + (dx / dist) * offset}
                  y1={a.y + (dy / dist) * offset}
                  x2={b!.x - (dx / dist) * offset}
                  y2={b!.y - (dy / dist) * offset}
                  stroke="#555"
                  strokeWidth={2}
                  markerEnd="url(#arrowhead)"
                />
              );
            })
        )}
        <defs>
          <marker id="arrowhead" markerWidth="10" markerHeight="7" refX="10" refY="3.5" orient="auto">
            <polygon points="0 0, 10 3.5, 0 7" fill="#555" />
          </marker>
        </defs>
        {positioned.map((p) => (
          <g key={p.node.name}>
            <circle cx={p.x} cy={p.y} r={22} fill={HEALTH_COLOR[p.node.health] ?? "#666"} fillOpacity={0.25} stroke={HEALTH_COLOR[p.node.health] ?? "#666"} strokeWidth={2} />
            <text x={p.x} y={p.y + 4} textAnchor="middle" fill="#c9d1d9" fontSize={12} fontFamily="monospace">{p.node.name}</text>
          </g>
        ))}
      </svg>
    </div>
  );
}
