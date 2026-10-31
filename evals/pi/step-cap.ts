// Step cap for pi trials (psi-plan.md §12.5). pi has no turn limit, so this
// extension aborts the run once PSI_EVAL_STEPS turns have ended. A turn is
// one model request plus the tool calls it returns, which is what Psi calls
// a step.
export default function (pi: any) {
  const max = Number(process.env.PSI_EVAL_STEPS || "40");
  let turns = 0;
  let stopped = false;
  pi.on("turn_end", async (_event: any, ctx: any) => {
    turns++;
    if (turns >= max && !stopped) {
      stopped = true;
      console.error(`psi eval: step cap reached after ${turns} turns`);
      ctx.abort();
    }
  });
}
