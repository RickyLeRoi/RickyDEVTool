import { Tabs, usePageTab, type TabDef } from "../../components/Tabs";
import { Clipboard } from "../clipboard/Clipboard";
import { Launch } from "../launch/Launch";
import { Calc } from "../calc/Calc";
import { Color } from "../color/Color";
import { Cron } from "../cron/Cron";
import { Compare } from "../compare/Compare";
import { ToolsPanel } from "../settings/ToolsPanel";

const TABS: TabDef[] = [
  { id: "clipboard", label: "📋 Appunti" },
  { id: "launch", label: "🚀 Avvii" },
  { id: "calc", label: "🧮 Calcolatrice" },
  { id: "color", label: "🎨 Colorimetro" },
  { id: "cron", label: "⏱ Cron" },
  { id: "compare", label: "🔀 Confronta cartelle" },
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
      {tab === "tools" && <ToolsPanel />}
    </div>
  );
}
