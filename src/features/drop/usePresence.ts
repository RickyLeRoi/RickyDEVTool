import { useEffect } from "react";
import { post } from "../../lib/api";
import { ws } from "../../lib/ws";
import { getDeviceId, getDeviceName } from "../../lib/device";
import { useDropStore } from "../../stores/dropStore";
import type { DropIncoming, DropPeer } from "../../lib/types";

const HELLO_INTERVAL_MS = 15000;

/**
 * Rende il dispositivo visibile agli altri (hello periodico) e resta in ascolto
 * dei file/testo in arrivo — attivo per tutta la vita della UI, così puoi
 * ricevere anche mentre sei su un'altra sezione.
 */
export function usePresence() {
  const setPeers = useDropStore((s) => s.setPeers);
  const addIncoming = useDropStore((s) => s.addIncoming);

  useEffect(() => {
    const deviceId = getDeviceId();
    let stopped = false;

    const hello = async () => {
      const r = await post<{ peers: DropPeer[] }>("/api/drop/hello", {
        deviceId,
        name: getDeviceName(),
      });
      if (!stopped && r.ok) setPeers(r.data.peers);
    };
    hello();
    const timer = setInterval(hello, HELLO_INTERVAL_MS);

    const unsubPeers = ws.subscribe("drop-peers", (event) => {
      const all = (event.payload as { peers: DropPeer[] }).peers;
      // Il broadcast include tutti: togli me stesso.
      setPeers(all.filter((p) => p.deviceId !== deviceId));
    });
    const unsubMine = ws.subscribe(`drop:${deviceId}`, (event) => {
      addIncoming(event.payload as DropIncoming);
    });

    return () => {
      stopped = true;
      clearInterval(timer);
      unsubPeers();
      unsubMine();
    };
  }, [setPeers, addIncoming]);
}
