import type { BisectResult } from "../data/source";
import { projectUrl } from "../router";
import { plural } from "../labels";
import { Cmd, Empty, Mono, Status, Table, when, short } from "./ui";

export function BisectResults({ results = [], project }: { results?: BisectResult[]; project: string }) {
  return results.length ? <div class="stack">
    {results.map((run) => <details key={run.id} open={results[0] === run}>
      <summary>{run.first_bad ? `First bad: ${short(run.first_bad)}` : "Bisect result"} · {when(run.time)}</summary>
      <div class="stack-s">
        <p>Good <a class="mono" href={projectUrl(project, "state/" + run.good)}>{short(run.good)}</a> → bad <a class="mono" href={projectUrl(project, "state/" + run.bad)}>{short(run.bad)}</a></p>
        {run.first_bad && <p>First bad: <a class="mono" href={projectUrl(project, "state/" + run.first_bad)}>{short(run.first_bad)}</a></p>}
        <Table label={`Bisect steps ${run.id}`} headers={["State", "Result", "Reused"]} rows={run.steps.map((step) => [
          <a class="mono" href={projectUrl(project, "state/" + step.state)}>{short(step.state)}</a>,
          <Status value={step.exit === 0 ? "passed" : step.exit === 125 ? "skipped" : "failed"} />,
          step.reused ? "yes" : "no",
        ])} />
        <p class="note">{plural(run.steps.length, "step")} · {run.steps.filter((step) => step.reused).length} reused</p>
        <Cmd text={run.argv.join(" ")} />
      </div>
    </details>)}
  </div> : <Empty>No bisect results recorded. Use the CLI to search exact states.</Empty>;
}
