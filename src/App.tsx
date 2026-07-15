import { useEffect, useState } from "react";
import { PairGate } from "./app/PairGate";
import { VitalsPanel } from "./app/VitalsPanel";
import { Dashboard } from "./features/dashboard/Dashboard";
import { Ports } from "./features/ports/Ports";
import { Settings } from "./features/settings/Settings";
import { ws } from "./lib/ws";
import { useStatsStore } from "./stores/statsStore";
import type { MachineStats } from "./lib/types";

type Section = "dashboard" | "ports" | "projects" | "services" | "settings";

const SECTIONS: { id: Section; icon: string; label: string }[] = [
  { id: "dashboard", icon: "🖥", label: "Dashboard" },
  { id: "ports", icon: "🔌", label: "Porte" },
  { id: "projects", icon: "📁", label: "Progetti" },
  { id: "services", icon: "🌐", label: "Servizi" },
  { id: "settings", icon: "⚙️", label: "Impostazioni" },
];

function Placeholder({ title, milestone }: { title: string; milestone: string }) {
  return (
    <div>
      <h2>{title}</h2>
      <div className="empty">In arrivo con la {milestone}.</div>
    </div>
  );
}

export default function App() {
  const [section, setSection] = useState<Section>("dashboard");
  const push = useStatsStore((s) => s.push);
  const setError = useStatsStore((s) => s.setError);

  // Il pannello vital signs è sempre visibile, quindi il topic "stats"
  // resta sottoscritto per tutta la vita della UI.
  useEffect(() => {
    return ws.subscribe("stats", (event) => {
      if (event.topic === "stats") push(event.payload as MachineStats);
      else if (event.topic === "stats:error")
        setError((event.payload as { message: string }).message);
    });
  }, [push, setError]);

  return (
    <PairGate>
      <div className="shell">
        <nav className="rail">
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              className={`rail-btn ${section === s.id ? "active" : ""}`}
              title={s.label}
              onClick={() => setSection(s.id)}
            >
              <span className="rail-icon">{s.icon}</span>
              <span className="rail-label">{s.label}</span>
            </button>
          ))}
        </nav>

        <main className="main">
          {section === "dashboard" && <Dashboard />}
          {section === "ports" && <Ports />}
          {section === "projects" && <Placeholder title="Progetti" milestone="M4" />}
          {section === "services" && <Placeholder title="Servizi online" milestone="M7" />}
          {section === "settings" && <Settings />}
        </main>

        <VitalsPanel />
      </div>
    </PairGate>
  );
}
