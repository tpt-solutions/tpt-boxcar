import { useState, useEffect, useRef } from "react";
import { streamLogs, type LogEntry } from "../api/tauriApi";

export default function Logs() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState("");
  const [autoScroll, setAutoScroll] = useState(true);
  const [loading, setLoading] = useState(true);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    streamLogs((entry) => {
      setLogs((prev) => [...prev, entry]);
      setLoading(false);
    }).then((fn) => {
      unlisten = fn;
      setLoading(false);
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
      log.message.toLowerCase().includes(filter.toLowerCase()) ||
      log.service.toLowerCase().includes(filter.toLowerCase())
  );

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
        <button onClick={() => setAutoScroll((prev) => !prev)}>
          Auto-scroll: {autoScroll ? "On" : "Off"}
        </button>
      </div>
      {loading ? (
        <p>Waiting for log events...</p>
      ) : (
        <pre className="log-output">
          {filtered.map((log, i) => (
            <div key={i}>
              <span className="log-timestamp">[{log.timestamp}]</span>{" "}
              <span className="log-service">[{log.service}]</span>{" "}
              {log.message}
            </div>
          ))}
          <div ref={endRef} />
        </pre>
      )}
    </div>
  );
}
