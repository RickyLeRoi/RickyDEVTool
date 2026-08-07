import { useTranslation } from "react-i18next";
import { Tabs, usePageTab, type TabDef } from "../../components/Tabs";
import { Clipboard } from "../clipboard/Clipboard";
import { Launch } from "../launch/Launch";
import { Calc } from "../calc/Calc";
import { Color } from "../color/Color";
import { Cron } from "../cron/Cron";
import { Compare } from "../compare/Compare";
import { ToolsPanel } from "../settings/ToolsPanel";
import { TOOL_TAB_IDS } from "../../lib/constants";
import { DEFAULT_TOOL_TAB } from "../../lib/defaults";

export function Tool() {
  const { t } = useTranslation();
  const [tab, setTab] = usePageTab("tool", [...TOOL_TAB_IDS], DEFAULT_TOOL_TAB);

  const tabs: TabDef[] = TOOL_TAB_IDS.map((id) => ({ id, label: t(`tool.tabs.${id}`) }));

  return (
    <div className="tool-page">
      <div className="tool-tabbar">
        <Tabs tabs={tabs} active={tab} onChange={setTab} />
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
