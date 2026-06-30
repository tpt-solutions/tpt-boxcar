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
  const [actionPending, setActionPending] = useState<string | null>(null);

  const fetchServices = useCallback(async () => {
    try {
      const data = await listServices();
      setServices(data);
    } catch (err) {
      console.error("Failed to list services:", err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchServices();
    const id = setInterval(fetchServices, 5000);
    return () => clearInterval(id);
  }, [fetchServices]);

  const doStart = useCallback(async (name: string) => {
    setActionPending(name);
    try {
      await startService(name);
      await fetchServices();
    } catch (err) {
      console.error(`Failed to start ${name}:`, err);
    } finally {
      setActionPending(null);
    }
  }, [fetchServices]);

  const doStop = useCallback(async (name: string) => {
    setActionPending(name);
    try {
      await stopService(name);
      await fetchServices();
    } catch (err) {
      console.error(`Failed to stop ${name}:`, err);
    } finally {
      setActionPending(null);
    }
  }, [fetchServices]);

  const doRestart = useCallback(async (name: string) => {
    setActionPending(name);
    try {
      await stopService(name);
      await startService(name);
      await fetchServices();
    } catch (err) {
      console.error(`Failed to restart ${name}:`, err);
    } finally {
      setActionPending(null);
    }
  }, [fetchServices]);

  useImperativeHandle(ref, () => ({
    startFirst: () => {
      const first = services[0];
      if (first) doStart(first.name);
    },
    stopFirst: () => {
      const first = services[0];
      if (first) doStop(first.name);
    },
    restartFirst: () => {
      const first = services[0];
      if (first) doRestart(first.name);
    },
  }), [services, doStart, doStop, doRestart]);

  const handleToggle = async (svc: Service) => {
    if (svc.status === "running") {
      await doStop(svc.name);
    } else {
      await doStart(svc.name);
    }
  };

  if (loading) {
    return (
      <div>
        <h2>Services</h2>
        <p>Loading services...</p>
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
            <th>Type</th>
            <th>Status</th>
            <th>CPU</th>
            <th>RAM</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {services.map((svc) => (
            <tr key={svc.name}>
              <td>{svc.name}</td>
              <td>{svc.type.toUpperCase()}</td>
              <td className={`status-${svc.status}`}>{svc.status}</td>
              <td>{svc.cpu}</td>
              <td>{svc.ram}</td>
              <td>
                <button
                  onClick={() => handleToggle(svc)}
                  disabled={actionPending === svc.name}
                >
                  {actionPending === svc.name
                    ? "Working..."
                    : svc.status === "running"
                      ? "Stop"
                      : "Start"}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <h2>System Resources</h2>
      <div className="resources">
        <p>Total CPU: 2.4%</p>
        <p>Total RAM: 144MB / 16384MB</p>
      </div>
    </div>
  );
});

export default Dashboard;
