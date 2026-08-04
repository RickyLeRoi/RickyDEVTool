import { useEffect, useState } from "react";
import { useNavStore, type Page } from "../stores/navStore";

export interface TabDef {
  id: string;
  label: string;
}

export function Tabs({
  tabs,
  active,
  onChange,
}: {
  tabs: TabDef[];
  active: string;
  onChange: (id: string) => void;
}) {
  return (
    <div className="segmented tabs">
      {tabs.map((t) => (
        <button
          key={t.id}
          className={active === t.id ? "active" : ""}
          onClick={() => onChange(t.id)}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

export function usePageTab(page: Page, tabIds: string[], defaultTab: string) {
  const [active, setActive] = useState(defaultTab);
  const current = useNavStore((s) => s.page);
  const requested = useNavStore((s) => s.tab);
  const seq = useNavStore((s) => s.seq);

  useEffect(() => {
    if (current === page && requested && tabIds.includes(requested)) {
      setActive(requested);
    }
    // 20260704 RG seq nelle deps: riseleziona anche se il tab richiesto è già quello attivo.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current, requested, seq, page]);

  return [active, setActive] as const;
}
