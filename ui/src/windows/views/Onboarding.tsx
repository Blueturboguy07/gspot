import type { Ctx } from "../Panel";
import { Hero } from "../Panel";
import { AiSetup } from "./AiSetup";
import * as I from "../../components/icons";

/** First run, after Accessibility: pick how the AI tools run. One screen, no URLs or ports. */
export function Onboarding({ ctx }: { ctx: Ctx }) {
  const done = async () => { await ctx.updateSettings({ onboarded: true }); ctx.setView("review"); };
  return (
    <div className="panel__scroll">
      <Hero icon={<I.Sparkle />} title="Choose how GSpot thinks">
        <p>Spelling and grammar run on this Mac. The sidebar's AI tools (chat, paraphrase, spot check, tone and more) need a model. Pick one now; you can change it any time in Settings.</p>
      </Hero>
      <AiSetup ctx={ctx} mode="onboarding" onDone={done} />
    </div>
  );
}
