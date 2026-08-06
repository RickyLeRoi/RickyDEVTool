const ID_KEY = "rdt-device-id";
const NAME_KEY = "rdt-device-name";
const SECRET_KEY = "rdt-device-secret";

// 20260806 ++ RG #Security il deviceId è pubblico: a dimostrare che il canale drop: è nostro è il
// segreto, che non esce mai dal device — serve entropia vera, non Math.random. I due nascono
// insieme: il server lega l'id al segreto che l'ha rivendicato per primo.
function identity(): { id: string; secret: string } {
  const id = localStorage.getItem(ID_KEY);
  const secret = localStorage.getItem(SECRET_KEY);
  if (id && secret) return { id, secret };

  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  const fresh = {
    id: "dev-" + Math.random().toString(36).slice(2, 10) + Date.now().toString(36),
    secret: Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join(""),
  };
  localStorage.setItem(ID_KEY, fresh.id);
  localStorage.setItem(SECRET_KEY, fresh.secret);
  return fresh;
}

export function getDeviceId(): string {
  return identity().id;
}

export function getDeviceSecret(): string {
  return identity().secret;
}

export function getDeviceName(): string {
  const stored = localStorage.getItem(NAME_KEY);
  if (stored) return stored;
  return guessName();
}

export function setDeviceName(name: string) {
  localStorage.setItem(NAME_KEY, name.trim().slice(0, 40) || guessName());
}

function guessName(): string {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return "iPhone";
  if (/iPad/.test(ua)) return "iPad";
  if (/Android/.test(ua)) return "Android";
  if (/Macintosh/.test(ua)) return "Mac";
  if (/Windows/.test(ua)) return "Windows";
  return "Dispositivo";
}
