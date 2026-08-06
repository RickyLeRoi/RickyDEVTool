import { useEffect } from "react";
import { api, post } from "../../lib/api";
import { ws } from "../../lib/ws";
import { getDeviceId, getDeviceName, getDeviceSecret } from "../../lib/device";
import { useDropStore } from "../../stores/dropStore";
import type { DropIncoming, DropPeer } from "../../lib/types";

const HELLO_INTERVAL_MS = 15000;

export function usePresence() {
  const setPeers = useDropStore((s) => s.setPeers);
  const addIncoming = useDropStore((s) => s.addIncoming);

  useEffect(() => {
    const deviceId = getDeviceId();
    const deviceSecret = getDeviceSecret();
    let stopped = false;
    let unsubHub: (() => void) | null = null;

    const handleIncoming = (data: DropIncoming) => {
      if (data.kind === "clipboard") {
        post("/api/clipboard/record", { text: data.text });
      }
      addIncoming(data);
    };

    // 20260806 ++ RG #Drop il canale dell'hub raccoglie i drop dagli altri PC ed è del
    // desktop: dal telefono il server lo nega, quindi non lo sottoscriviamo nemmeno.
    api<{ hubId: string; isDesktop: boolean }>("/api/drop/self").then((r) => {
      if (!stopped && r.ok && r.data.isDesktop) {
        unsubHub = ws.subscribe(`drop:${r.data.hubId}`, (event) => {
          handleIncoming(event.payload as DropIncoming);
        });
      }
    });

    // il canale personale si sottoscrive dopo il primo hello: prima il server non conosce
    // ancora la rivendicazione e negherebbe. Gli hello successivi la riaffermano, così un
    // riavvio del server non lascia il device muto.
    let unsubMine: (() => void) | null = null;
    const hello = async () => {
      const r = await post<{ peers: DropPeer[] }>("/api/drop/hello", {
        deviceId,
        deviceSecret,
        name: getDeviceName(),
      });
      if (stopped || !r.ok) return;
      setPeers(r.data.peers);
      if (unsubMine) {
        ws.resubscribe(`drop:${deviceId}`);
      } else {
        unsubMine = ws.subscribe(
          `drop:${deviceId}`,
          (event) => handleIncoming(event.payload as DropIncoming),
          deviceSecret,
        );
      }
    };
    hello();
    const timer = setInterval(hello, HELLO_INTERVAL_MS);

    const unsubPeers = ws.subscribe("drop-peers", (event) => {
      const all = (event.payload as { peers: DropPeer[] }).peers;
      setPeers(all.filter((p) => p.deviceId !== deviceId));
    });

    return () => {
      stopped = true;
      clearInterval(timer);
      unsubPeers();
      unsubMine?.();
      unsubHub?.();
    };
  }, [setPeers, addIncoming]);
}
