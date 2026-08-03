import { Tabs, usePageTab, type TabDef } from "../../components/Tabs";
import { Clipboard } from "../clipboard/Clipboard";
import { Launch } from "../launch/Launch";
import { Calc } from "../calc/Calc";
import { Color } from "../color/Color";
import { Cron } from "../cron/Cron";
import { Compare } from "../compare/Compare";
import { AntiIdlePanel } from "./AntiIdlePanel";
import { ToolsPanel } from "../settings/ToolsPanel";

// Appunti è il tab di default (primo, apre subito). Gli id combaciano con le
// vecchie sezioni, così tray e deep-link (#/clipboard, …) restano validi.
const TABS: TabDef[] = [
  { id: "clipboard", label: "📋 Appunti" },
  { id: "launch", label: "🚀 Avvii" },
  { id: "calc", label: "🧮 Calcolatrice" },
  { id: "color", label: "🎨 Colorimetro" },
  { id: "cron", label: "⏱ Cron" },
  { id: "compare", label: "🔀 Confronta cartelle" },
  { id: "antiidle", label: "🕒 Anti-inattività" },
  { id: "tools", label: "🔧 Strumenti" },
];

export function Tool() {
  const [tab, setTab] = usePageTab(
    "tool",
    TABS.map((t) => t.id),
    "clipboard",
  );

  return (
    <div className="tool-page">
      <div className="tool-tabbar">
        <Tabs tabs={TABS} active={tab} onChange={setTab} />
      </div>
      {tab === "clipboard" && <Clipboard />}
      {tab === "launch" && <Launch />}
      {tab === "calc" && <Calc />}
      {tab === "color" && <Color />}
      {tab === "cron" && <Cron />}
      {tab === "compare" && <Compare />}
      {tab === "antiidle" && <AntiIdlePanel />}
      {tab === "tools" && <ToolsPanel />}
    </div>
  );
}
