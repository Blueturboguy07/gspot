import { useEffect, useState } from "react";
import { ipc, on } from "../lib/ipc";
import type { Alert } from "../lib/types";
import { GMark } from "../components/icons";

/** The edge tab: a dark pill flush with the screen edge. Click slides the panel out. */
export function Tab() {
  const [count, setCount] = useState(0);
  const [side, setSide] = useState<"left" | "right">("left");
  useEffect(() => {
    ipc.getState().then((s) => {
      setCount(s.alerts.length);
      setSide(s.settings.dock_side === "right" ? "right" : "left");
    }).catch(() => {});
    const u1 = on<Alert[]>("host://alerts", (a) => setCount(a.length));
    return () => { u1.then((f) => f()); };
  }, []);
  return (
    <div className={"tabwin side-" + side}>
      <div className="edgetab" onClick={() => ipc.openPanel("review")} title="Open GSpot">
        <GMark style={{ color: "#2cc9b6" }} />
        {count > 0 && <span className="badge">{count > 99 ? "99+" : count}</span>}
      </div>
    </div>
  );
}
