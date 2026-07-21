import { useEffect } from "react";
import { api, post } from "../../lib/api";
import { ws } from "../../lib/ws";
import { getDeviceId, getDeviceName } from "../../lib/device";
import { useDropStore } from "../../stores/dropStore";
import type { DropIncoming, DropPeer } from "../../lib/types";

const HELLO_INTERVAL_MS = 15000;

/**
 * Rende il dispositivo visibile agli altri (hello periodico) e resta in ascolto
 * dei file/testo in arrivo — attivo per tutta la vita della UI, così puoi
 * ricevere anche mentre sei su un'altra sezione.
 *
 * Oltre al proprio canale, si sottoscrive anche a quello dell'hub di QUESTO
 * server (id stabile, indipendente dal browser): è il canale su cui arrivano
 * i file/testo proxati da un altro computer scoperto in LAN, che non passano
 * per un hello di questo browser.
 */
export function usePresence() {
  const setPeers = useDropStore((s) => s.setPeers);
  const addIncoming = useDropStore((s) => s.addIncoming);

  useEffect(() => {
    const deviceId = getDeviceId();
    let stopped = false;
    let unsubHub: (() => void) | null = null;

    api<{ hubId: string }>("/api/drop/self").then((r) => {
      if (!stopped && r.ok) {
        unsubHub = ws.subscribe(`drop:${r.data.hubId}`, (event) => {
          addIncoming(event.payload as DropIncoming);
        });
      }
    });

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
      unsubHub?.();
    };
  }, [setPeers, addIncoming]);
}
