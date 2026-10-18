import { useEffect, useState } from "react";
import type { Ctx } from "../Panel";
import { Hero, NoDoc, Steps, TitleBar, useFeature, Empty } from "../Panel";
import { ipc } from "../../lib/ipc";
import { LANGUAGES } from "../../lib/text";
import * as I from "../../components/icons";
import { RewriteResult } from "./Rewrite";

export function Translate({ ctx }: { ctx: Ctx }) {
  const f = useFeature<{ translation: string; detected_language: string; idioms: { text: string; meaning: string; origin: string; example: string }[]; used_selection: boolean }>("translate", ["Detecting language", "Translating"]);
  const target = ctx.settings.target_language;
  if (!ctx.doc) return <NoDoc ctx={ctx} />;
  const src = ctx.doc.selected_text || ctx.doc.text;
  return (
    <>
      <TitleBar title="" right={<button title="Info" onClick={() => ctx.toast("Translation runs through your configured model.")}><I.Info /></button>} />
      <div className="panel__scroll">
        <Hero icon={<I.Globe />} title="Translate" />
        <div className="section-label" style={{ marginTop: 0 }}>Target language</div>
        <div className="select-wrap"><select className="select" value={target} onChange={(e) => ctx.updateSettings({ target_language: e.target.value })}>{LANGUAGES.map((l) => <option key={l}>{l}</option>)}</select></div>
        {f.loading ? <Steps steps={["Detecting language", "Translating"]} active={f.step} />
        : f.error ? <div className="notice">{f.error}</div>
        : f.data ? (
          <>
            <div className="section-label">{f.data.detected_language ? `${f.data.detected_language} → ${target}` : target}</div>
            <RewriteResult ctx={ctx} original={src} rewritten={f.data.translation} usedSelection={f.data.used_selection} onBack={f.reset} />
            {f.data.idioms?.map((id, i) => (
              <div className="card" key={i} style={{ marginTop: 12 }}><div className="card__title" style={{ fontSize: 15 }}>“{id.text}”</div>
                {id.meaning && <div className="card__desc"><b>Meaning</b> <i>{id.meaning}</i></div>}
                {id.origin && id.origin !== "null" && <div className="card__desc"><b>Origin</b> <i>{id.origin}</i></div>}
                {id.example && <div className="card__desc"><b>Example</b> <i>{id.example}</i></div>}</div>
            ))}
          </>
        ) : (
          <>
            <Empty icon={<I.Translate />} title="Ready to translate.">Select any text or translate the full page in one click.</Empty>
            <div className="row-actions" style={{ justifyContent: "center" }}>
              {ctx.doc.selected_text && <button className="btn btn--primary" onClick={() => f.run({ target })}>Translate selection</button>}
              <button className={"btn " + (ctx.doc.selected_text ? "btn--outline" : "btn--primary")} onClick={() => f.run({ target, whole: true })}>Translate all</button>
            </div>
          </>
        )}
      </div>
    </>
  );
}

export function Explain({ ctx }: { ctx: Ctx }) {
  const f = useFeature<{ summary: string; explanation: string; terms: { term: string; definition: string }[]; used_selection: boolean }>("explain", ["Reading", "Explaining"]);
  if (!ctx.doc) return <NoDoc ctx={ctx} />;
  return (
    <>
      <TitleBar title="" right={<button title="Info" onClick={() => ctx.toast("Explain It uses your configured model.")}><I.Info /></button>} />
      <div className="panel__scroll">
        <Hero icon={<I.Book />} title="Explain It" />
        {f.loading ? <Steps steps={["Reading", "Explaining"]} active={f.step} />
        : f.error ? <div className="notice">{f.error}</div>
        : f.data ? (
          <>
            <div className="section-label">{f.data.used_selection ? "Your selection, explained" : "The whole page, explained"}</div>
            <p style={{ fontSize: 17, lineHeight: 1.5 }}>{f.data.summary}</p>
            <div className="result-box" style={{ whiteSpace: "pre-wrap" }}>{f.data.explanation}</div>
            {f.data.terms?.map((t, i) => (
              <div className="card" key={i} style={{ marginTop: 10 }}><div style={{ fontSize: 11, letterSpacing: 1, color: "var(--text-subdued)", fontWeight: 700 }}>TERM</div><div className="card__title">{t.term}</div><div className="card__desc">{t.definition}</div></div>
            ))}
            <div className="row-actions"><button className="btn btn--ghost" onClick={f.reset}>Back</button></div>
          </>
        ) : (
          <>
            <Empty icon={<I.Glasses />} title="Spot something confusing?">Highlight text for a quick explainer, or explain the whole page in one click.</Empty>
            <div className="row-actions" style={{ justifyContent: "center" }}>
              {ctx.doc.selected_text && <button className="btn btn--primary" onClick={() => f.run({})}>Explain selection</button>}
              <button className={"btn " + (ctx.doc.selected_text ? "btn--outline" : "btn--primary")} onClick={() => f.run({ whole: true })}>Explain the whole page</button>
            </div>
          </>
        )}
      </div>
    </>
  );
}

interface Phrase { id: string; phrase: string; bucket: "frequent" | "often" | "sometimes"; bucket_label: string; u16_start: number; u16_end: number; sentence: string }

export function Rewriter({ ctx }: { ctx: Ctx }) {
  const scan = useFeature<{ phrases: Phrase[]; words: number; too_short: boolean }>("rewriter_scan", ["Scanning your text", "Checking common AI phrases", "Generating report"]);
  const [filter, setFilter] = useState<"all" | Phrase["bucket"]>("all");
  const [done, setDone] = useState<Set<string>>(new Set());
  const [sugg, setSugg] = useState<Record<string, string[] | "loading">>({});
  useEffect(() => { if (ctx.doc) scan.run(); }, [ctx.doc?.version]);
  if (!ctx.doc) return <NoDoc ctx={ctx} />;
  const data = scan.data;
  const phrases = (data?.phrases ?? []).filter((p) => !done.has(p.id) && (filter === "all" || p.bucket === filter));
  const counts = { frequent: 0, often: 0, sometimes: 0 } as Record<Phrase["bucket"], number>;
  data?.phrases.forEach((p) => { if (!done.has(p.id)) counts[p.bucket]++; });
  const total = data?.phrases.length ?? 0;
  const reviewed = done.size;
  const getSuggestions = async (p: Phrase) => {
    setSugg((s) => ({ ...s, [p.id]: "loading" }));
    try { const r = await ipc.runFeature<{ suggestions: string[] }>("rewrite_phrase", { sentence: p.sentence, phrase: p.phrase }); setSugg((s) => ({ ...s, [p.id]: r.suggestions })); }
    catch (e) { ctx.toast(String(e)); setSugg((s) => { const n = { ...s }; delete n[p.id]; return n; }); }
  };
  const apply = async (p: Phrase, text: string) => {
    const d = ctx.doc!;
    const start = d.text.indexOf(p.sentence);
    if (start < 0) { ctx.toast("The sentence changed. Rescan."); return; }
    try { await ipc.replaceRange(start, start + p.sentence.length, text); setDone((s) => new Set(s).add(p.id)); ctx.toast("Replaced"); } catch (e) { ctx.toast(String(e)); }
  };
  return (
    <>
      <TitleBar title="" onReload={() => { setDone(new Set()); setSugg({}); scan.run(); }} />
      <div className="panel__scroll">
        <div className="hero" style={{ paddingBottom: 0 }}><div className="hero__icon"><I.Bolt /></div><h1 style={{ display: "flex", alignItems: "center", gap: 10 }}>AI Rewriter <button title="About" style={{ color: "#bcbcbc" }} onClick={() => ctx.toast("AI Rewriter finds words and phrases AI uses frequently, which can also appear in human writing. It suggests alternatives AI uses less often.")}><I.Info style={{ width: 20, height: 20 }} /></button></h1></div>
        {scan.loading && !data ? <Steps steps={["Scanning your text", "Checking common AI phrases", "Generating report"]} active={scan.step} />
        : scan.error ? <div className="notice">{scan.error}</div>
        : !data || data.too_short ? <Empty icon={<I.Bolt />} title="AI Rewriter is ready to go">Write or paste text to see common AI phrases.</Empty>
        : total === 0 ? <Empty icon={<I.Check />} title="No AI phrases found">I didn't find any common AI writing phrases.</Empty>
        : (
          <>
            <div className="section-label">AI phrases reviewed · {reviewed}/{total}</div>
            <div className="progress"><i style={{ width: `${total ? (reviewed / total) * 100 : 0}%` }} /></div>
            <p style={{ color: "var(--text-subdued)", fontSize: 15 }}>I found words and phrases that AI uses frequently. Check the highlighted sections for rewritten versions.</p>
            <div className="section-label">Phrases more frequent in AI text</div>
            <div className="filter">
              <button className={filter === "all" ? "active" : ""} onClick={() => setFilter("all")}>All AI phrases</button>
              <button className={filter === "frequent" ? "active" : ""} onClick={() => setFilter("frequent")}>Almost always AI · {counts.frequent}</button>
              <button className={filter === "often" ? "active" : ""} onClick={() => setFilter("often")}>Usually AI · {counts.often}</button>
              <button className={filter === "sometimes" ? "active" : ""} onClick={() => setFilter("sometimes")}>Sometimes AI · {counts.sometimes}</button>
            </div>
            {phrases.length === 0 && <Empty icon={<I.Check />} title="All phrases reviewed">You addressed all AI phrases found in your doc.</Empty>}
            {phrases.map((p) => {
              const s = sugg[p.id];
              const before = p.sentence.slice(0, p.sentence.toLowerCase().indexOf(p.phrase.toLowerCase()));
              const after = p.sentence.slice(before.length + p.phrase.length);
              return (
                <div className="phrase" key={p.id}>
                  <div className="phrase__head"><b>{p.phrase}</b><span className="phrase__bucket">{p.bucket_label} · {p.bucket === "frequent" ? "Over 50× frequency" : p.bucket === "often" ? "10-49× frequency" : "Less than 10× frequency"}</span></div>
                  <div style={{ fontSize: 14, color: "var(--text-subdued)" }}>{before}<mark>{p.phrase}</mark>{after}</div>
                  {s === "loading" ? <div className="spin" style={{ marginTop: 10 }} /> : Array.isArray(s) ? (
                    <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 6 }}>{s.map((x, i) => <button key={i} className="list-opt" style={{ textAlign: "left", fontSize: 14, padding: "10px 12px" }} onClick={() => apply(p, x)}>{x}</button>)}</div>
                  ) : null}
                  <div className="row-actions" style={{ marginTop: 10 }}>
                    {!Array.isArray(s) && <button className="btn btn--primary" style={{ padding: "7px 12px", fontSize: 13 }} onClick={() => getSuggestions(p)}>Rewrite</button>}
                    <button className="btn btn--ghost" style={{ padding: "7px 12px", fontSize: 13 }} onClick={() => setDone((d) => new Set(d).add(p.id))}>Dismiss</button>
                  </div>
                </div>
              );
            })}
          </>
        )}
      </div>
    </>
  );
}

export function Detector({ ctx }: { ctx: Ctx }) {
  const f = useFeature<{ too_short?: boolean; authenticity: number; ai_likelihood: number; verdict: string; reasons: string[]; most_ai_sentence: string; signals: any; method: string }>("detector", ["Scanning your text", "Measuring rhythm and vocabulary", "Checking patterns"]);
  if (!ctx.doc) return <NoDoc ctx={ctx} />;
  const d = f.data;
  return (
    <>
      <TitleBar title="" right={<button title="History" onClick={() => ctx.setView("history")}><I.Clock /></button>} />
      <div className="panel__scroll">
        <div className="titlebar" style={{ gap: 8 }}><I.Scanner style={{ width: 22, height: 22 }} /> AI Detector</div>
        {f.loading ? <Steps steps={["Scanning your text", "Measuring rhythm and vocabulary", "Checking patterns"]} active={f.step} />
        : f.error ? <div className="notice">{f.error}</div>
        : d?.too_short ? <Empty icon={<I.Pencil />} title="Write a little more">To receive feedback, your text must be 8 words or longer.</Empty>
        : d ? (
          <>
            <div className="score-head" style={{ borderBottom: 0 }}>
              <div className="score-big"><b>{d.authenticity}</b><span>authentic</span></div>
              <div><h3>{d.verdict}</h3><p>Some parts of your text may contain patterns that resemble AI text. These can also occur in your writing. If you use generative AI, you can cite it.</p></div>
            </div>
            <div className="progress"><i style={{ width: `${d.authenticity}%`, background: d.ai_likelihood > 55 ? "#ffbf47" : "#2cc9b6" }} /></div>
            {d.reasons?.length ? <ul style={{ paddingLeft: 18, color: "var(--text-subdued)", fontSize: 15, lineHeight: 1.6 }}>{d.reasons.map((r, i) => <li key={i}>{r}</li>)}</ul> : null}
            {d.most_ai_sentence && <div className="result-box" style={{ fontStyle: "italic" }}>“{d.most_ai_sentence}”</div>}
            <div className="section-label">Signals</div>
            <div className="kv"><span>Sentence rhythm (burstiness)</span><span>{d.signals.burstiness.toFixed(2)}</span></div>
            <div className="kv"><span>Average sentence length</span><span>{d.signals.avg_sentence_len.toFixed(1)} words</span></div>
            <div className="kv"><span>AI phrases per 100 words</span><span>{d.signals.ai_phrases_per_100w.toFixed(2)}</span></div>
            <div className="kv"><span>Human markers</span><span>{d.signals.human_markers}</span></div>
            <div className="notice--ok notice" style={{ marginTop: 14 }}>Method: {d.method}.</div>
            <div className="row-actions"><button className="btn btn--outline" onClick={() => f.run()}>Check again</button></div>
          </>
        ) : (
          <div className="uphook">
            <I.Scanner style={{ width: 72, height: 72, color: "#fff" }} />
            <h2>You're a true original.<br />Ensure your work is, too.</h2>
            <ul><li><span className="ok"><I.Check style={{ width: 14, height: 14 }} /></span>Detect AI generated text</li><li><span className="ok"><I.Check style={{ width: 14, height: 14 }} /></span>Get a text authenticity score</li><li><span className="ok"><I.Check style={{ width: 14, height: 14 }} /></span>See which sentences stand out</li></ul>
            <button className="btn btn--premium" onClick={() => f.run()}>Check this text</button>
          </div>
        )}
      </div>
    </>
  );
}

interface Source { url: string; title: string; site: string; matched_words: number; matched_pct: number; snippet: string; citations: Record<string, string> }

export function Plagiarism({ ctx }: { ctx: Ctx }) {
  const f = useFeature<{ too_short?: boolean; sources: Source[]; overall_pct: number; queries: string[]; searches?: number; search_via?: string; search_note?: string | null; words: number }>("plagiarism", ["Picking distinctive phrases", "Searching the web", "Comparing sources", "Building citations"]);
  const style = ctx.settings.citation_style;
  if (!ctx.doc) return <NoDoc ctx={ctx} />;
  const d = f.data;
  return (
    <>
      <TitleBar title="Plagiarism" />
      <div className="panel__scroll">
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 0 4px" }}>
          <b style={{ fontSize: 20 }}>Plagiarism and AI check</b>
          <select className="select" style={{ width: "auto", padding: "6px 28px 6px 10px", fontSize: 14 }} value={style} onChange={(e) => ctx.updateSettings({ citation_style: e.target.value })}><option>APA</option><option>MLA</option><option>Chicago</option></select>
        </div>
        {f.loading ? <Steps steps={["Picking distinctive phrases", "Searching the web", "Comparing sources", "Building citations"]} active={f.step} />
        : f.error ? <div className="notice">{f.error}</div>
        : d?.too_short ? <Empty icon={<I.Pencil />} title="Write a little more">Plagiarism checks need at least 20 words.</Empty>
        : d ? (
          <>
            <div className="score-head" style={{ borderBottom: 0, paddingTop: 10 }}>
              <div className="score-big"><b>{Math.round(d.overall_pct)}%</b><span>matched</span></div>
              <div><h3>{d.sources.length === 0 ? "No matches found" : `${d.sources.length} source${d.sources.length === 1 ? "" : "s"} with matching text`}</h3><p>Checked {d.words} words against the web with {d.searches ?? d.queries.length} searches.{d.search_via === "publik" ? " Searched with publik API." : d.search_via === "brave" ? " Searched with your Brave key." : ""}</p></div>
            </div>
            {d.search_note && <div className="notice">{d.search_note}</div>}
            {d.sources.map((s, i) => (
              <div className="src" key={i}>
                <div style={{ display: "flex", justifyContent: "space-between", gap: 8 }}><b>{s.title || s.site}</b><span className="src__pct">{s.matched_pct.toFixed(1)}%</span></div>
                <a className="src__url" href="#" onClick={(e) => { e.preventDefault(); ipc.openUrl(s.url); }}>{s.url}</a>
                {s.snippet && <div className="src__quote">“…{s.snippet}…”</div>}
                <div className="src__cite">{s.citations[style] ?? s.citations["APA"]}</div>
                <div className="row-actions" style={{ marginTop: 8 }}><button className="btn btn--outline" style={{ padding: "6px 12px", fontSize: 13 }} onClick={() => navigator.clipboard.writeText(s.citations[style] ?? "").then(() => ctx.toast("Citation copied"))}>Copy citation</button></div>
              </div>
            ))}
            <div className="row-actions"><button className="btn btn--outline" onClick={() => f.run()}>Check again</button></div>
          </>
        ) : (
          <div className="uphook" style={{ borderColor: "transparent" }}>
            <I.Incognito style={{ width: 72, height: 72, color: "#fff" }} />
            <h2>Check for plagiarism<br />before you submit</h2>
            <ul><li><span className="ok"><I.Arrow style={{ width: 14, height: 14 }} /></span>Catch missing citations</li><li><span className="ok"><I.Arrow style={{ width: 14, height: 14 }} /></span>Detect text matches</li><li><span className="ok"><I.Arrow style={{ width: 14, height: 14 }} /></span>Create citations quickly</li></ul>
            <button className="btn btn--premium" onClick={() => f.run()}>Check this text</button>
          </div>
        )}
      </div>
    </>
  );
}
