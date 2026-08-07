import {
  DEVICE_NAME_BY_UA,
  DEVICE_NAME_MAX_CHARS,
  DEVICE_SECRET_BYTES,
  STORAGE_KEYS,
} from "./constants";
import { DEFAULT_DEVICE_NAME } from "./defaults";

// 20260806 ++ RG #Security il deviceId è pubblico: a dimostrare che il canale drop: è nostro è il
// segreto, che non esce mai dal device — serve entropia vera, non Math.random. I due nascono
// insieme: il server lega l'id al segreto che l'ha rivendicato per primo.
function identity(): { id: string; secret: string } {
  const id = localStorage.getItem(STORAGE_KEYS.deviceId);
  const secret = localStorage.getItem(STORAGE_KEYS.deviceSecret);
  if (id && secret) return { id, secret };

  const bytes = new Uint8Array(DEVICE_SECRET_BYTES);
  crypto.getRandomValues(bytes);
  const fresh = {
    id: "dev-" + Math.random().toString(36).slice(2, 10) + Date.now().toString(36),
    secret: Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join(""),
  };
  localStorage.setItem(STORAGE_KEYS.deviceId, fresh.id);
  localStorage.setItem(STORAGE_KEYS.deviceSecret, fresh.secret);
  return fresh;
}

export function getDeviceId(): string {
  return identity().id;
}

export function getDeviceSecret(): string {
  return identity().secret;
}

export function getDeviceName(): string {
  const stored = localStorage.getItem(STORAGE_KEYS.deviceName);
  if (stored) return stored;
  return guessName();
}

export function setDeviceName(name: string) {
  localStorage.setItem(
    STORAGE_KEYS.deviceName,
    name.trim().slice(0, DEVICE_NAME_MAX_CHARS) || guessName(),
  );
}

function guessName(): string {
  const ua = navigator.userAgent;
  return DEVICE_NAME_BY_UA.find((d) => d.pattern.test(ua))?.name ?? DEFAULT_DEVICE_NAME;
}
