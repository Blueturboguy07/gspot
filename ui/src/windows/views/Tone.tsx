import { useEffect } from "react";
import type { Ctx } from "../Panel";
import { NoDoc, TitleBar, Empty, useFeature } from "../Panel";
import * as I from "../../components/icons";

interface ToneResult { too_short?: boolean; tones: { name: string; confidence: number }[]; summary: string }

export function Tone({ ctx }: { ctx: Ctx }) {
  const f = useFeature<ToneResult>("tone", ["Reading your text", "Detecting tone"]);
  useEffect(() => { if (ctx.doc && ctx.doc.text.trim().split(/\s+/).length >= 12) f.run(); }, [ctx.doc?.version]);
  if (!ctx.doc) return <NoDoc ctx={ctx} />;
  const d = f.data;
  return (
    <>
      <TitleBar title="Tone insights" onReload={() => f.run()} />
      <div className="panel__scroll">
        {f.loading && !d ? <div className="empty"><div className="spin" /><p style={{ marginTop: 16 }}>Detecting your tone.</p></div>
        : f.error ? <div className="notice">{f.error}</div>
        : !d || d.too_short ? (
          <Empty icon={<I.ToneWave />} title="We couldn't detect your tone yet">Add more text to get tone insights.</Empty>
        ) : (
          <>
            <div className="tone-list">
              {d.tones.map((t, i) => (
                <div className="tone" key={t.name}><span className="dot" style={{ opacity: 1 - i * 0.3 }} />{t.name}<span className="pct">{Math.round(t.confidence * 100)}%</span></div>
              ))}
            </div>
            <p style={{ color: "var(--text-subdued)", fontSize: 15, lineHeight: 1.5, marginTop: 18 }}>{d.summary}</p>
          </>
        )}
      </div>
    </>
  );
}
