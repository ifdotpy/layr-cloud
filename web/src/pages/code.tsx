import { useState } from "preact/hooks";
import type {
  DataSource,
  Project,
  State,
  Tree,
  FileContent,
} from "../data/source";
import type { Route } from "../router";
import { projectUrl, queryUrl } from "../router";
import {
  Avatar,
  Badge,
  Card,
  Cmd,
  CodeText,
  Dialog,
  Dropdown,
  Empty,
  Icon,
  LoadError,
  Mono,
  PageHead,
  ReadOnlyAction,
  Section,
  Status,
  Table,
  ToggleButtons,
  Who,
  bytes,
  short,
  useData,
  when,
} from "../components/ui";
import { DiffBrowser, DiffFiles } from "../components/diff";
import { plural, shortText } from "../labels";

export function CodePage({
  source,
  project,
  route,
}: {
  source: DataSource;
  project: Project;
  route: Route;
}) {
  const [switchTab, setSwitchTab] = useState("Lines"),
    [filter, setFilter] = useState("");
  const { data, error } = useData(async () => {
    const [lines, refs, copies] = await Promise.all([
      source.lines(project.id),
      source.refs(project.id),
      source.copies(project.id),
    ]);
    const line =
      lines.find((l) => l.name === route.line) ??
      lines.find((l) => l.name === (project.default_line ?? "main")) ??
      lines[0];
    if (!line) return { lines, refs, copies };
    const states = await source.log(project.id, line.name);
    const state = route.state ?? line.head;
    let tree: Tree | undefined,
      file: FileContent | undefined,
      readme: FileContent | undefined,
      contentError = "";
    try {
      tree = await source.tree(project.id, state, route.path);
    } catch (treeError) {
      try {
        file = await source.file(project.id, state, route.path ?? "");
      } catch {
        contentError =
          treeError instanceof Error
            ? treeError.message
            : "Files are unavailable.";
      }
    }
    if (tree?.entries.some((e) => e.name === "README.md")) {
      try {
        readme = await source.file(
          project.id,
          state,
          tree.entries.find((e) => e.name === "README.md")!.path,
        );
      } catch {}
    }
    return {
      lines,
      refs,
      copies,
      line,
      states,
      state,
      tree,
      file,
      readme,
      contentError,
    };
  }, [source, project.id, route.line, route.state, route.path]);
  if (!data) return <LoadError error={error} />;
  if (!data.line) return <Empty>No lines recorded.</Empty>;
  const head = data.states?.find((s) => s.id === data.state),
    base = projectUrl(project.id, "code");
  const link = (path: string, state = data.state, line = data.line?.name) =>
    queryUrl(base, { path, state, line });
  return (
    <div class="cols">
      <div class="stack">
        <div class="row">
          <Dropdown label={`line: ${data.line.name}`}>
            {(close) => (
              <>
                <ToggleButtons
                  label="Line selector"
                  items={["Lines", "Refs", "Views"]}
                  value={switchTab}
                  onChange={setSwitchTab}
                />
                <input
                  class="input"
                  type="search"
                  aria-label="Filter lines and refs"
                  placeholder="Filter"
                  value={filter}
                  onInput={(e) => setFilter(e.currentTarget.value)}
                />
                {switchTab === "Lines" ? (
                  data.lines
                    .filter((l) => l.name.includes(filter))
                    .map((l) => (
                      <a href={link("", l.head, l.name)} onClick={close}>
                        {l.name}
                      </a>
                    ))
                ) : switchTab === "Refs" ? (
                  data.refs
                    .filter((r) => r.name.includes(filter))
                    .map((r) => (
                      <a href={link("", r.target)} onClick={close}>
                        {r.name}
                      </a>
                    ))
                ) : (
                  <p class="note">No views recorded.</p>
                )}
              </>
            )}
          </Dropdown>
          <a class="btn ghost" href={projectUrl(project.id, "lines")}>
            {plural(data.lines.length, "line")}
          </a>
          <a
            class="btn ghost"
            href={projectUrl(project.id, "deploy/environments")}
          >
            {plural(data.refs.length, "ref")}
          </a>
          <a class="btn ghost" href={projectUrl(project.id, "states")}>
            {plural(data.states?.length ?? 0, "state")}
          </a>
          <a class="btn ghost" href={projectUrl(project.id, "overlays")}>
            {plural(project.overlays?.length ?? 0, "overlay")}
          </a>
          <span class="spacer" />
          {data.tree && (
            <input
              class="input file-filter"
              aria-label="Go to file"
              placeholder="Go to file"
              value={filter}
              onInput={(e) => setFilter(e.currentTarget.value)}
            />
          )}
        </div>
        <div>
          <div class="headrow">
            {head && (
              <>
                <Avatar name={head.author.name} />
                <b>{head.author.name}</b>
                <span class="msg muted">
                  {head.message || "Automatic state"}
                </span>
              </>
            )}
            <a
              href={projectUrl(project.id, "state/" + data.state)}
              class="mono"
            >
              {short(data.state)}
            </a>
            <span class="muted">{when(head?.time)}</span>
          </div>
          {data.contentError ? (
            <Empty>{data.contentError}</Empty>
          ) : data.file ? (
            <div class="frame">
              <div class="fh">
                {data.file.path}
                <span class="spacer" />
                {bytes(data.file.size)}
              </div>
              {data.file.text === undefined ? (
                <Empty>
                  File contents are not available. Size: {bytes(data.file.size)}
                  .
                </Empty>
              ) : (
                <CodeText text={data.file.text} />
              )}
            </div>
          ) : (
            data.tree && (
              <>
                <Table
                  headers={["Name", "State", "Size"]}
                  label="Files"
                  rows={[
                    ...(route.path
                      ? [
                          [
                            <a
                              href={link(
                                route.path.split("/").slice(0, -1).join("/"),
                              )}
                            >
                              ..
                            </a>,
                            "",
                            "",
                          ],
                        ]
                      : []),
                    ...data.tree.entries
                      .filter((e) =>
                        e.name.toLowerCase().includes(filter.toLowerCase()),
                      )
                      .map((e) => [
                        <span class="filecell">
                          <Icon
                            name={e.kind === "directory" ? "folder" : "file"}
                          />
                          <a href={link(e.path)}>
                            {e.name}
                            {e.kind === "directory" ? "/" : ""}
                          </a>
                          {e.overlay && (
                            <a
                              href={projectUrl(
                                project.id,
                                "overlay/" + encodeURIComponent(e.overlay),
                              )}
                            >
                              <Badge>overlay: {e.overlay}</Badge>
                            </a>
                          )}
                          {e.derived && <Badge>derived</Badge>}
                        </span>,
                        <a
                          class="mono"
                          href={projectUrl(
                            project.id,
                            "state/" + (e.state ?? data.state),
                          )}
                        >
                          {short(e.state ?? data.state)}
                        </a>,
                        <Mono>
                          {e.kind === "directory" ? "" : bytes(e.size)}
                        </Mono>,
                      ]),
                  ]}
                />
                {data.tree.hidden_overlays?.map((name) => (
                  <p class="note">
                    Overlay {name} is hidden. Your key has no access.
                  </p>
                ))}
              </>
            )
          )}
        </div>
        {data.readme ? (
          <Card title="README.md">
            <Markdown text={data.readme.text ?? ""} />
          </Card>
        ) : (
          data.tree && (
            <Card title="README.md">
              <p class="note">No README.md in this folder.</p>
            </Card>
          )
        )}
      </div>
      <aside class="side">
        <section>
          <h2 class="side-title">About</h2>
          <p class="note">
            {project.description ?? "No project description recorded."}
          </p>
        </section>
        <section>
          <h2 class="side-title">
            Overlays{" "}
            <a href={projectUrl(project.id, "settings/private-files")}>
              Manage
            </a>
          </h2>
          {project.overlays?.length ? (
            <ul>
              {project.overlays.map((o) => (
                <li>
                  <Mono>{o.name}</Mono>
                  <span class="r">{plural(o.readers.length, "reader")}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p class="note">No overlays recorded.</p>
          )}
        </section>
        <section>
          <h2 class="side-title">
            Environments{" "}
            <a href={projectUrl(project.id, "deploy/environments")}>All</a>
          </h2>
          <ul>
            {data.refs
              .filter((r) => r.name.startsWith("env/"))
              .map((r) => (
                <li>
                  <Mono>{r.name}</Mono>
                  <span class="r">→ {short(r.target)}</span>
                </li>
              ))}
          </ul>
        </section>
        <section>
          <h2 class="side-title">
            Copies <a href={projectUrl(project.id, "settings/copies")}>All</a>
          </h2>
          {data.copies.length ? (
            <ul>
              {data.copies.map((c) => (
                <li>
                  {c.name}
                  <span class="r">{c.kind}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p class="note">No copies recorded.</p>
          )}
        </section>
        <section>
          <h2 class="side-title">
            Runnables{" "}
            <a href={projectUrl(project.id, "deploy/images")}>Images</a>
          </h2>
          <p class="note">
            {project.images?.length
              ? `${project.images.length} images recorded.`
              : "No images recorded."}
          </p>
        </section>
      </aside>
    </div>
  );
}
/** Minimal markdown supports source README headings, lists and code, without HTML execution. */
function Markdown({ text }: { text: string }) {
  let code = false,
    buffer: string[] = [];
  const nodes = [];
  for (const [i, line] of text.split("\n").entries()) {
    if (line.startsWith("```")) {
      if (code) {
        nodes.push(<pre key={i}>{buffer.join("\n")}</pre>);
        buffer = [];
      }
      code = !code;
      continue;
    }
    if (code) {
      buffer.push(line);
      continue;
    }
    if (line.startsWith("# ")) nodes.push(<h1 key={i}>{line.slice(2)}</h1>);
    else if (line.startsWith("## "))
      nodes.push(<h2 key={i}>{line.slice(3)}</h2>);
    else if (line.startsWith("- "))
      nodes.push(<p key={i}>• {line.slice(2)}</p>);
    else if (line) nodes.push(<p key={i}>{line}</p>);
  }
  if (buffer.length) nodes.push(<pre>{buffer.join("\n")}</pre>);
  return <div class="md">{nodes}</div>;
}
export function LinesPage({
  source,
  project,
}: {
  source: DataSource;
  project: Project;
}) {
  const [filter, setFilter] = useState(""),
    [agents, setAgents] = useState(false),
    [diskMode, setDiskMode] = useState("Layr");
  const { data, error } = useData(async () => {
    const [lines, machines, operations] = await Promise.all([
      source.lines(project.id),
      source.machines().catch(() => []),
      source.opLog(project.id).catch(() => []),
    ]);
    return { lines, machines, operations };
  }, [source, project.id]);
  if (!data) return <LoadError error={error} />;
  const agentLines = data.lines.filter((l) => l.name.startsWith("agent-"));
  const rows = data.lines.filter(
    (l) => (agents || !l.name.startsWith("agent-")) && l.name.includes(filter),
  );
  const disk = project.disk ?? [],
    total = disk.reduce(
      (n, d) =>
        n +
        (diskMode === "Layr" ? d.shared_bytes : d.separate_bytes) +
        d.cache_bytes,
      0,
    ),
    max = Math.max(1, ...disk.map((d) => d.separate_bytes + d.cache_bytes));
  return (
    <div class="stack">
      <PageHead
        title="Lines"
        description="Each line is a real folder with its own caches."
        actions={
          <ReadOnlyAction
            label="New line"
            command="layr line new topic main"
            primary
          />
        }
      />
      <div class="filters">
        <input
          class="input grow"
          type="search"
          aria-label="Filter lines"
          placeholder="Filter lines"
          value={filter}
          onInput={(e) => setFilter(e.currentTarget.value)}
        />
        {agentLines.length > 0 && (
          <button
            class="btn"
            type="button"
            aria-expanded={agents}
            onClick={() => setAgents(!agents)}
          >
            {agents ? "Hide" : "Show"} {plural(agentLines.length, "agent line")}
          </button>
        )}
      </div>
      <Table
        headers={["Line", "Owner", "Head", "Machine"]}
        stacked
        label="Lines"
        rows={rows.map((l) => {
          const owner = data.operations.find(
            (operation) =>
              operation.machine === l.machine &&
              operation.actor.uid === l.owner &&
              operation.actor.name,
          )?.actor.name;
          const machine = data.machines.find((item) => item.id === l.machine);
          return [
            <a href={queryUrl(projectUrl(project.id), { line: l.name })}>
              {l.name}
            </a>,
            owner ? (
              <Who name={owner} />
            ) : (
              <span class="muted">uid {l.owner}</span>
            ),
            <a href={projectUrl(project.id, "state/" + l.head)} class="mono">
              {short(l.head)}
            </a>,
            <span title={short(l.machine)}>
              {machine?.name || `machine ${short(l.machine).slice(0, 7)}`}
            </span>,
          ];
        })}
      />
      {disk.length ? (
        <Card title="Disk use">
          <div class="row">
            <div class="big">{bytes(total)}</div>
            <span class="spacer" />
            <ToggleButtons
              label="Disk use mode"
              items={["Layr", "As separate copies"]}
              value={diskMode}
              onChange={setDiskMode}
            />
          </div>
          <div class="bars">
            {disk.map((d) => (
              <div class="bar-row">
                <span>{d.line}</span>
                <span class="bar">
                  <i
                    style={{
                      width: `${(((diskMode === "Layr" ? d.shared_bytes : d.separate_bytes) + d.cache_bytes) / max) * 100}%`,
                    }}
                  />
                </span>
                <span class="num">
                  {bytes(
                    (diskMode === "Layr" ? d.shared_bytes : d.separate_bytes) +
                      d.cache_bytes,
                  )}
                </span>
              </div>
            ))}
          </div>
        </Card>
      ) : (
        <p class="note">Disk use is not recorded for this store.</p>
      )}
    </div>
  );
}
export function StatesPage({
  source,
  project,
  route,
}: {
  source: DataSource;
  project: Project;
  route: Route;
}) {
  const [auto, setAuto] = useState(false),
    [bisect, setBisect] = useState(false),
    [limit, setLimit] = useState(100);
  const { data, error } = useData(
    () =>
      source.log(project.id, route.line ?? project.default_line ?? "main", {
        auto,
        limit,
      }),
    [source, project.id, route.line, auto, limit],
  );
  if (!data) return <LoadError error={error} />;
  return (
    <>
      <PageHead
        title="States"
        description="Each state is a frozen, signed copy of the whole folder."
        actions={
          <button class="btn" type="button" onClick={() => setBisect(true)}>
            Find the state that broke it
          </button>
        }
      />
      <div class="filters">
        <label class="toggle">
          <input
            type="checkbox"
            checked={auto}
            onChange={(e) => setAuto(e.currentTarget.checked)}
          />
          Show automatic states
        </label>
        <span class="spacer" />
        <span class="muted">{plural(data.length, "state")} loaded</span>
      </div>
      <div class="list">
        {data.map((s) => (
          <div class={`item ${s.kind === "auto" ? "auto" : ""}`}>
            <div class="main">
              <a class="title" href={projectUrl(project.id, "state/" + s.id)}>
                {s.message || (s.kind === "auto" ? "Automatic state" : "State")}
              </a>
              <div class="meta">
                <Who name={s.author.name} />
                <span>{when(s.time)}</span>
                {s.kind === "auto" && <Badge>auto</Badge>}
              </div>
            </div>
            <div class="side">
              <a class="mono" href={projectUrl(project.id, "state/" + s.id)}>
                {short(s.id)}
              </a>
              <a
                class="btn sm"
                href={queryUrl(projectUrl(project.id), { state: s.id })}
              >
                Browse files
              </a>
            </div>
          </div>
        ))}
      </div>
      {data.length >= limit && (
        <button class="btn" type="button" onClick={() => setLimit(limit + 100)}>
          Load more states
        </button>
      )}
      <Cmd text={`layr log${auto ? " --auto" : ""}`} />
      <Dialog
        title="Find the state that broke it"
        open={bisect}
        onClose={() => setBisect(false)}
      >
        {project.bisect ? (
          <div class="stack-s">
            <p>
              Good <Mono>{short(project.bisect.good)}</Mono> → bad{" "}
              <Mono>{short(project.bisect.bad)}</Mono>
            </p>
            {project.bisect.first_bad && (
              <div class="callout bad">
                First bad:{" "}
                <a
                  href={projectUrl(
                    project.id,
                    "state/" + project.bisect.first_bad,
                  )}
                >
                  {short(project.bisect.first_bad)}
                </a>
              </div>
            )}
            <Table
              headers={["State", "Result", "Reused"]}
              rows={project.bisect.steps.map((s) => [
                <Mono>{short(s.state)}</Mono>,
                <Status value={s.result} />,
                s.reused ? "yes" : "no",
              ])}
            />
            <Cmd text={project.bisect.command.join(" ")} />
          </div>
        ) : (
          <Empty>
            No bisect results recorded. Use the CLI to search exact states.
          </Empty>
        )}
        <Cmd text="layr bisect run --auto -- cargo test" />
      </Dialog>
    </>
  );
}
export function StatePage({
  source,
  project,
  stateId,
}: {
  source: DataSource;
  project: Project;
  stateId: string;
}) {
  const { data, error } = useData(async () => {
    const states = await source.log(project.id, stateId, {
      auto: true,
      limit: 1,
    });
    const state = states.find((s) => s.id === stateId);
    if (!state) throw new Error("State not found.");
    const attestations = await source.attestations(project.id, state.id);
    let diff;
    let diffError = "";
    if (state.parents[0])
      try {
        diff = await source.diff(project.id, state.parents[0], state.id);
      } catch (e) {
        diffError =
          e instanceof Error ? e.message : "File differences are unavailable.";
      }
    return { state, attestations, diff, diffError };
  }, [source, project.id, stateId]);
  if (!data) return <LoadError error={error} />;
  const { state: s } = data;
  return (
    <div class="stack">
      <PageHead
        title={s.message || "Automatic state"}
        description={
          <>
            <Who name={s.author.name} />
            {when(s.time)} · <Mono>{s.id}</Mono>
          </>
        }
      />
      <p class="note">
        Parents:{" "}
        {s.parents.length
          ? s.parents.map((id) => (
              <a href={projectUrl(project.id, "state/" + id)} class="mono">
                {short(id)}{" "}
              </a>
            ))
          : "none"}
      </p>
      <div class="row">
        <ReadOnlyAction
          label="Restore folder to this state"
          command={`layr restore --source ${s.id} .`}
        />
        <ReadOnlyAction label="Pick onto line…" command={`layr pick ${s.id}`} />
        <ReadOnlyAction label="Invert" command={`layr invert ${s.id}`} />
        <ReadOnlyAction
          label="New line from here"
          command={`layr line new topic ${s.id}`}
        />
      </div>
      <Section title="Attestations">
        <Table
          headers={["Kind", "Subject", "Signer", "Time"]}
          rows={data.attestations.map((a) => [
            a.kind,
            <Mono>{short(a.subject.id)}</Mono>,
            <Mono>{short(a.signer)}</Mono>,
            when(a.time),
          ])}
        />
        {!data.attestations.length && <Empty>No attestations recorded.</Empty>}
      </Section>
      {data.diff ? (
        <DiffBrowser
          source={source}
          project={project.id}
          base={data.diff.base}
          head={data.diff.head}
          files={data.diff.files}
        />
      ) : (
        <Empty>{data.diffError || "This state has no parent."}</Empty>
      )}
    </div>
  );
}
