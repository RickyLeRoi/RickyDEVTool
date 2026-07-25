import { useEffect, useState } from "react";
import { useNavStore, type Page } from "../stores/navStore";

export interface TabDef {
  id: string;
  label: string;
}

/** Barra di tab (stile `segmented`, coerente con dashboard/rete). */
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

/**
 * Tab attivo di una pagina, con default e sincronizzazione con la richiesta di
 * navigazione (tray / deep-link / command palette): quando si arriva su questa
 * pagina con un `tab` valido, quel tab viene selezionato. `seq` fa sì che anche
 * una richiesta verso lo stesso tab riattivi eventuali azioni one-shot.
 */
export function usePageTab(page: Page, tabIds: string[], defaultTab: string) {
  const [active, setActive] = useState(defaultTab);
  const current = useNavStore((s) => s.page);
  const requested = useNavStore((s) => s.tab);
  const seq = useNavStore((s) => s.seq);

  useEffect(() => {
    if (current === page && requested && tabIds.includes(requested)) {
      setActive(requested);
    }
    // seq nelle deps: riseleziona anche se il tab richiesto è già quello attivo.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current, requested, seq, page]);

  return [active, setActive] as const;
}
