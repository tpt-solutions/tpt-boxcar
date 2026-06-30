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

  const handleToggle = async (svc: Service) => {
    setActionPending(svc.name);
    try {
      if (svc.status === "running") {
        await stopService(svc.name);
      } else {
        await startService(svc.name);
      }
      await fetchServices();
    } catch (err) {
      console.error(`Failed to toggle ${svc.name}:`, err);
    } finally {
      setActionPending(null);
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
}
