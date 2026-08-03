import { useEffect, useMemo, useState } from "react";
import { PairGate } from "./app/PairGate";
import { VitalsPanel } from "./app/VitalsPanel";
import { Dashboard } from "./features/dashboard/Dashboard";
import { Projects } from "./features/projects/Projects";
import { Settings } from "./features/settings/Settings";
import { Drop } from "./features/drop/Drop";
import { NetTools } from "./features/nettools/NetTools";
import { Tool } from "./features/tool/Tool";
import { LogViewer } from "./features/log/LogViewer";
import { Snippets } from "./features/snippets/Snippets";
import { Ssh } from "./features/ssh/Ssh";
import { Tasks } from "./features/tasks/Tasks";
import { About } from "./features/about/About";
import { DropToasts } from "./features/drop/DropToasts";
import { UpdateBanner } from "./features/update/UpdateBanner";
import { CommandPalette, type Command } from "./features/command/CommandPalette";
import { usePresence } from "./features/drop/usePresence";
import { ws } from "./lib/ws";
import { api } from "./lib/api";
import { applyTheme } from "./lib/theme";
import { useNavStore, type Page } from "./stores/navStore";
import { useStatsStore } from "./stores/statsStore";
import { useDisksStore } from "./stores/disksStore";
import { useDropStore } from "./stores/dropStore";
import { useTrayIntentStore } from "./stores/trayIntentStore";
import { useTasksStore } from "./stores/tasksStore";
import type { DiskInfo, MachineStats, TaskInfo } from "./lib/types";

const SECTIONS: { id: Page; icon: string; label: string; position: "top" | "bottom" }[] = [
  { id: "dashboard", icon: "🖥", label: "Dashboard", position: "top" },
  { id: "projects", icon: "📁", label: "Progetti", position: "top" },
  { id: "net", icon: "🌐", label: "Rete", position: "top" },
  { id: "tool", icon: "🧰", label: "Tool", position: "top" },
  { id: "log", icon: "📜", label: "Log", position: "top" },
  { id: "snippets", icon: "⌨️", label: "Snippet", position: "top" },
  { id: "ssh", icon: "🔑", label: "SSH", position: "top" },
  { id: "drop", icon: "📤", label: "Drop", position: "top" },
  { id: "tasks", icon: "🧾", label: "Task", position: "bottom" },
  { id: "about", icon: "ℹ️", label: "About", position: "bottom" },
  { id: "settings", icon: "⚙️", label: "Impostazioni", position: "bottom" },
];

// Azioni rapide extra offerte dalla palette, oltre alla navigazione: aprono
// direttamente un tab specifico (id storico risolto dallo store di navigazione).
const QUICK_NAV: { id: string; title: string; icon: string }[] = [
  { id: "ports", title: "Porte in ascolto", icon: "🔌" },
  { id: "docker", title: "Docker", icon: "🐳" },
  { id: "clipboard", title: "Appunti", icon: "📋" },
  { id: "launch", title: "Avvii compositi", icon: "🚀" },
  { id: "calc", title: "Calcolatrice", icon: "🧮" },
  { id: "color", title: "Colorimetro", icon: "🎨" },
  { id: "compare", title: "Confronta cartelle", icon: "🔀" },
  { id: "services", title: "Servizi (ping)", icon: "📡" },
];

// Deep-link/hash: #/<id>. Ignora `#pair=…` (gestito da PairGate).
function idFromHash(): string | null {
  const m = window.location.hash.match(/^#\/([a-z]+)/);
  return m ? m[1] : null;
}

export default function App() {
  const page = useNavStore((s) => s.page);
  const go = useNavStore((s) => s.go);
  const push = useStatsStore((s) => s.push);
  const setError = useStatsStore((s) => s.setError);
  const setDisks = useDisksStore((s) => s.setDisks);
  const peerCount = useDropStore((s) => s.peers.length);
  const setTasks = useTasksStore((s) => s.setTasks);
  const taskCount = useTasksStore((s) => s.tasks.length);
  const [paletteOpen, setPaletteOpen] = useState(false);

  // Presenza drop attiva sempre: sei visibile e ricevi da qualsiasi sezione.
  usePresence();

  // All'avvio: se l'URL ha un deep-link, ha la precedenza sull'ultima pagina.
  useEffect(() => {
    const id = idFromHash();
    if (id) go(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Conteggio task sempre monitorato: decide se mostrare la voce Task nella rail.
  useEffect(() => {
    api<{ tasks: TaskInfo[] }>("/api/tasks").then((r) => {
      if (r.ok) setTasks(r.data.tasks ?? []);
    });
    return ws.subscribe("tasks", (event) => {
      if (event.topic === "tasks") setTasks((event.payload as { tasks?: TaskInfo[] }).tasks ?? []);
    });
  }, [setTasks]);

  // ⌘K / Ctrl+K apre/chiude la command palette.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Click su una voce del menu del tray: naviga sulla sezione giusta.
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ section: string; extra?: string | null }>("tray-navigate", (event) => {
        const { section, extra } = event.payload;
        go(section, extra ?? null);
        // Alcune sezioni consumano l'"extra" via trayIntent (es. QR in Impostazioni).
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
  }, [go]);

  // Deep-link: tiene l'hash allineato alla pagina e reagisce ai cambi manuali.
  useEffect(() => {
    if (idFromHash() !== page) {
      history.replaceState(null, "", `#/${page}`);
    }
    const onHashChange = () => {
      const id = idFromHash();
      if (id) go(id);
    };
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, [page, go]);

  // Il pannello vital signs è sempre visibile: il topic "stats" resta sottoscritto.
  useEffect(() => {
    return ws.subscribe("stats", (event) => {
      if (event.topic === "stats") push(event.payload as MachineStats);
      else if (event.topic === "stats:error")
        setError((event.payload as { message: string }).message);
    });
  }, [push, setError]);

  // I dischi si aggiornano solo mentre la dashboard è aperta.
  useEffect(() => {
    if (page !== "dashboard") return;
    return ws.subscribe("disks", (event) => {
      if (event.topic === "disks")
        setDisks((event.payload as { disks: DiskInfo[] }).disks);
    });
  }, [page, setDisks]);

  // Comandi della palette: navigazione + azioni rapide + tema.
  const commands = useMemo<Command[]>(() => {
    const nav: Command[] = SECTIONS.map((s) => ({
      id: `nav:${s.id}`,
      title: s.label,
      hint: "Vai a",
      icon: s.icon,
      run: () => go(s.id),
    }));
    const quick: Command[] = QUICK_NAV.map((q) => ({
      id: `quick:${q.id}`,
      title: q.title,
      hint: "Apri",
      icon: q.icon,
      keywords: "tool",
      run: () => go(q.id),
    }));
    const themes: Command[] = [
      { key: "auto", label: "Auto (sistema)" },
      { key: "light", label: "Chiaro" },
      { key: "dark", label: "Scuro" },
    ].map((t) => ({
      id: `theme:${t.key}`,
      title: `Tema: ${t.label}`,
      hint: "Aspetto",
      icon: "🌓",
      run: () => applyTheme(t.key as "auto" | "light" | "dark", true),
    }));
    return [...nav, ...quick, ...themes];
  }, [go]);

  const railDot = (id: Page): { count: number; title: string } | undefined => {
    if (id === "drop") return { count: peerCount, title: `${peerCount} dispositivi online` };
    if (id === "tasks") return { count: taskCount, title: `${taskCount} task` };
    return undefined;
  };

  // Task è l'unica voce a comparsa condizionata: senza task non viene mostrata.
  const isVisible = (s: (typeof SECTIONS)[number]) => s.id !== "tasks" || taskCount > 0;

  const railButton = (s: (typeof SECTIONS)[number]) => {
    const dot = railDot(s.id);
    const showDot = (dot?.count ?? 0) > 0;
    return (
      <button
        key={s.id}
        className={`rail-btn ${page === s.id ? "active" : ""}`}
        title={showDot ? `${s.label} (${dot?.count})` : s.label}
        onClick={() => go(s.id)}
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
          <div className="rail-spacer" />
          {SECTIONS.filter((s) => s.position === "bottom" && isVisible(s)).map(railButton)}
        </nav>

        <main className="main">
          {page === "dashboard" && <Dashboard />}
          {page === "projects" && <Projects />}
          {page === "net" && <NetTools />}
          {page === "tool" && <Tool />}
          {page === "log" && <LogViewer />}
          {page === "snippets" && <Snippets />}
          {page === "ssh" && <Ssh />}
          {page === "drop" && <Drop />}
          {page === "tasks" && <Tasks />}
          {page === "about" && <About />}
          {page === "settings" && <Settings />}
        </main>

        <VitalsPanel onNavigate={(s) => go(s)} />
        <DropToasts />
        <UpdateBanner />
        <CommandPalette
          open={paletteOpen}
          onClose={() => setPaletteOpen(false)}
          commands={commands}
        />
      </div>
    </PairGate>
  );
}
