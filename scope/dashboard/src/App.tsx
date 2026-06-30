import { useState } from "react";
import { ThemeProvider, useTheme } from "./components/ThemeToggle";
import { RefreshProvider } from "./components/RefreshPicker";
import RefreshPicker from "./components/RefreshPicker";
import ServiceGraph from "./components/ServiceGraph";
import TraceWaterfall from "./components/TraceWaterfall";
import MetricsCharts from "./components/MetricsCharts";
import LogViewer from "./components/LogViewer";
import WasmMetrics from "./components/WasmMetrics";

type View = "graph" | "traces" | "metrics" | "wasm" | "logs";

function ThemeToggleButton() {
  const { theme, toggle } = useTheme();
  return (
    <button
      onClick={toggle}
      title={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
      style={{
        background: "transparent",
        border: "1px solid var(--border, #333)",
        borderRadius: 4,
        cursor: "pointer",
        padding: "4px 8px",
        color: "var(--text-primary, #fff)",
        fontSize: 16,
        marginLeft: "auto",
      }}
    >
      {theme === "dark" ? "\u2600" : "\u263E"}
    </button>
  );
}

export default function App() {
  return (
    <ThemeProvider>
      <RefreshProvider>
        <AppInner />
      </RefreshProvider>
    </ThemeProvider>
  );
}

function AppInner() {
  const [view, setView] = useState<View>("graph");

  return (
    <div
      style={{
        display: "flex",
        height: "100vh",
        fontFamily: "system-ui",
        background: "var(--bg-primary, #0d1117)",
        color: "var(--text-primary, #e6edf3)",
      }}
    >
      <nav
        style={{
          width: 200,
          background: "var(--bg-nav, #0d1117)",
          color: "var(--text-primary, #fff)",
          padding: "1rem",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", marginBottom: "1rem" }}>
          <h2 style={{ fontSize: "1.1rem", margin: 0 }}>TPT Scope</h2>
          <ThemeToggleButton />
        </div>
        <RefreshPicker />
        <div style={{ marginTop: "0.5rem" }}>
          {(["graph", "traces", "metrics", "wasm", "logs"] as View[]).map((v) => (
            <button
              key={v}
              onClick={() => setView(v)}
              style={{
                display: "block",
                width: "100%",
                padding: "0.5rem",
                margin: "0.25rem 0",
                background: view === v ? "var(--accent, #1f6feb)" : "transparent",
                color: "var(--text-primary, #fff)",
                border: "1px solid var(--border, #333)",
                borderRadius: 4,
                cursor: "pointer",
                textAlign: "left",
              }}
            >
              {v.charAt(0).toUpperCase() + v.slice(1)}
            </button>
          ))}
        </div>
      </nav>
      <main style={{ flex: 1, padding: "1rem", overflow: "auto" }}>
        {view === "graph" && <ServiceGraph />}
        {view === "traces" && <TraceWaterfall />}
        {view === "metrics" && <MetricsCharts />}
        {view === "wasm" && <WasmMetrics />}
        {view === "logs" && <LogViewer />}
      </main>
    </div>
  );
}
