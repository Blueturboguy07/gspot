import type { Ctx } from "../Panel";
import * as I from "../../components/icons";
import { NoDoc, TitleBar } from "../Panel";

export function Performance({ ctx }: { ctx: Ctx }) {
  if (!ctx.doc) return <NoDoc ctx={ctx} />;
  const s = ctx.score;
  const rows = s ? [
    { label: "Correctness", color: "#ff7a74", fill: s.correctness_fill, val: s.correctness_label, ok: s.correctness_count === 0 },
    { label: "Clarity", color: "#7d99f0", fill: s.clarity_fill, val: s.clarity_label, ok: s.clarity_count === 0 },
    { label: "Engagement", color: "#2cc9b6", fill: s.engagement_fill, val: s.engagement_label, ok: s.engagement_count === 0 },
    { label: "Delivery", color: "#a598f0", fill: s.delivery_fill, val: s.delivery_label, ok: s.delivery_count === 0 },
  ] : [];
  return (
    <>
      <TitleBar title="Performance" />
      <div className="panel__scroll">
        <div className="score-head">
          <div className="score-big"><b>{s?.overall ?? "–"}</b><span>of 100</span></div>
          <div>
            <h3>Overall score</h3>
            <p>{s?.note ?? "The score represents the quality of this text. Increase it by addressing issues."}</p>
          </div>
        </div>
        {rows.map((r) => (
          <div className="score-row" key={r.label}>
            <div className="lbl"><i style={{ width: 12, height: 12, borderRadius: "50%", background: r.color, display: "inline-block" }} />{r.label}</div>
            <div className="bar"><i style={{ width: `${r.fill}%`, background: r.color }} /></div>
            <div className="val">{r.ok ? <I.Check style={{ width: 16, height: 16, color: "#2cc9b6" }} /> : null}{r.val}</div>
          </div>
        ))}
        {s && (
          <div style={{ marginTop: 24 }}>
            <div className="section-label">Text statistics</div>
            <div className="kv"><span>Words</span><span>{s.words}</span></div>
            <div className="kv"><span>Characters</span><span>{s.characters}</span></div>
            <div className="kv"><span>Sentences</span><span>{s.sentences}</span></div>
            <div className="kv"><span>Reading time</span><span>{Math.max(1, Math.round(s.reading_time_sec / 60))} min</span></div>
            <div className="kv"><span>Readability (Flesch)</span><span>{Math.round(s.flesch)}</span></div>
          </div>
        )}
      </div>
    </>
  );
}
