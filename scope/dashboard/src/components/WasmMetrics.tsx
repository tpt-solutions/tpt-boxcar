import { useState, useEffect } from "react";
import { getWasmMetrics } from "../api/client";
import type { WasmModule } from "../api/types";

export default function WasmMetrics() {
  const [modules, setModules] = useState<WasmModule[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchData = async () => {
    try {
      setModules(await getWasmMetrics());
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

  return (
    <div>
      <h2>Wasm-Specific Metrics</h2>
      <table style={{ width: "100%", borderCollapse: "collapse" }}>
        <thead>
          <tr>
            <th style={{ textAlign: "left", padding: "0.5rem", borderBottom: "2px solid #333" }}>Module</th>
            <th style={{ textAlign: "left", padding: "0.5rem", borderBottom: "2px solid #333" }}>Compile Time</th>
            <th style={{ textAlign: "left", padding: "0.5rem", borderBottom: "2px solid #333" }}>Instantiate</th>
            <th style={{ textAlign: "left", padding: "0.5rem", borderBottom: "2px solid #333" }}>Memory Pages</th>
            <th style={{ textAlign: "left", padding: "0.5rem", borderBottom: "2px solid #333" }}>Memory</th>
          </tr>
        </thead>
        <tbody>
          {modules.map((mod) => (
            <tr key={mod.name}>
              <td style={{ padding: "0.5rem" }}>{mod.name}</td>
              <td style={{ padding: "0.5rem" }}>{mod.compileTime}</td>
              <td style={{ padding: "0.5rem" }}>{mod.instantiateTime}</td>
              <td style={{ padding: "0.5rem" }}>{mod.memoryPages}</td>
              <td style={{ padding: "0.5rem" }}>{mod.memoryMB} MB</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
