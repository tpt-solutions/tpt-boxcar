import { useEffect, useRef, useState } from "react";
import cytoscape from "cytoscape";
import { getServices } from "../api/client";
import type { ServiceNode } from "../api/types";
import { useRefreshInterval } from "./RefreshPicker";

const HEALTH_COLOR: Record<string, string> = {
  healthy: "#2ecc71",
  degraded: "#d29922",
  down: "#f85149",
};

function toElements(services: ServiceNode[]): cytoscape.ElementDefinition[] {
  const names = new Set(services.map((s) => s.name));
  const nodes: cytoscape.ElementDefinition[] = services.map((s) => ({
    data: { id: s.name, label: s.name, health: s.health },
  }));
  const edges: cytoscape.ElementDefinition[] = services.flatMap((s) =>
    s.dependencies
      .filter((dep) => names.has(dep))
      .map((dep) => ({ data: { id: `${s.name}->${dep}`, source: s.name, target: dep } }))
  );
  return [...nodes, ...edges];
}

export default function ServiceGraph() {
  const containerRef = useRef<HTMLDivElement>(null);
  const cyRef = useRef<cytoscape.Core | null>(null);
  const [services, setServices] = useState<ServiceNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const { interval } = useRefreshInterval();

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
    if (interval === 0) return;
    const id = setInterval(fetchData, interval * 1000);
    return () => clearInterval(id);
  }, [interval]);

  useEffect(() => {
    if (!containerRef.current) return;

    const cy = cytoscape({
      container: containerRef.current,
      elements: [],
      style: [
        {
          selector: "node",
          style: {
            "background-color": (ele: cytoscape.NodeSingular) => HEALTH_COLOR[ele.data("health")] ?? "#666",
            "background-opacity": 0.25,
            "border-color": (ele: cytoscape.NodeSingular) => HEALTH_COLOR[ele.data("health")] ?? "#666",
            "border-width": 2,
            label: "data(label)",
            color: "#c9d1d9",
            "font-size": 12,
            "font-family": "monospace",
            "text-valign": "center",
            "text-halign": "center",
            width: 48,
            height: 48,
          },
        },
        {
          selector: "edge",
          style: {
            width: 2,
            "line-color": "#555",
            "target-arrow-color": "#555",
            "target-arrow-shape": "triangle",
            "curve-style": "bezier",
          },
        },
      ],
      layout: { name: "cose" },
      minZoom: 0.2,
      maxZoom: 3,
      wheelSensitivity: 0.2,
    });

    cyRef.current = cy;

    return () => {
      cy.destroy();
      cyRef.current = null;
    };
  }, []);

  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;

    cy.elements().remove();
    cy.add(toElements(services));
    cy.layout({ name: "cose", animate: false }).run();
  }, [services]);

  return (
    <div>
      <h2>Service Dependency Graph</h2>
      {error && <div style={{ color: "#f85149", marginBottom: "0.5rem" }}>Error: {error}</div>}
      {loading && !error && <div style={{ color: "#666", marginBottom: "0.5rem" }}>Loading...</div>}
      <div
        ref={containerRef}
        style={{ width: 600, height: 500, border: "1px solid #333", borderRadius: 8, background: "#0d1117" }}
      />
    </div>
  );
}
