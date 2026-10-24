import { useState } from "preact/hooks";
import type { Change, DataSource, Project, QueueEntry } from "../data/source";
import { projectUrl } from "../router";
import {
  Badge,
  Card,
  Cmd,
  Dropdown,
  Empty,
  LoadError,
  Mono,
  PageHead,
  ReadOnlyAction,
  Section,
  Segments,
  Status,
  Table,
  ToggleButtons,
  Who,
  short,
  useData,
  when,
} from "../components/ui";
import { BehaviorCards } from "../components/behavior";
import { DiffBrowser, DiffFiles, InterdiffBrowser } from "../components/diff";
import { changeStatus, loadChangeIndex } from "../data/change-index";

export function ChangesPage({
  source,
  project,
  section,
}: {
  source: DataSource;
  project: Project;
  section: string;
}) {
  const [filter, setFilter] = useState("");
  const { data, error } = useData(
    () => loadChangeIndex(source, project),
    [source, project.id],
  );
  if (!data) return <LoadError error={error} />;
  const base = projectUrl(project.id, "changes");
  const rows = data.changes
    .filter(
      (c) =>
        changeStatus(c, data.queue, data.statuses[c.change_id]) ===
        (section === "merged"
          ? "merged"
          : section === "closed"
            ? "closed"
            : "open"),
    )
    .filter((c) =>
      c.versions
        .at(-1)
        ?.state.message.toLowerCase()
        .includes(filter.toLowerCase()),
    );
  return (
    <>
      <Segments
        items={[
          [`Open`, base],
          ["In queue", base + "/queue"],
          ["Merged", base + "/merged"],
          ["Closed", base + "/closed"],
        ]}
        current={
          section === "queue"
            ? "In queue"
            : section === "merged"
              ? "Merged"
              : section === "closed"
                ? "Closed"
                : "Open"
        }
      />
      <PageHead
        title={section === "queue" ? "Queue for main" : "Changes"}
        description={
          section === "queue"
            ? "Each entry merges only after required checks pass on the exact result."
            : "Review changes and their exact states."
        }
        actions={
          section === "queue" ? (
            <ReadOnlyAction
              label="Run queue"
              command="layr queue run"
              primary
            />
          ) : (
            <ReadOnlyAction
              label="New change"
              command="layr save -m 'Describe the change'"
              primary
            />
          )
        }
      />
      {section === "queue" ? (
        data.queue.length ? (
          <>
            <Table
              label="Queue"
              headers={[
                "Position",
                "Change",
                "Result state",
                "Status",
                "Probes",
                "Added by",
                "Time",
              ]}
              rows={data.queue.map((q, i) => [
                <Mono>{i + 1}</Mono>,
                q.change_id ? (
                  <a
                    href={projectUrl(
                      project.id,
                      "change/" + q.change_id + "/overview",
                    )}
                  >
                    {short(q.change_id)}
                  </a>
                ) : (
                  <Mono>{short(q.state)}</Mono>
                ),
                <a
                  href={projectUrl(
                    project.id,
                    "state/" + (q.result ?? q.state),
                  )}
                  class="mono"
                >
                  {short(q.result ?? q.state)}
                </a>,
                <Status value={q.status} />,
                <Status
                  value={
                    q.probes &&
                    typeof q.probes === "object" &&
                    !Array.isArray(q.probes)
                      ? String(q.probes.status ?? "Not recorded")
                      : undefined
                  }
                />,
                <Mono>{q.added_by}</Mono>,
                when(q.time),
              ])}
            />
            {data.queue
              .filter((q) => q.reason)
              .map((q) => (
                <div class="callout bad queue-reason">{q.reason}</div>
              ))}
          </>
        ) : (
          <Empty>The queue is empty.</Empty>
        )
      ) : (
        <>
          <div class="filters">
            <input
              class="input grow"
              type="search"
              aria-label="Filter changes"
              placeholder="Filter changes"
              value={filter}
              onInput={(e) => setFilter(e.currentTarget.value)}
            />
            <span class="muted">{rows.length} changes</span>
          </div>
          {rows.length ? (
            <div class="list">
              {rows.map((c) => {
                const v = c.versions.at(-1)!;
                return (
                  <div class="item">
                    <span class="mark open">
                      {section === "merged" ? "✓" : "○"}
                    </span>
                    <div class="main">
                      <a
                        class="title"
                        href={projectUrl(
                          project.id,
                          "change/" + c.change_id + "/overview",
                        )}
                      >
                        {v.state.message || "Change"}
                      </a>
                      <div class="meta">
                        <Mono>{short(c.change_id)}</Mono>
                        <Who name={v.state.author.name} />
                        <span>{v.version}</span>
                        <span>{when(v.state.time)}</span>
                      </div>
                    </div>
                    <div class="side">
                      {section === "merged" ? (
                        <span class="stamp purple">merged</span>
                      ) : (
                        <Status value={changeStatus(c, data.queue)} />
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <Empty>
              No{" "}
              {section === "closed"
                ? "closed"
                : section === "merged"
                  ? "merged"
                  : "open"}{" "}
              changes recorded.
            </Empty>
          )}
        </>
      )}
      <Cmd
        text={section === "queue" ? "layr queue list" : "layr change list"}
      />
    </>
  );
}
export function ChangePage({
  source,
  project,
  id,
  section,
}: {
  source: DataSource;
  project: Project;
  id: string;
  section: string;
}) {
  const [version, setVersion] = useState(""),
    [from, setFrom] = useState(""),
    [to, setTo] = useState(""),
    [quick, setQuick] = useState("Since your review"),
    [diffMode, setDiffMode] = useState("Unified");
  const { data, error } = useData(async () => {
    const [change, queue, operations] = await Promise.all([
      source.changeShow(project.id, id),
      source.queue(project.id),
      source.opLog(project.id),
    ]);
    return { change, queue, operations };
  }, [source, project.id, id]);
  const change = data?.change;
  const { data: viewer } = useData(() => source.you(), [source]);
  const latest = change?.versions.at(-1);
  const reviewed = [...(change?.versions ?? [])]
    .reverse()
    .find(
      (v) =>
        viewer?.login &&
        v.attestations.some(
          (a) => a.kind === "approval" && a.signer === `person:${viewer.login}`,
        ),
    );
  const selected =
    change?.versions.find((v) => v.version === version) ??
    change?.versions.at(-1);
  const first =
    from && from !== "base"
      ? change?.versions.find((v) => v.version === from)
      : from === "base"
        ? undefined
        : quick === "Latest version only"
          ? change?.versions.at(-2)
          : quick === "Since your review"
            ? reviewed
            : undefined;
  const last = change?.versions.find((v) => v.version === to) ?? latest;
  const baseId = first?.state.id ?? last?.state.parents[0];
  const { data: files, error: diffError } = useData(async () => {
    if (!last || section !== "files") return undefined;
    const base = baseId;
    if (first && change) {
      try {
        const native = await source.changeDiff(
          project.id,
          change.change_id,
          first.version,
          last.version,
        );
        return {
          base: base ?? "",
          head: last.state.id,
          files: native.files,
          canonical: true,
          compared: native.compared,
        };
      } catch {}
    }
    if (!base) throw new Error("The selected state has no base.");
    try {
      await source.tree(project.id, base);
    } catch {
      return {
        base,
        head: last.state.id,
        files: [],
        missing: first?.version ?? "Base",
      };
    }
    return source.diff(project.id, base, last.state.id);
  }, [source, project.id, baseId, last?.state.id, section, from, quick]);
  const { data: screens, error: screensError } = useData(async () => {
    if (
      !change?.behavior.base ||
      !change.behavior.head ||
      section !== "behavior"
    )
      return [];
    return source.screens(
      project.id,
      change.behavior.base,
      change.behavior.head,
    );
  }, [
    source,
    project.id,
    change?.behavior.base,
    change?.behavior.head,
    section,
  ]);
  const { data: merge, error: mergeError } = useData(
    async () =>
      latest
        ? source.mergeStatus(
            project.id,
            latest.state.id,
            project.default_line ?? "main",
          )
        : undefined,
    [source, project.id, latest?.state.id],
  );
  if (!data || !selected) return <LoadError error={error} />;
  const c = data.change,
    s = selected.state,
    status = merge?.already_in_target ? "merged" : changeStatus(c, data.queue),
    url = projectUrl(project.id, "change/" + c.change_id);
  const steps = data.operations.flatMap((op) =>
    (op.states ?? []).filter(
      (state) =>
        state.author.session &&
        state.kind === "auto" &&
        (state.change_id === c.change_id || state.line === s.line),
    ),
  );
  const tabs: [string, string][] = [
    ["Overview", url + "/overview"],
    ["Files", url + "/files"],
    ["Behavior", url + "/behavior"],
  ];
  if (steps.length) tabs.push(["Steps", url + "/steps"]);
  const entry = data.queue.find(
    (q) => q.change_id === c.change_id || q.state === latest!.state.id,
  );
  const approvals = latest!.attestations.filter((a) => a.kind === "approval");
  const checks = latest!.attestations.filter((a) => a.kind === "check");
  const selectedChecks = selected.attestations.filter(
    (a) => a.kind === "check",
  );
  const currentStep =
    merge?.already_in_target || status === "merged"
      ? 4
      : merge?.allowed === true
        ? 3
        : 1;
  return (
    <>
      <div class="chead">
        <h1>
          {s.message || "Change"}{" "}
          <span class="id mono">{short(c.change_id)}</span>
        </h1>
        <div class="meta">
          <Status value={status} />
          <Who name={s.author.name} />
          <span>
            wants to merge into <Mono>main</Mono>
          </span>
          <label class="row">
            <span class="sr">Version</span>
            <select
              class="input"
              aria-label="Version"
              value={selected.version}
              onChange={(e) => setVersion(e.currentTarget.value)}
            >
              {c.versions.map((v) => (
                <option value={v.version}>
                  {v.version} · {when(v.state.time)} ·{" "}
                  {v.state.message || "Automatic state"}
                  {viewer?.login &&
                  v.attestations.some(
                    (a) =>
                      a.kind === "approval" &&
                      a.signer === `person:${viewer.login}`,
                  )
                    ? " · you approved"
                    : ""}
                </option>
              ))}
            </select>
          </label>
        </div>
      </div>
      <div class="change-nav">
        <Segments
          items={tabs}
          current={section[0].toUpperCase() + section.slice(1)}
        />
      </div>
      <div class="cols">
        <div class="stack">
          {section === "overview" ? (
            <>
              <Card title={<Who name={s.author.name} />}>
                <p>{s.message || "No description recorded."}</p>
              </Card>
              <Section title="Versions">
                <ul class="tl">
                  {[...c.versions].reverse().map((v) => (
                    <li class="major">
                      <b>{v.version}</b>{" "}
                      <a
                        class="mono"
                        href={projectUrl(project.id, "state/" + v.state.id)}
                      >
                        {short(v.state.id)}
                      </a>
                      <span class="when"> · {when(v.state.time)}</span>
                      <p class="note">{v.state.message}</p>
                    </li>
                  ))}
                </ul>
              </Section>
              <Section title="Checks">
                <Table
                  label="Change checks"
                  headers={["Kind", "Subject", "Signer", "Time"]}
                  rows={selectedChecks.map((a) => [
                    a.kind,
                    <Mono>{short(a.subject.id)}</Mono>,
                    <Mono>{short(a.signer)}</Mono>,
                    when(a.time),
                  ])}
                />
                {!selectedChecks.length && (
                  <Empty>
                    No check attestations recorded for this version.
                  </Empty>
                )}
                <p class="note">
                  Attestations name exact states. Their payloads determine the
                  result.
                </p>
              </Section>
              <Section title="Attestations">
                <Table
                  headers={["Kind", "State", "Signer", "Time"]}
                  rows={selected.attestations.map((a) => [
                    a.kind,
                    <Mono>{short(a.subject.id)}</Mono>,
                    <Mono>{short(a.signer)}</Mono>,
                    when(a.time),
                  ])}
                />
              </Section>
              <Section title="Conversation">
                <Empty>No comments recorded.</Empty>
              </Section>
            </>
          ) : section === "files" ? (
            <>
              <div class="row compare-bar">
                <Dropdown
                  label={
                    quick === "Since your review"
                      ? "Changes since your review"
                      : quick === "All changes"
                        ? "Everything in this change"
                        : quick === "Latest version only"
                          ? "Only the latest update"
                          : "Pick two versions"
                  }
                >
                  {(close) => (
                    <>
                      <button
                        type="button"
                        onClick={() => {
                          setQuick("Since your review");
                          setFrom("");
                          setTo("");
                          close();
                        }}
                      >
                        Since your review
                        <span class="note">
                          From your last review to the latest version.
                        </span>
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          setQuick("All changes");
                          setFrom("");
                          setTo("");
                          close();
                        }}
                      >
                        Everything in this change
                        <span class="note">
                          The latest version against its current parent.
                        </span>
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          setQuick("Latest version only");
                          setFrom("");
                          setTo("");
                          close();
                        }}
                      >
                        Only the latest update
                        <span class="note">The last two versions.</span>
                      </button>
                      <hr />
                      <div class="lbl">Or pick two versions</div>
                      <div class="row">
                        <label class="sr" for="compare-from">
                          From version
                        </label>
                        <select
                          id="compare-from"
                          class="input"
                          value={first?.version ?? "base"}
                          onChange={(e) => {
                            setFrom(e.currentTarget.value);
                            setQuick("Custom");
                          }}
                        >
                          {c.versions.map((v) => (
                            <option value={v.version}>
                              {v.version} · {when(v.state.time)} ·{" "}
                              {v.state.message || "Automatic state"}
                              {viewer?.login &&
                              v.attestations.some(
                                (a) =>
                                  a.kind === "approval" &&
                                  a.signer === `person:${viewer.login}`,
                              )
                                ? " · you approved"
                                : ""}
                            </option>
                          ))}
                          <option value="base">
                            Parent of {last?.version}
                          </option>
                        </select>
                        <span>→</span>
                        <label class="sr" for="compare-to">
                          To version
                        </label>
                        <select
                          id="compare-to"
                          class="input"
                          value={last?.version}
                          onChange={(e) => {
                            setTo(e.currentTarget.value);
                            setQuick("Custom");
                          }}
                        >
                          {c.versions.map((v) => (
                            <option value={v.version}>
                              {v.version} · {when(v.state.time)} ·{" "}
                              {v.state.message || "Automatic state"}
                              {viewer?.login &&
                              v.attestations.some(
                                (a) =>
                                  a.kind === "approval" &&
                                  a.signer === `person:${viewer.login}`,
                              )
                                ? " · you approved"
                                : ""}
                            </option>
                          ))}
                        </select>
                      </div>
                      <Cmd
                        text={
                          first
                            ? `layr change diff ${c.change_id} --from ${first.version} --to ${last?.version}`
                            : `layr diff ${baseId ?? ""} ${last?.state.id ?? ""}`
                        }
                      />
                    </>
                  )}
                </Dropdown>
                {/* TODO: Add aggregate line counts when the canonical model interdiff supplies totals. */}
                {files && !("missing" in files) && (
                  <span class="muted small">
                    {files.files.length} files
                    {"compared" in files && files.compared === false
                      ? " to compare"
                      : ""}
                  </span>
                )}
                <span class="spacer" />
                <ToggleButtons
                  label="Diff view"
                  items={["Unified", "Split"]}
                  value={diffMode}
                  onChange={setDiffMode}
                />
              </div>
              {/* TODO: The records task must supply replay actor and target metadata before a replay note can name them. */}
              {first && !("canonical" in (files ?? {})) && (
                <p class="note compare-note">
                  This compare also shows changes that came from main.
                </p>
              )}
              {files && "missing" in files ? (
                <div class="stack-s">
                  <Empty>
                    {files.missing === "Base"
                      ? "The base is not in this store yet."
                      : `Version ${files.missing} is not in this store yet.`}
                  </Empty>
                  <button
                    class="btn"
                    type="button"
                    onClick={() => setFrom("base")}
                  >
                    Compare {last?.version} against its base
                  </button>
                </div>
              ) : files && "canonical" in files && first && last ? (
                <InterdiffBrowser
                  key={`${first.version}:${last.version}`}
                  source={source}
                  project={project.id}
                  change={c.change_id}
                  from={first.version}
                  to={last.version}
                  files={files.files}
                  mode={diffMode}
                />
              ) : files ? (
                <DiffBrowser
                  source={source}
                  project={project.id}
                  base={files.base}
                  head={files.head}
                  files={files.files}
                  mode={diffMode}
                />
              ) : (
                <LoadError error={diffError} />
              )}
            </>
          ) : section === "behavior" ? (
            <>
              <h2>
                Behavior diff: base <Mono>{short(c.behavior.base)}</Mono> →{" "}
                <Mono>{short(c.behavior.head)}</Mono>
              </h2>
              <BehaviorCards behavior={c.behavior} screens={screens} />
              {screensError && (
                <Empty>UI diffs are not available in the browser yet.</Empty>
              )}
            </>
          ) : section === "steps" && steps.length ? (
            <>
              <div class="callout">
                {steps.length} steps. Every step is a state you can open.
              </div>
              <ul class="tl">
                {steps.map((step) => (
                  <li>
                    <span class="when">{when(step.time)}</span>{" "}
                    <b>{step.author.session}</b>
                    <p>{step.message || "Automatic state"}</p>
                    <a href={projectUrl(project.id, "state/" + step.id)}>
                      View diff
                    </a>{" "}
                    <Mono>{short(step.id)}</Mono>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <Empty>No agent steps recorded for this change.</Empty>
          )}
        </div>
        <aside class="side" aria-label="Merge checklist">
          <section>
            <h2 class="side-title">
              Merge into main{" "}
              <a href={projectUrl(project.id, "settings/protection")}>Rules</a>
            </h2>
            <div class="stepper" role="list" aria-label="Merge progress">
              {["Saved", "Checks", "Approvals", "Queue", "Merged"].map(
                (label, i) => (
                  <div
                    class={`step ${i < currentStep ? "done" : i === currentStep ? "now" : ""}`}
                    role="listitem"
                  >
                    <i />
                    <span>{label}</span>
                  </div>
                ),
              )}
            </div>
            <ul class="stack-s">
              <li>✓ {c.versions.length} saved versions</li>
              {merge ? (
                <>
                  <li>
                    {merge.approvals.filter((a) => a.met === true).length} of{" "}
                    {merge.approvals_needed} approvals from{" "}
                    {merge.approvers.length
                      ? merge.approvers.join(", ")
                      : "maintainers"}{" "}
                    on {latest!.version}
                  </li>
                  {merge.checks.map((check) => (
                    <li>
                      <Status value={check.result} /> {check.name}
                    </li>
                  ))}
                  {merge.path_requirements.map((path) => (
                    <li>
                      <span
                        class={
                          path.met === true
                            ? "ok"
                            : path.met === false
                              ? "bad"
                              : "muted"
                        }
                      >
                        {path.met === true
                          ? "✓"
                          : path.met === false
                            ? "✕"
                            : "●"}
                      </span>{" "}
                      <Mono>{path.path}</Mono> → {path.role}:{" "}
                      {path.met === null
                        ? "role not known here"
                        : path.met
                          ? "met"
                          : "not met"}
                    </li>
                  ))}
                  {merge.undecided_reasons.map((reason) => (
                    <li class="muted">{reason}</li>
                  ))}
                </>
              ) : (
                <>
                  <li>
                    {checks.length} check attestations on {latest!.version}
                  </li>
                  <li>
                    {approvals.length} approval attestations on{" "}
                    {latest!.version}
                  </li>
                  <li class="muted">
                    {mergeError?.message ??
                      "Merge evaluation is not available."}
                  </li>
                </>
              )}
              <li>
                {entry ? (
                  <Status value={entry.status} />
                ) : (
                  <span class="muted">Not in queue</span>
                )}
              </li>
            </ul>
            <ReadOnlyAction
              label={status === "merged" ? "Merged" : "Merge"}
              command={`layr queue add ${latest!.state.id}`}
              primary
            />
            <p class="note">Read only here. Use the CLI command.</p>
          </section>
          <section>
            <h2 class="side-title">Preview</h2>
            <p class="note">
              {project.images?.some((i) => i.state === s.id)
                ? "Images are available in Deploy."
                : "No preview image recorded."}
            </p>
          </section>
        </aside>
      </div>
    </>
  );
}
