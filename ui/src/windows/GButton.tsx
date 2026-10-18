import { useEffect, useState } from "react";
import { ipc, on } from "../lib/ipc";
import type { Alert, Status } from "../lib/types";
import { GMark, Check } from "../components/icons";

export function GButton() {
  const [count, setCount] = useState(0);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    ipc.getState().then((s) => { setCount(s.alerts.length); setBusy(s.status.linting); }).catch(() => {});
    const u1 = on<Alert[]>("host://alerts", (a) => setCount(a.length));
    const u2 = on<Status>("host://status", (s) => setBusy(s.linting));
    return () => { u1.then((f) => f()); u2.then((f) => f()); };
  }, []);
  return (
    <div style={{ width: "100%", height: "100%", display: "flex", alignItems: "center", justifyContent: "center" }}>
      <div className={"gbtn" + (busy ? " busy" : "")} onClick={() => ipc.openPanel("review")} title="Open GSpot">
        <GMark style={{ color: "#2cc9b6" }} />
        {count > 0 ? <span className="badge">{count > 99 ? "99+" : count}</span> : <span className="badge ok"><Check style={{ width: 11, height: 11 }} /></span>}
      </div>
    </div>
  );
}
