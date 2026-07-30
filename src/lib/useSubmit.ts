import { useState } from "react";
import type { ApiError, ApiResult } from "./types";

/// Ciclo di vita di un'azione che chiama il backend: alza `busy`, azzera
/// l'errore precedente, esegue, e riporta l'esito. Ogni dialog se lo riscriveva
/// identico; averlo qui significa che "disabilita i pulsanti mentre invii" e
/// "non lasciare a schermo l'errore del tentativo prima" valgono ovunque.
export function useSubmit() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);

  /// Esegue `call`; su successo invoca `onOk`. Restituisce comunque il risultato
  /// grezzo, così chi ha bisogno di ispezionare l'errore (per offrire un
  /// ritenta, o una variante forzata) può farlo senza duplicare lo stato.
  async function run<T>(
    call: () => Promise<ApiResult<T>>,
    onOk?: (data: T) => void,
  ): Promise<ApiResult<T>> {
    setBusy(true);
    setError(null);
    const result = await call();
    setBusy(false);
    if (result.ok) onOk?.(result.data);
    else setError(result.error);
    return result;
  }

  return { busy, error, setError, run };
}
