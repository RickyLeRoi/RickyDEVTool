import { useEffect, useState } from "react";
import { PairGate } from "./app/PairGate";
import { VitalsPanel } from "./app/VitalsPanel";
import { Dashboard } from "./features/dashboard/Dashboard";
import { Ports } from "./features/ports/Ports";
import { Projects } from "./features/projects/Projects";
import { Services } from "./features/services/Services";
import { Settings } from "./features/settings/Settings";
import { Drop } from "./features/drop/Drop";
import { NetTools } from "./features/nettools/NetTools";
import { Docker } from "./features/docker/Docker";
import { Tasks } from "./features/tasks/Tasks";
import { Calc } from "./features/calc/Calc";
import { Color } from "./features/color/Color";
import { Clipboard } from "./features/clipboard/Clipboard";
import { About } from "./features/about/About";
import { Launch } from "./features/launch/Launch";
import { DropToasts } from "./features/drop/DropToasts";
import { UpdateBanner } from "./features/update/UpdateBanner";
import { usePresence } from "./features/drop/usePresence";
import { ws } from "./lib/ws";
import { api } from "./lib/api";
import { useStatsStore } from "./stores/statsStore";
import { useDisksStore } from "./stores/disksStore";
import { useDropStore } from "./stores/dropStore";
import { useTrayIntentStore } from "./stores/trayIntentStore";
import { useTasksStore } from "./stores/tasksStore";
import type { DiskInfo, MachineStats, TaskInfo } from "./lib/types";

type Section =
  | "dashboard"
  | "ports"
  | "projects"
  | "services"
  | "net"
  | "docker"
  | "launch"
  | "calc"
  | "color"
  | "clipboard"
  | "drop"
  | "tasks"
  | "about"
  | "settings";


const SECTIONS: { id: Section; icon: string; label: string; position: "top" | "bottom" }[] = [
  { id: "dashboard", icon: "🖥", label: "Dashboard", position: "top" },
  { id: "ports", icon: "🔌", label: "Porte", position: "top" },
  { id: "projects", icon: "📁", label: "Progetti", position: "top" },
  { id: "services", icon: "📡", label: "Servizi", position: "top" },
  { id: "net", icon: "🌐", label: "Rete", position: "top" },
  { id: "docker", icon: "🐳", label: "Docker", position: "top" },
  { id: "launch", icon: "🚀", label: "Avvii", position: "top" },
  { id: "calc", icon: "🧮", label: "Calcolatrice", position: "top" },
  { id: "color", icon: "🎨", label: "Colori", position: "top" },
  { id: "clipboard", icon: "📋", label: "Appunti", position: "top" },
  { id: "drop", icon: "📤", label: "Drop", position: "top" },
  { id: "tasks", icon: "🧾", label: "Task", position: "bottom" },
  { id: "about", icon: "ℹ️", label: "About", position: "bottom" },
  { id: "settings", icon: "⚙️", label: "Impostazioni", position: "bottom" },
];

const SECTION_IDS = new Set<string>(SECTIONS.map((s) => s.id));

// URL tipo http://<ip>:6969/#/services). Ignora `#pair=…` (gestito da PairGate).
function sectionFromHash(): Section | null {
  const m = window.location.hash.match(/^#\/([a-z]+)/);
  return m && SECTION_IDS.has(m[1]) ? (m[1] as Section) : null;
}

export default function App() {
  const [section, setSection] = useState<Section>(() => sectionFromHash() ?? "dashboard");
  const push = useStatsStore((s) => s.push);
  const setError = useStatsStore((s) => s.setError);
  const setDisks = useDisksStore((s) => s.setDisks);
  const peerCount = useDropStore((s) => s.peers.length);
  const setTasks = useTasksStore((s) => s.setTasks);
  const taskCount = useTasksStore((s) => s.tasks.length);

  // Presenza drop attiva sempre: sei visibile e ricevi da qualsiasi sezione.
  usePresence();

  // Conteggio task sempre monitorato: decide se mostrare la voce Task nella rail.
  // Il topic "tasks" è a eventi (spawn/uscita/pulizia), non un poller.
  useEffect(() => {
    api<{ tasks: TaskInfo[] }>("/api/tasks").then((r) => {
      if (r.ok) setTasks(r.data.tasks ?? []);
    });
    return ws.subscribe("tasks", (event) => {
      if (event.topic === "tasks") setTasks((event.payload as { tasks?: TaskInfo[] }).tasks ?? []);
    });
  }, [setTasks]);

  // Click su una voce del menu del tray: naviga sulla sezione giusta.
  // Il bridge Tauri non esiste quando la pagina è aperta da un browser
  // normale (telefono/LAN): l'evento semplicemente non arriva mai.
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ section: Section; extra?: string | null }>("tray-navigate", (event) => {
        const { section, extra } = event.payload;
        setSection(section);
        useTrayIntentStore.getState().apply(section, extra ?? null);
      }),
    ).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Deep-link
  useEffect(() => {
    if (sectionFromHash() !== section) {
      history.replaceState(null, "", `#/${section}`);
    }
    const onHashChange = () => {
      const next = sectionFromHash();
      if (next) setSection(next);
    };
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, [section]);

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

  // Pallino/badge delle voci che ne hanno uno: peer drop online, task attivi.
  const railDot = (id: Section): { count: number; title: string } | undefined => {
    if (id === "drop") return { count: peerCount, title: `${peerCount} dispositivi online` };
    if (id === "tasks") return { count: taskCount, title: `${taskCount} task` };
    return undefined;
  };

  // Task è l'unica voce a comparsa condizionata: senza task nella sessione non
  // viene mostrata.
  const isVisible = (s: (typeof SECTIONS)[number]) => s.id !== "tasks" || taskCount > 0;

  // Un pulsante della rail. `dot` accende il pallino/badge e, quando presente,
  // porta il conteggio anche nel tooltip.
  const railButton = (s: (typeof SECTIONS)[number]) => {
    const dot = railDot(s.id);
    const showDot = (dot?.count ?? 0) > 0;
    return (
      <button
        key={s.id}
        className={`rail-btn ${section === s.id ? "active" : ""}`}
        title={showDot ? `${s.label} (${dot?.count})` : s.label}
        onClick={() => setSection(s.id)}
      >
        <span className="rail-icon">
          {s.icon}
          {showDot && <span className="rail-dot" title={dot?.title} />}
        </span>
        <span className="rail-label">{s.label}</span>
      </button>
    );
  };

  return (
    <PairGate>
      <div className="shell">
        <nav className="rail">
          {SECTIONS.filter((s) => s.position === "top").map(railButton)}
          {/* Spinge il gruppo in fondo (Task/About/Impostazioni) verso il basso. */}
          <div className="rail-spacer" />
          {SECTIONS.filter((s) => s.position === "bottom" && isVisible(s)).map(railButton)}
        </nav>

        <main className="main">
          {section === "dashboard" && <Dashboard />}
          {section === "ports" && <Ports />}
          {section === "projects" && <Projects />}
          {section === "services" && <Services />}
          {section === "net" && <NetTools />}
          {section === "docker" && <Docker />}
          {section === "launch" && <Launch />}
          {section === "calc" && <Calc />}
          {section === "color" && <Color />}
          {section === "clipboard" && <Clipboard />}
          {section === "drop" && <Drop />}
          {section === "tasks" && <Tasks />}
          {section === "about" && <About />}
          {section === "settings" && <Settings />}
        </main>

        <VitalsPanel onNavigate={(s) => setSection(s as Section)} />
        <DropToasts />
        <UpdateBanner />
      </div>
    </PairGate>
  );
}
