import { useState } from "react";
import type { ApiError, ApiResult } from "./types";

export function useSubmit() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);

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
