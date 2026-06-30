import { useState, useEffect, createContext, useContext, ReactNode } from "react";

export type RefreshInterval = 5 | 15 | 30 | 0; // 0 = manual

interface RefreshContextValue {
  interval: RefreshInterval;
  setInterval: (v: RefreshInterval) => void;
}

const RefreshContext = createContext<RefreshContextValue>({
  interval: 15,
  setInterval: () => {},
});

export function useRefreshInterval() {
  return useContext(RefreshContext);
}

const STORAGE_KEY = "scope-refresh-interval";

const options: { label: string; value: RefreshInterval }[] = [
  { label: "5s", value: 5 },
  { label: "15s", value: 15 },
  { label: "30s", value: 30 },
  { label: "Manual", value: 0 },
];

export function RefreshProvider({ children }: { children: ReactNode }) {
  const [interval, setInterval] = useState<RefreshInterval>(() => {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored !== null) {
      const n = parseInt(stored, 10);
      if ([5, 15, 30, 0].includes(n)) return n as RefreshInterval;
    }
    return 15;
  });

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, String(interval));
  }, [interval]);

  return (
    <RefreshContext.Provider value={{ interval, setInterval }}>
      {children}
    </RefreshContext.Provider>
  );
}

export default function RefreshPicker() {
  const { interval, setInterval } = useRefreshInterval();

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <span style={{ fontSize: 12, opacity: 0.7 }}>Refresh:</span>
      <select
        value={interval}
        onChange={(e) => setInterval(Number(e.target.value) as RefreshInterval)}
        style={{
          background: "var(--bg-secondary, #161b22)",
          color: "var(--text-primary, #e6edf3)",
          border: "1px solid var(--border, #30363d)",
          borderRadius: 4,
          padding: "2px 6px",
          fontSize: 12,
        }}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </div>
  );
}
