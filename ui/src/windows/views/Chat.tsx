import { useEffect, useRef, useState } from "react";
import type { Ctx } from "../Panel";
import { ipc, on } from "../../lib/ipc";
import * as I from "../../components/icons";

interface Msg { role: "user" | "assistant"; content: string; pending?: boolean }

export function Chat({ ctx }: { ctx: Ctx }) {
  const [messages, setMessages] = useState<Msg[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const reqRef = useRef<string | null>(null);
  const bodyRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const un = on<{ req_id: string; delta: string; done: boolean; error?: string }>("llm://chunk", (c) => {
      if (c.req_id !== reqRef.current) return;
      setMessages((m) => {
        const last = m[m.length - 1];
        if (!last || last.role !== "assistant") return m;
        const content = c.error ? last.content + `\n\nError: ${c.error}` : last.content + c.delta;
        return [...m.slice(0, -1), { ...last, content, pending: !c.done }];
      });
      if (c.done) { setBusy(false); reqRef.current = null; }
    });
    return () => { un.then((f) => f()); };
  }, []);

  useEffect(() => { bodyRef.current?.scrollTo({ top: 1e9 }); }, [messages]);

  const send = async (text: string) => {
    const t = text.trim();
    if (!t || busy) return;
    const next: Msg[] = [...messages, { role: "user", content: t }, { role: "assistant", content: "", pending: true }];
    setMessages(next); setInput(""); setBusy(true);
    const id = `r${Date.now()}`;
    reqRef.current = id;
    try {
      await ipc.chatSend(id, next.filter((m) => !m.pending).map((m) => ({ role: m.role, content: m.content })));
    } catch (e) {
      setMessages((m) => [...m.slice(0, -1), { role: "assistant", content: `Error: ${e}` }]);
      setBusy(false); reqRef.current = null;
    }
  };

  const summarize = () => send("Summarize this page.");
  const review = () => { ctx.setView("review"); };

  return (
    <div className="chat">
      <div className="titlebar" style={{ justifyContent: "flex-start", paddingLeft: 16 }}>
        <div className="titlebar__actions"><button title="New chat" onClick={() => setMessages([])}><I.Edit /></button></div>
      </div>
      <div className="chat__body" ref={bodyRef}>
        {messages.length === 0 ? (
          <>
            <div className="hero"><div className="hero__icon"><I.ChatSpark /></div><h1>GSpot AI Chat</h1></div>
            <div className="chat__greeting">{ctx.settings.user_name.split(" ")[0] || "Hey"}, how can I help?</div>
            <div className="chat__chips">
              <button className="btn btn--outline" onClick={summarize}><I.ChatSpark style={{ width: 18, height: 18, color: "#2cc9b6" }} />Summarize this page</button>
              {ctx.alerts.length > 0 && <button className="btn btn--outline" onClick={review}><I.GMark style={{ width: 18, height: 18, color: "#2cc9b6" }} />Review {ctx.alerts.length} suggestions</button>}
            </div>
          </>
        ) : (
          messages.map((m, i) => (
            <div key={i} className={"msg " + (m.role === "user" ? "msg--user" : "msg--ai")}>{m.content}{m.pending && m.content === "" ? <span className="spin" style={{ display: "inline-block", width: 14, height: 14, borderWidth: 2 }} /> : null}</div>
          ))
        )}
      </div>
      <div className="chat__composer">
        {ctx.doc && (
          <div className="chat__ctx"><span className="ico"><I.Pencil /></span><span><b style={{ fontWeight: 600 }}>{ctx.doc.app_name || "Document"}</b><br /><span style={{ color: "var(--text-subdued)" }}>{ctx.doc.selected_text ? "Selection" : `${ctx.doc.text.split(/\s+/).filter(Boolean).length} words`}</span></span></div>
        )}
        <textarea className="chat__input" placeholder="Ask AI" value={input} rows={1} onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(input); } }} />
        <div className="chat__foot">
          <button className="btn btn--ghost" style={{ padding: 6 }} title="Attach the current selection" onClick={() => ctx.doc?.selected_text && setInput((v) => v + (v ? "\n\n" : "") + ctx.doc!.selected_text)}><I.Plus /></button>
          <button className="chat__send" disabled={!input.trim() || busy} onClick={() => send(input)}><I.ArrowUp /></button>
        </div>
      </div>
    </div>
  );
}
