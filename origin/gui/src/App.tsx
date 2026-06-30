import { useState, useCallback, useRef } from "react";
import { ThemeProvider, useTheme } from "./components/ThemeToggle";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import Dashboard, { type DashboardHandle } from "./components/Dashboard";
import Logs from "./components/Logs";
import ManifestEditor from "./components/ManifestEditor";

type View = "dashboard" | "logs" | "editor";

function ThemeToggleButton() {
  const { theme, toggle } = useTheme();
  return (
    <button
      onClick={toggle}
      title={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
      className="theme-toggle"
    >
      {theme === "dark" ? "☀" : "☾"}
    </button>
  );
}

export default function App() {
  return (
    <ThemeProvider>
      <AppInner />
    </ThemeProvider>
  );
}

function AppInner() {
  const [view, setView] = useState<View>("dashboard");
  const dashboardRef = useRef<DashboardHandle>(null);

  const handleLogs = useCallback(() => setView("logs"), []);
  const handleStart = useCallback(() => {
    setView("dashboard");
    // Allow state update to flush before calling ref method
    setTimeout(() => dashboardRef.current?.startFirst(), 0);
  }, []);
  const handleStop = useCallback(() => {
    setView("dashboard");
    setTimeout(() => dashboardRef.current?.stopFirst(), 0);
  }, []);
  const handleRestart = useCallback(() => {
    setView("dashboard");
    setTimeout(() => dashboardRef.current?.restartFirst(), 0);
  }, []);

  useKeyboardShortcuts({
    onStart: handleStart,
    onStop: handleStop,
    onRestart: handleRestart,
    onLogs: handleLogs,
  });

  return (
    <div
      className="app"
      style={{
        background: "var(--bg-primary, #0d1117)",
        color: "var(--text-primary, #e6edf3)",
      }}
    >
      <nav
        className="sidebar"
        style={{
          background: "var(--bg-nav, #0d1117)",
          color: "var(--text-primary, #fff)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center" }}>
          <h1 className="logo" style={{ flex: 1 }}>TPT Origin</h1>
          <ThemeToggleButton />
        </div>
        <button onClick={() => setView("dashboard")}>Dashboard</button>
        <button onClick={() => setView("logs")}>Logs</button>
        <button onClick={() => setView("editor")}>Manifest</button>
        <div style={{ marginTop: "1rem", fontSize: 11, opacity: 0.5 }}>
          Shortcuts: S=start X=stop R=restart L=logs
        </div>
      </nav>
      <main
        className="content"
        style={{
          background: "var(--bg-secondary, #161b22)",
          color: "var(--text-primary, #e6edf3)",
        }}
      >
        {view === "dashboard" && <Dashboard ref={dashboardRef} />}
        {view === "logs" && <Logs />}
        {view === "editor" && <ManifestEditor />}
      </main>
    </div>
  );
}
