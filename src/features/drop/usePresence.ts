import { useEffect } from "react";
import { api, post } from "../../lib/api";
import { ws } from "../../lib/ws";
import { getDeviceId, getDeviceName } from "../../lib/device";
import { useDropStore } from "../../stores/dropStore";
import type { DropIncoming, DropPeer } from "../../lib/types";

const HELLO_INTERVAL_MS = 15000;

export function usePresence() {
  const setPeers = useDropStore((s) => s.setPeers);
  const addIncoming = useDropStore((s) => s.addIncoming);

  useEffect(() => {
    const deviceId = getDeviceId();
    let stopped = false;
    let unsubHub: (() => void) | null = null;

    const handleIncoming = (data: DropIncoming) => {
      if (data.kind === "clipboard") {
        post("/api/clipboard/record", { text: data.text });
      }
      addIncoming(data);
    };

    api<{ hubId: string }>("/api/drop/self").then((r) => {
      if (!stopped && r.ok) {
        unsubHub = ws.subscribe(`drop:${r.data.hubId}`, (event) => {
          handleIncoming(event.payload as DropIncoming);
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
      setPeers(all.filter((p) => p.deviceId !== deviceId));
    });
    const unsubMine = ws.subscribe(`drop:${deviceId}`, (event) => {
      handleIncoming(event.payload as DropIncoming);
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
