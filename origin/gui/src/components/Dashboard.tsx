import { useState, useEffect, useCallback, useImperativeHandle, forwardRef } from "react";
import {
  listServices,
  startService,
  stopService,
  type Service,
} from "../api/tauriApi";

export interface DashboardHandle {
  startFirst: () => void;
  stopFirst: () => void;
  restartFirst: () => void;
}

const Dashboard = forwardRef<DashboardHandle>(function Dashboard(_props, ref) {
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

  const doStart = useCallback(async (id: string) => {
    setActionPending(id);
    try {
      await startService(id);
      await fetchServices();
    } catch (err) {
      console.error(`Failed to start ${id}:`, err);
    } finally {
      setActionPending(null);
    }
  }, [fetchServices]);

  const doStop = useCallback(async (id: string) => {
    setActionPending(id);
    try {
      await stopService(id);
      await fetchServices();
    } catch (err) {
      console.error(`Failed to stop ${id}:`, err);
    } finally {
      setActionPending(null);
    }
  }, [fetchServices]);

  const doRestart = useCallback(async (id: string) => {
    setActionPending(id);
    try {
      await stopService(id);
      await startService(id);
      await fetchServices();
    } catch (err) {
      console.error(`Failed to restart ${id}:`, err);
    } finally {
      setActionPending(null);
    }
  }, [fetchServices]);

  useImperativeHandle(ref, () => ({
    startFirst: () => {
      const first = services[0];
      if (first) doStart(first.id);
    },
    stopFirst: () => {
      const first = services[0];
      if (first) doStop(first.id);
    },
    restartFirst: () => {
      const first = services[0];
      if (first) doRestart(first.id);
    },
  }), [services, doStart, doStop, doRestart]);

  const handleToggle = async (svc: Service) => {
    if (svc.status === "running") {
      await doStop(svc.id);
    } else {
      await doStart(svc.id);
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
                  onClick={() => doStart(svc.id)}
                  disabled={
                    actionPending === svc.id || svc.status === "running"
                  }
                >
                  {actionPending === svc.id ? "Working..." : "Start"}
                </button>
                <button
                  onClick={() => doStop(svc.id)}
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
});

export default Dashboard;
