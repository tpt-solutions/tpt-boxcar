import { useState, useEffect, useCallback } from "react";
import {
  listServices,
  startService,
  stopService,
  type Service,
} from "../api/tauriApi";

export default function Dashboard() {
  const [services, setServices] = useState<Service[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionPending, setActionPending] = useState<string | null>(null);

  const fetchServices = useCallback(async () => {
    try {
      const data = await listServices();
      setServices(data);
      setError(null);
    } catch (err) {
      console.error("Failed to list services:", err);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchServices();
    const id = setInterval(fetchServices, 5000);
    return () => clearInterval(id);
  }, [fetchServices]);

  const handleStart = async (svc: Service) => {
    setActionPending(svc.id);
    try {
      await startService(svc.id);
      await fetchServices();
    } catch (err) {
      console.error(`Failed to start ${svc.name}:`, err);
    } finally {
      setActionPending(null);
    }
  };

  const handleStop = async (svc: Service) => {
    setActionPending(svc.id);
    try {
      await stopService(svc.id);
      await fetchServices();
    } catch (err) {
      console.error(`Failed to stop ${svc.name}:`, err);
    } finally {
      setActionPending(null);
    }
  };

  if (loading) {
    return (
      <div>
        <h2>Services</h2>
        <p className="loading-spinner">Loading services...</p>
      </div>
    );
  }

  if (error) {
    return (
      <div>
        <h2>Services</h2>
        <div className="error">Error: {error}</div>
        <button onClick={fetchServices}>Retry</button>
      </div>
    );
  }

  return (
    <div>
      <h2>Services</h2>
      <table>
        <thead>
          <tr>
            <th>Name</th>
            <th>Kind</th>
            <th>Status</th>
            <th>CPU (%)</th>
            <th>Mem (MB)</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {services.map((svc) => (
            <tr key={svc.id}>
              <td>{svc.name}</td>
              <td>{svc.kind.toUpperCase()}</td>
              <td className={`status-${svc.status}`}>{svc.status}</td>
              <td>{svc.cpu.toFixed(1)}</td>
              <td>{svc.memMb}</td>
              <td>
                <button
                  onClick={() => handleStart(svc)}
                  disabled={
                    actionPending === svc.id || svc.status === "running"
                  }
                >
                  {actionPending === svc.id ? "Working..." : "Start"}
                </button>
                <button
                  onClick={() => handleStop(svc)}
                  disabled={
                    actionPending === svc.id || svc.status === "stopped"
                  }
                  style={{ marginLeft: "0.5rem" }}
                >
                  {actionPending === svc.id ? "Working..." : "Stop"}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
