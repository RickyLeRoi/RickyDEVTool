// Identità del dispositivo per il file drop: id stabile + nome modificabile.

const ID_KEY = "rdt-device-id";
const NAME_KEY = "rdt-device-name";

export function getDeviceId(): string {
  let id = localStorage.getItem(ID_KEY);
  if (!id) {
    id = "dev-" + Math.random().toString(36).slice(2, 10) + Date.now().toString(36);
    localStorage.setItem(ID_KEY, id);
  }
  return id;
}

export function getDeviceName(): string {
  const stored = localStorage.getItem(NAME_KEY);
  if (stored) return stored;
  return guessName();
}

export function setDeviceName(name: string) {
  localStorage.setItem(NAME_KEY, name.trim().slice(0, 40) || guessName());
}

/** Nome di default dedotto dallo user agent. */
function guessName(): string {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return "iPhone";
  if (/iPad/.test(ua)) return "iPad";
  if (/Android/.test(ua)) return "Android";
  if (/Macintosh/.test(ua)) return "Mac";
  if (/Windows/.test(ua)) return "Windows";
  return "Dispositivo";
}
