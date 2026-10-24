import { useState } from "preact/hooks";
import type {
  DataSource,
  Project,
  Operation,
  Behavior,
  Line,
} from "../data/source";
import { projectUrl } from "../router";
import {
  machineLabel,
  object,
  operationLabel,
  personLabel,
  plural,
  shortText,
} from "../labels";
import {
  Badge,
  Card,
  Cmd,
  CodeText,
  Empty,
  LoadError,
  Mono,
  PageHead,
  ReadOnlyAction,
  Section,
  Segments,
  Status,
  Table,
  Who,
  bytes,
  short,
  useData,
  when,
} from "../components/ui";
import { BehaviorCards } from "../components/behavior";

export function ChecksPage({
  source,
  project,
  behavior,
}: {
  source: DataSource;
  project: Project;
  behavior: boolean;
}) {
  const [filter, setFilter] = useState(""),
    [result, setResult] = useState("Any result"),
    [base, setBase] = useState(""),
    [head, setHead] = useState(""),
    [pair, setPair] = useState<[string, string]>();
  const { data, error } = useData(async () => {
    const states = await source.log(project.id, project.default_line ?? "main");
    const b = pair?.[0] ?? states.at(-1)?.id ?? "",
      h = pair?.[1] ?? states[0]?.id ?? "";
    let comparison: Behavior | undefined,
      comparisonError = "";
    if (behavior && b && h) {
      try {
        comparison = await source.probeDiff(project.id, b, h);
      } catch (e) {
        comparisonError =
          e instanceof Error ? e.message : "Probe results unavailable.";
      }
    }
    let screens: Awaited<ReturnType<DataSource["screens"]>> = [];
    let screensError = "";
    if (behavior && b && h)
      try {
        screens = await source.screens(project.id, b, h);
      } catch {
        screensError = "UI diffs are not available in the browser yet.";
      }
    return { states, b, h, comparison, comparisonError, screens, screensError };
  }, [source, project.id, behavior, pair?.join()]);
  if (!data) return <LoadError error={error} />;
  const definitions = Object.entries(project.config ?? {})
    .filter(([key]) => key.startsWith("check."))
    .map(([key, value]) => ({
      name: key.slice(6),
      command:
        typeof value === "string"
          ? value
          : Array.isArray(value)
            ? value.join(" ")
            : "Not recorded",
    }));
  const checks = (project.checks ?? [])
    .filter((c) => c.name.toLowerCase().includes(filter.toLowerCase()))
    .filter(
      (c) =>
        result === "Any result" ||
        (c.exit === 0
          ? "passed"
          : c.exit === undefined
            ? "not run"
            : "failed") === result,
    );
  return (
    <>
      <Segments
        items={[
          ["Tests", projectUrl(project.id, "checks")],
          ["Behavior", projectUrl(project.id, "checks/behavior")],
        ]}
        current={behavior ? "Behavior" : "Tests"}
      />
      <PageHead
        title={behavior ? "Behavior" : "Checks"}
        description={
          behavior
            ? "Probes record what the program does on an exact state. A behavior diff compares two states."
            : "Signed check results. Each one names one exact state and one exact command."
        }
      />
      {behavior ? (
        <div class="stack">
          <Table
            label="Probe definitions"
            headers={["Probe", "Kind", "Command", "Queue limit"]}
            rows={(project.probes ?? []).map((p) => [
              <Mono>{p.name}</Mono>,
              p.kind,
              <Mono>{p.run.join(" ")}</Mono>,
              p.fail_over === undefined
                ? "Not recorded"
                : `fail over +${p.fail_over * 100}%`,
            ])}
          />
          {!project.probes?.length && (
            <Empty>No probe definitions recorded.</Empty>
          )}
          <Card title="Compare any two states">
            <form
              class="row"
              onSubmit={(e) => {
                e.preventDefault();
                setPair([base || data.b, head || data.h]);
              }}
            >
              <label class="field">
                <span>Base</span>
                <select
                  class="input mono"
                  value={base || data.b}
                  onChange={(e) => setBase(e.currentTarget.value)}
                >
                  {data.states.map((s) => (
                    <option value={s.id}>
                      {short(s.id)} {s.message}
                    </option>
                  ))}
                </select>
              </label>
              <label class="field">
                <span>Head</span>
                <select
                  class="input mono"
                  value={head || data.h}
                  onChange={(e) => setHead(e.currentTarget.value)}
                >
                  {data.states.map((s) => (
                    <option value={s.id}>
                      {short(s.id)} {s.message}
                    </option>
                  ))}
                </select>
              </label>
              <button class="btn primary" type="submit">
                Compare
              </button>
            </form>
            <Cmd text={`layr probe diff ${base || data.b} ${head || data.h}`} />
          </Card>
          <h2>
            Behavior diff: <Mono>{short(data.b)}</Mono> →{" "}
            <Mono>{short(data.h)}</Mono>
          </h2>
          {data.comparison ? (
            <BehaviorCards behavior={data.comparison} screens={data.screens} />
          ) : (
            <Empty>
              {data.comparisonError || "No probe comparisons recorded."}
            </Empty>
          )}
          {data.screensError && <Empty>{data.screensError}</Empty>}
        </div>
      ) : (
        <div class="cols">
          <div class="stack">
            <div class="filters">
              <input
                class="input grow"
                type="search"
                aria-label="Filter checks"
                placeholder="Filter checks"
                value={filter}
                onInput={(e) => setFilter(e.currentTarget.value)}
              />
              <select
                class="input"
                aria-label="Check result"
                value={result}
                onChange={(e) => setResult(e.currentTarget.value)}
              >
                {["Any result", "passed", "failed", "not run"].map((r) => (
                  <option>{r}</option>
                ))}
              </select>
            </div>
            <Table
              label="Check results"
              headers={[
                "Check",
                "State",
                "Result",
                "Duration",
                "Runner",
                "Command",
                "Reuse",
              ]}
              rows={checks.map((c) => [
                <Mono>{c.name}</Mono>,
                c.state ? (
                  <a
                    class="mono"
                    href={projectUrl(project.id, "state/" + c.state)}
                  >
                    {short(c.state)}
                  </a>
                ) : (
                  "Not recorded"
                ),
                <Status
                  value={
                    c.exit === undefined
                      ? "not run"
                      : c.exit === 0
                        ? "passed"
                        : "failed"
                  }
                />,
                c.duration_ms === undefined
                  ? "Not recorded"
                  : `${c.duration_ms} ms`,
                <Mono>{short(c.machine)}</Mono>,
                <Mono>{c.argv.join(" ")}</Mono>,
                c.reused ? <Badge>reused</Badge> : "",
              ])}
            />
            {!checks.length && <Empty>No check results recorded.</Empty>}
            {checks
              .filter((c) => c.log)
              .map((c) => (
                <details>
                  <summary>
                    Log of {c.name} on {short(c.state)}
                  </summary>
                  <div class="frame">
                    <CodeText text={c.log!} />
                  </div>
                </details>
              ))}
          </div>
          <aside class="side">
            <section>
              <h2 class="side-title">Check definitions</h2>
              <ul>
                {definitions.map((c) => (
                  <li>
                    <Mono>{c.name}</Mono>
                    <span class="r">{c.command}</span>
                  </li>
                ))}
              </ul>
              {!definitions.length && (
                <p class="note">No check definitions recorded.</p>
              )}
            </section>
            <section>
              <h2 class="side-title">
                Probe definitions{" "}
                <a href={projectUrl(project.id, "checks/behavior")}>Results</a>
              </h2>
              <ul>
                {(project.probes ?? []).map((p) => (
                  <li>
                    <Mono>{p.name}</Mono>
                    <span class="r">{p.kind}</span>
                  </li>
                ))}
              </ul>
            </section>
          </aside>
        </div>
      )}
    </>
  );
}
export function DeployPage({
  source,
  project,
  section,
  reference,
}: {
  source: DataSource;
  project: Project;
  section: string;
  reference?: string;
}) {
  const { data, error } = useData(async () => {
    const refs = await source.refs(project.id);
    const history =
      section === "environment" && reference
        ? await source.refLog(project.id, reference)
        : [];
    return { refs, history };
  }, [source, project.id, section, reference]);
  if (!data) return <LoadError error={error} />;
  const images = project.images ?? [],
    envs = data.refs.filter((r) => r.name.startsWith("env/")),
    others = data.refs.filter((r) => !r.name.startsWith("env/"));
  return (
    <>
      <Segments
        items={[
          ["Environments", projectUrl(project.id, "deploy/environments")],
          ["Images", projectUrl(project.id, "deploy/images")],
        ]}
        current={section === "images" ? "Images" : "Environments"}
      />
      {section === "environment" ? (
        <div class="stack">
          <PageHead
            title={<Mono>{reference}</Mono>}
            description="An environment is a ref. Every position names an exact state."
            actions={
              <ReadOnlyAction
                label={`Move ${reference}…`}
                command={`layr ref set ${reference} <state> --expect <current>`}
                primary
              />
            }
          />
          <Section title="Position history">
            <Table
              label="Ref history"
              headers={["Position", "State", "Expected value"]}
              rows={[...data.history].reverse().map((r, i) => [
                <Mono>
                  {reference}@{"{"}
                  {i}
                  {"}"}
                </Mono>,
                r.after ? (
                  <a
                    class="mono"
                    href={projectUrl(project.id, "state/" + r.after.target)}
                  >
                    {short(r.after.target)}
                  </a>
                ) : (
                  "removed"
                ),
                <Mono>{r.before ? short(r.before.target) : "none"}</Mono>,
              ])}
            />
            {!data.history.length && <Empty>No ref history recorded.</Empty>}
            <Cmd text={`layr ref log ${reference}`} />
          </Section>
        </div>
      ) : section === "images" ? (
        <div class="cols">
          <div class="stack">
            <PageHead title="Images" description="Images name exact states." />
            <div class="callout">
              Images are built on demand from the exact state when a tag is
              first pulled.
            </div>
            <Section title="Images">
              <Table
                headers={["Image", "Tag", "Maps to", "Digest", "Size"]}
                rows={images.map((i) => [
                  <Mono>{i.name}</Mono>,
                  <Mono>{i.tag}</Mono>,
                  <Mono>{i.ref ?? short(i.state)}</Mono>,
                  <Mono>{i.digest ?? "Not recorded"}</Mono>,
                  bytes(i.bytes),
                ])}
              />
              {!images.length && (
                <Empty>
                  No image records available. The image service supplies digests
                  and tags.
                </Empty>
              )}
            </Section>
            <Section title="Helm charts">
              <Empty>
                No chart records available. The image service supplies chart
                versions.
              </Empty>
            </Section>
            <Section title="Runnables">
              <Empty>No runnable definitions recorded.</Empty>
            </Section>
          </div>
          <aside class="side">
            <section>
              <h2 class="side-title">Pull</h2>
              <p class="note">Get a token with the CLI.</p>
              <Cmd text="layr access token" />
            </section>
            <section>
              <h2 class="side-title">Deterministic</h2>
              <p class="note">Image layers come from the state's files.</p>
            </section>
          </aside>
        </div>
      ) : (
        <div class="stack">
          <PageHead
            title="Environments"
            description="An environment is a ref. It moves only with compare and swap, and keeps its history."
          />
          <Table
            label="Environments"
            headers={["Ref", "Current state", "Expires"]}
            rows={envs.map((r) => [
              <a
                class="mono"
                href={projectUrl(
                  project.id,
                  "deploy/environment/" + encodeURIComponent(r.name),
                )}
              >
                {r.name}
              </a>,
              <a
                class="mono"
                href={projectUrl(project.id, "state/" + r.target)}
              >
                {short(r.target)}
              </a>,
              r.expires_at ? when(r.expires_at) : "never",
            ])}
          />
          {!envs.length && <Empty>No environments recorded.</Empty>}
          <Section title="Other refs">
            <Table
              headers={["Ref", "State", "Expires"]}
              rows={others.map((r) => [
                <Mono>{r.name}</Mono>,
                <a
                  class="mono"
                  href={projectUrl(project.id, "state/" + r.target)}
                >
                  {short(r.target)}
                </a>,
                r.expires_at ? when(r.expires_at) : "never",
              ])}
            />
            <Cmd text="layr ref list" />
          </Section>
        </div>
      )}
    </>
  );
}
function operationTarget(op: Operation, lines: Line[]) {
  if (op.op === "config") return "";
  const recorded =
    Object.values(op.lines ?? {})
      .map((change) => {
        const line = change.after ?? change.before;
        return line ? `${shortText(line.name)} ${short(line.head)}` : "";
      })
      .filter(Boolean)
      .join(", ") ||
    op.states
      ?.map((state) => {
        const line = lines.find((line) => line.id === state.line);
        return [line && shortText(line.name), short(state.id)]
          .filter(Boolean)
          .join(" ");
      })
      .join(", ");
  if (recorded) return recorded;
  const data = object(op.data),
    queueChange = object(data.queue_change),
    queue = object(queueChange.after ?? queueChange.before),
    refChange = object(data.ref_change),
    ref = object(refChange.after ?? refChange.before),
    subject = object(object(data.attestation).subject),
    state =
      queue.state ??
      data.state ??
      (subject.kind === "state" ? subject.id : undefined) ??
      ref.target,
    line = lines.find((line) => line.id === (queue.target_line ?? data.line)),
    name = line?.name ?? (typeof queue.into === "string" ? queue.into : "");
  return [shortText(name), typeof state === "string" ? short(state) : ""]
    .filter(Boolean)
    .join(" ");
}
export function ActivityPage({
  source,
  project,
}: {
  source: DataSource;
  project: Project;
}) {
  const [person, setPerson] = useState("Anyone"),
    [kind, setKind] = useState("All kinds"),
    [limit, setLimit] = useState(50);
  const { data, error } = useData(async () => {
    const [operations, refs, copies, machines, people, lines] =
      await Promise.all([
        source.opLog(project.id),
        source.refs(project.id),
        source.copies(project.id),
        source.machines(),
        source.people(),
        source.lines(project.id),
      ]);
    return { operations, refs, copies, machines, people, lines };
  }, [source, project.id]);
  if (!data) return <LoadError error={error} />;
  const operations = data.operations.filter(
    (o) =>
      (person === "Anyone" || o.actor.name === person) &&
      (kind === "All kinds" || o.op === kind),
  );
  const now = Date.now(),
    today = new Date(now).setUTCHours(0, 0, 0, 0),
    recent = data.operations.filter((o) => o.time >= today && o.time <= now),
    lastHour = data.operations.filter(
      (o) => o.time >= now - 3600000 && o.time <= now,
    );
  const counters: [string, number][] = [
    [
      "Saves today",
      recent.filter((o) => o.op === "commit" || o.op === "save").length,
    ],
    ["Merged today", recent.filter((o) => o.op === "merge").length],
    [
      "Checks passed",
      recent.filter(
        (o) =>
          o.op === "attest" &&
          o.data &&
          typeof o.data === "object" &&
          !Array.isArray(o.data) &&
          typeof o.data.check === "object" &&
          o.data.check !== null &&
          !Array.isArray(o.data.check) &&
          o.data.check.exit === 0,
      ).length,
    ],
  ];
  const agents = lastHour.filter((o) => o.actor.session),
    people = lastHour.filter((o) => !o.actor.session),
    machines = new Set(lastHour.map((o) => o.machine)),
    agentCount = new Set(agents.map((o) => o.actor.session)).size,
    personCount = new Set(people.map((o) => o.actor.uid)).size,
    environmentCount = data.refs.filter((r) =>
      r.name.startsWith("env/"),
    ).length;
  return (
    <div class="stack">
      <PageHead
        title="Activity"
        description="The signed operation log. Every operation names who, where and what."
      />
      <div class="filters">
        <select
          class="input"
          aria-label="Person filter"
          value={person}
          onChange={(e) => {
            setPerson(e.currentTarget.value);
            setLimit(50);
          }}
        >
          <option>Anyone</option>
          {[...new Set(data.operations.map((o) => o.actor.name))].map((p) => (
            <option value={p}>{shortText(p)}</option>
          ))}
        </select>
        <select
          class="input"
          aria-label="Operation filter"
          value={kind}
          onChange={(e) => {
            setKind(e.currentTarget.value);
            setLimit(50);
          }}
        >
          <option>All kinds</option>
          {[...new Set(data.operations.map((o) => o.op))].map((k) => (
            <option value={k} title={k}>
              {operationLabel(k)}
            </option>
          ))}
        </select>
      </div>
      {data.operations.length ? (
        <div class="card pulse-card">
          <div class="hd">
            <b>{project.name}</b>
            <span class="spacer" />
            <Badge>stored records</Badge>
          </div>
          <div class="summary pulse-kpi">
            {counters.map(([label, value]) => (
              <div>
                <div class="k">{label}</div>
                <div class="v">{value}</div>
              </div>
            ))}
          </div>
          <div class="bd">
            <Pulse
              sources={[
                {
                  name: plural(agentCount, "agent"),
                  count: agentCount,
                  rate: agents.length,
                },
                {
                  name: plural(personCount, "person", "people"),
                  count: personCount,
                  rate: people.length,
                },
                {
                  name: plural(machines.size, "machine"),
                  count: machines.size,
                  rate: lastHour.length,
                },
              ]}
              targets={[
                {
                  name: plural(environmentCount, "environment"),
                  count: environmentCount,
                  rate: lastHour.filter((o) => o.op === "ref.set").length,
                },
                {
                  name: plural(data.copies.length, "copy", "copies"),
                  count: data.copies.length,
                  rate: lastHour.filter(
                    (o) => o.op === "push" || o.op === "sync",
                  ).length,
                },
              ]}
            />
            <p class="note">Flow rate uses operations from the last hour.</p>
          </div>
        </div>
      ) : (
        <p class="note">Activity is not recorded for this store.</p>
      )}
      <Section title="Operations">
        <Table
          label="Operations"
          headers={[
            "Operation",
            "Who",
            "Machine",
            "Target",
            "Time",
            "Signature",
          ]}
          rows={operations.slice(0, limit).map((op) => {
            const actor = shortText(
              op.actor.session ??
                personLabel(String(op.actor.uid), data.operations, data.people),
            );
            return [
              <span title={op.op}>{operationLabel(op)}</span>,
              actor.startsWith("uid ") ? (
                <span class="muted">{actor}</span>
              ) : (
                <Who name={actor} />
              ),
              <span title={short(op.machine)}>{machineLabel(op.machine, data.machines)}</span>,
              <Mono>{operationTarget(op, data.lines)}</Mono>,
              when(op.time),
              <Badge kind="ok">✓ signed</Badge>,
            ];
          })}
        />
        {operations.length > limit && (
          <button
            class="btn"
            type="button"
            onClick={() => setLimit(limit + 50)}
          >
            Show more
          </button>
        )}
        <Cmd text="layr op log" />
      </Section>
    </div>
  );
}
type FlowNode = { name: string; count: number; rate: number };

function Pulse({
  sources,
  targets,
}: {
  sources: FlowNode[];
  targets: FlowNode[];
}) {
  return (
    <svg
      class="pulse"
      viewBox="0 0 820 240"
      role="img"
      aria-label="Operations flow through the queue into main, then to environments and copies"
    >
      {sources
        .filter((node) => node.count > 0)
        .map(({ name, rate }, i) => {
          const y = 50 + i * 70,
            id = "pulse-source-" + i;
          return (
            <g>
              <path
                id={id}
                d={`M220,${y} C320,${y} 280,120 364,120`}
                class="flow"
              />
              {rate > 0 && (
                <circle r="3" class="dot">
                  <animateMotion
                    dur={`${Math.max(1, 10 / rate)}s`}
                    repeatCount="indefinite"
                  >
                    <mpath href={"#" + id} />
                  </animateMotion>
                </circle>
              )}
              <rect
                x="80"
                y={y - 15}
                width="140"
                height="30"
                rx="6"
                class="node"
              />
              <text x="150" y={y + 4} text-anchor="middle">
                {name}
              </text>
            </g>
          );
        })}
      <circle cx="410" cy="120" r="58" class="halo" />
      <circle cx="410" cy="120" r="44" class="hub" />
      <text x="410" y="118" text-anchor="middle" class="hub-t">
        main
      </text>
      <text x="410" y="134" text-anchor="middle" class="hub-s">
        via queue
      </text>
      {targets
        .filter((node) => node.count > 0)
        .map(({ name, rate }, i) => {
          const y = 70 + i * 100,
            id = "pulse-target-" + i;
          return (
            <g>
              <path
                id={id}
                d={`M456,120 C540,120 500,${y} 600,${y}`}
                class="flow"
              />
              {rate > 0 && (
                <circle r="3" class="dot out">
                  <animateMotion
                    dur={`${Math.max(1, 10 / rate)}s`}
                    repeatCount="indefinite"
                  >
                    <mpath href={"#" + id} />
                  </animateMotion>
                </circle>
              )}
              <rect
                x="600"
                y={y - 15}
                width="140"
                height="30"
                rx="6"
                class="node"
              />
              <text x="670" y={y + 4} text-anchor="middle">
                {name}
              </text>
            </g>
          );
        })}
    </svg>
  );
}
