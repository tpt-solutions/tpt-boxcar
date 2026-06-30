import { useState, useEffect } from "react";
import { getLogs } from "../api/client";
import type { LogEntry } from "../api/types";
import { useRefreshInterval } from "./RefreshPicker";

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <button
      onClick={handleCopy}
      title="Copy log line"
      style={{
        background: "transparent",
        border: "none",
        cursor: "pointer",
        padding: "0 4px",
        fontSize: "0.85rem",
        opacity: copied ? 1 : 0.4,
        color: copied ? "#58a6ff" : "inherit",
        flexShrink: 0,
      }}
    >
      {copied ? "Copied!" : "⎘"}
    </button>
  );
}

export default function LogViewer() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [serviceFilter, setServiceFilter] = useState("");
  const [severityFilter, setSeverityFilter] = useState("");
  const [search, setSearch] = useState("");
  const { interval } = useRefreshInterval();

  const fetchLogs = async () => {
    try {
      setLogs(await getLogs());
      setError(null);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Failed to load");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchLogs();
    if (interval === 0) return;
    const id = setInterval(fetchLogs, interval * 1000);
    return () => clearInterval(id);
  }, [interval]);

  const filtered = logs.filter(
    (log) =>
      (!serviceFilter || log.service === serviceFilter) &&
      (!severityFilter || log.severity === severityFilter) &&
      (!search || log.message.toLowerCase().includes(search.toLowerCase()))
  );

  return (
    <div>
      <h2>Log Viewer</h2>
      <div style={{ display: "flex", gap: "0.5rem", marginBottom: "1rem" }}>
        <input placeholder="Search..." value={search} onChange={(e) => setSearch(e.target.value)} />
        <select value={serviceFilter} onChange={(e) => setServiceFilter(e.target.value)}>
          <option value="">All services</option>
          <option value="nginx">nginx</option>
          <option value="api">api</option>
          <option value="db">db</option>
        </select>
        <select value={severityFilter} onChange={(e) => setSeverityFilter(e.target.value)}>
          <option value="">All levels</option>
          <option value="info">info</option>
          <option value="warn">warn</option>
          <option value="error">error</option>
        </select>
      </div>
      {loading && logs.length === 0 ? (
        <div style={{ color: "#666" }}>Loading...</div>
      ) : error && logs.length === 0 ? (
        <div style={{ color: "#f85149" }}>Error: {error}</div>
      ) : (
        <pre style={{ background: "#0d1117", color: "#c9d1d9", padding: "1rem", borderRadius: 4, fontSize: "0.85rem" }}>
          {filtered.map((log, i) => {
            const lineText = `${log.time} [${log.service}] ${log.message}`;
            return (
              <div key={i} style={{ display: "flex", alignItems: "baseline" }}>
                <CopyButton text={lineText} />
                <span>
                  <span style={{ color: "#8b949e" }}>{log.time}</span>{" "}
                  <span
                    style={{
                      color:
                        log.severity === "error" ? "#f85149" : log.severity === "warn" ? "#d29922" : "#58a6ff",
                    }}
                  >
                    [{log.service}]
                  </span>{" "}
                  {log.message}
                </span>
              </div>
            );
          })}
        </pre>
      )}
    </div>
  );
}
