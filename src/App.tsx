import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { PairGate } from "./app/PairGate";
import { VitalsPanel } from "./app/VitalsPanel";
import { Dashboard } from "./features/dashboard/Dashboard";
import { Projects } from "./features/projects/Projects";
import { RickyAI } from "./features/rickyai/RickyAI";
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
import { hideToTray, isTauri } from "./lib/appWindow";
import { NAV_SECTIONS, QUICK_NAV, THEMES } from "./lib/constants";
import { DEFAULT_PAGE } from "./lib/defaults";
import { useNavStore, type Page } from "./stores/navStore";
import { useStatsStore } from "./stores/statsStore";
import { useDisksStore } from "./stores/disksStore";
import { useDropStore } from "./stores/dropStore";
import { useTrayIntentStore } from "./stores/trayIntentStore";
import { useTasksStore } from "./stores/tasksStore";
import type { AiStatus, DiskInfo, MachineStats, TaskInfo } from "./lib/types";

function idFromHash(): string | null {
  const m = window.location.hash.match(/^#\/([a-z]+)/);
  return m ? m[1] : null;
}

export default function App() {
  const { t } = useTranslation();
  const page = useNavStore((s) => s.page);
  const go = useNavStore((s) => s.go);
  const push = useStatsStore((s) => s.push);
  const setError = useStatsStore((s) => s.setError);
  const setDisks = useDisksStore((s) => s.setDisks);
  const peerCount = useDropStore((s) => s.peers.length);
  const setTasks = useTasksStore((s) => s.setTasks);
  const taskCount = useTasksStore((s) => s.tasks.length);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [aiEnabled, setAiEnabled] = useState<boolean | null>(null);

  usePresence();

  useEffect(() => {
    const id = idFromHash();
    if (id) go(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    api<{ tasks: TaskInfo[] }>("/api/tasks").then((r) => {
      if (r.ok) setTasks(r.data.tasks ?? []);
    });
    return ws.subscribe("tasks", (event) => {
      if (event.topic === "tasks") setTasks((event.payload as { tasks?: TaskInfo[] }).tasks ?? []);
    });
  }, [setTasks]);

  useEffect(() => {
    const load = () => {
      api<AiStatus>("/api/ai/status").then((r) => {
        if (r.ok) setAiEnabled(r.data.enabled === true);
      });
    };
    load();
    return ws.subscribe("ai", load);
  }, []);

  useEffect(() => {
    if (aiEnabled === false && page === "rickyai") go(DEFAULT_PAGE);
  }, [aiEnabled, page, go]);

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

  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    import("@tauri-apps/api/event").then(({ listen }) =>
      listen<{ section: string; extra?: string | null }>("tray-navigate", (event) => {
        const { section, extra } = event.payload;
        go(section, extra ?? null);
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

  useEffect(() => {
    return ws.subscribe("stats", (event) => {
      if (event.topic === "stats") push(event.payload as MachineStats);
      else if (event.topic === "stats:error")
        setError((event.payload as { message: string }).message);
    });
  }, [push, setError]);

  useEffect(() => {
    if (page !== "dashboard") return;
    return ws.subscribe("disks", (event) => {
      if (event.topic === "disks")
        setDisks((event.payload as { disks: DiskInfo[] }).disks);
    });
  }, [page, setDisks]);

  const isVisible = (s: (typeof NAV_SECTIONS)[number]) => {
    if (s.id === "tasks") return taskCount > 0;
    if (s.id === "rickyai") return aiEnabled === true;
    return true;
  };

  const commands = useMemo<Command[]>(() => {
    const nav: Command[] = NAV_SECTIONS.filter(isVisible).map((s) => ({
      id: `nav:${s.id}`,
      title: t(`nav.${s.id}`),
      hint: t("common.goTo"),
      icon: s.icon,
      run: () => go(s.id),
    }));
    const quick: Command[] = QUICK_NAV.map((q) => ({
      id: `quick:${q.id}`,
      title: t(`quickNav.${q.id}`),
      hint: t("common.open"),
      icon: q.icon,
      keywords: "tool",
      run: () => go(q.id),
    }));
    const themes: Command[] = THEMES.map((key) => ({
      id: `theme:${key}`,
      title: t("theme.label", { name: t(`theme.${key}`) }),
      hint: t("theme.appearance"),
      icon: "🌓",
      run: () => applyTheme(key, true),
    }));
    const windowCmds: Command[] = isTauri
      ? [
          {
            id: "window:tray",
            title: t("nav.minimizeToTray"),
            hint: t("windowPanel.title"),
            icon: "🔽",
            run: () => void hideToTray(),
          },
        ]
      : [];
    return [...nav, ...quick, ...themes, ...windowCmds];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [go, taskCount, aiEnabled, t]);

  const railDot = (id: Page): { count: number; title: string } | undefined => {
    if (id === "drop")
      return { count: peerCount, title: t("nav.devicesOnline", { count: peerCount }) };
    if (id === "tasks") return { count: taskCount, title: t("nav.taskCount", { count: taskCount }) };
    return undefined;
  };

  const railButton = (s: (typeof NAV_SECTIONS)[number]) => {
    const dot = railDot(s.id);
    const showDot = (dot?.count ?? 0) > 0;
    const label = t(`nav.${s.id}`);
    return (
      <button
        key={s.id}
        className={`rail-btn ${page === s.id ? "active" : ""}`}
        title={showDot ? `${label} (${dot?.count})` : label}
        onClick={() => go(s.id)}
      >
        <span className="rail-icon">
          {s.icon}
          {showDot && <span className="rail-dot" title={dot?.title} />}
        </span>
        <span className="rail-label">{label}</span>
      </button>
    );
  };

  return (
    <PairGate>
      <div className="shell">
        <nav className="rail">
          {NAV_SECTIONS.filter((s) => s.position === "top" && isVisible(s)).map(railButton)}
          <div className="rail-spacer" />
          {NAV_SECTIONS.filter((s) => s.position === "bottom" && isVisible(s)).map(railButton)}
          {isTauri && (
            <button
              className="rail-btn"
              title={t("nav.minimizeToTray")}
              onClick={() => void hideToTray()}
            >
              <span className="rail-icon">🔽</span>
              <span className="rail-label">{t("nav.minimizeToTray")}</span>
            </button>
          )}
        </nav>

        <main className="main">
          {page === "dashboard" && <Dashboard />}
          {page === "projects" && <Projects />}
          {page === "rickyai" && aiEnabled && <RickyAI />}
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
