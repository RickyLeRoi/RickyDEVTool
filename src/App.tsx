import { useEffect, useState } from "react";
import { PairGate } from "./app/PairGate";
import { VitalsPanel } from "./app/VitalsPanel";
import { Dashboard } from "./features/dashboard/Dashboard";
import { Ports } from "./features/ports/Ports";
import { Projects } from "./features/projects/Projects";
import { Services } from "./features/services/Services";
import { Settings } from "./features/settings/Settings";
import { Drop } from "./features/drop/Drop";
import { DropToasts } from "./features/drop/DropToasts";
import { usePresence } from "./features/drop/usePresence";
import { ws } from "./lib/ws";
import { useStatsStore } from "./stores/statsStore";
import { useDisksStore } from "./stores/disksStore";
import { useDropStore } from "./stores/dropStore";
import type { DiskInfo, MachineStats } from "./lib/types";

type Section = "dashboard" | "ports" | "projects" | "services" | "drop" | "settings";

const SECTIONS: { id: Section; icon: string; label: string }[] = [
  { id: "dashboard", icon: "🖥", label: "Dashboard" },
  { id: "ports", icon: "🔌", label: "Porte" },
  { id: "projects", icon: "📁", label: "Progetti" },
  { id: "services", icon: "🌐", label: "Servizi" },
  { id: "drop", icon: "📤", label: "Drop" },
  { id: "settings", icon: "⚙️", label: "Impostazioni" },
];

export default function App() {
  const [section, setSection] = useState<Section>("dashboard");
  const push = useStatsStore((s) => s.push);
  const setError = useStatsStore((s) => s.setError);
  const setDisks = useDisksStore((s) => s.setDisks);
  const peerCount = useDropStore((s) => s.peers.length);

  // Presenza drop attiva sempre: sei visibile e ricevi da qualsiasi sezione.
  usePresence();

  // Il pannello vital signs è sempre visibile, quindi il topic "stats"
  // resta sottoscritto per tutta la vita della UI.
  useEffect(() => {
    return ws.subscribe("stats", (event) => {
      if (event.topic === "stats") push(event.payload as MachineStats);
      else if (event.topic === "stats:error")
        setError((event.payload as { message: string }).message);
    });
  }, [push, setError]);

  // I dischi si aggiornano (anche su inserimento/rimozione) solo mentre la
  // dashboard è aperta: il poller backend si spegne quando si esce.
  useEffect(() => {
    if (section !== "dashboard") return;
    return ws.subscribe("disks", (event) => {
      if (event.topic === "disks")
        setDisks((event.payload as { disks: DiskInfo[] }).disks);
    });
  }, [section, setDisks]);

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
              <span className="rail-icon">
                {s.icon}
                {s.id === "drop" && peerCount > 0 && (
                  <span className="rail-dot" title={`${peerCount} dispositivi online`} />
                )}
              </span>
              <span className="rail-label">{s.label}</span>
            </button>
          ))}
        </nav>

        <main className="main">
          {section === "dashboard" && <Dashboard />}
          {section === "ports" && <Ports />}
          {section === "projects" && <Projects />}
          {section === "services" && <Services />}
          {section === "drop" && <Drop />}
          {section === "settings" && <Settings />}
        </main>

        <VitalsPanel />
        <DropToasts />
      </div>
    </PairGate>
  );
}
