import { useState, useEffect, useRef } from "react";
import { streamLogs, type LogEvent } from "../api/tauriApi";

export default function Logs() {
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [filter, setFilter] = useState("");
  const [autoScroll, setAutoScroll] = useState(true);
  const [listening, setListening] = useState(false);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    streamLogs((entry) => {
      setLogs((prev) => [...prev, entry]);
    }).then((fn) => {
      unlisten = fn;
      setListening(true);
    });

    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (autoScroll) {
      endRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [logs, autoScroll]);

  const filtered = logs.filter(
    (log) =>
      !filter ||
      log.line.toLowerCase().includes(filter.toLowerCase()) ||
      log.service.toLowerCase().includes(filter.toLowerCase())
  );

  const formatTimestamp = (ts: number) =>
    new Date(ts).toISOString().replace("T", " ").slice(0, 23);

  return (
    <div>
      <h2>Logs</h2>
      <div className="log-controls">
        <input
          type="text"
          placeholder="Filter logs..."
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <label style={{ display: "flex", alignItems: "center", gap: "0.4rem" }}>
          <input
            type="checkbox"
            checked={autoScroll}
            onChange={(e) => setAutoScroll(e.target.checked)}
          />
          Auto-scroll
        </label>
      </div>
      {!listening ? (
        <p>Connecting to log stream...</p>
      ) : (
        <pre className="log-output">
          {filtered.map((log, i) => (
            <div key={i}>
              <span className="log-timestamp">
                [{formatTimestamp(log.timestamp)}]
              </span>{" "}
              <span className="log-service">[{log.service}]</span>{" "}
              {log.line}
            </div>
          ))}
          <div ref={endRef} />
        </pre>
      )}
    </div>
  );
}
