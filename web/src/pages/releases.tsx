import { useState } from "preact/hooks";
import type { Check, DataSource, Project } from "../data/source";
import { checkNames, releases } from "../data/releases";
import { machineLabel, personLabel, plural, shortText } from "../labels";
import { projectUrl } from "../router";
import { Badge, Cmd, Empty, LoadError, Mono, PageHead, ReadOnlyAction, Segments, Status, Table, Who, bytes, short, useData, when } from "../components/ui";
import { DiffBrowser } from "../components/diff";

export function DeployNav({ project, current }: { project: string; current: string }) {
  return <Segments items={[
    ["Environments", projectUrl(project, "deploy/environments")],
    ["Releases", projectUrl(project, "deploy/releases")],
    ["Images", projectUrl(project, "deploy/images")],
  ]} current={current} />;
}

function CheckSummary({ checks }: { checks: Check[] }) {
  return checks.length ? <span>{checks.filter((c) => c.exit === 0).length} of {checks.length} passed</span> : <span class="muted">No checks recorded</span>;
}

export function ReleasesPage({ source, project, reference }: { source: DataSource; project: Project; reference?: string }) {
  const [compare, setCompare] = useState(false);
  const { data, error } = useData(async () => {
    const [rows, machines] = await Promise.all([releases(source, project.id, project.checks), source.machines()]);
    return { rows, machines };
  }, [source, project.id]);
  if (!data) return <LoadError error={error} />;
  const url = (name: string) => projectUrl(project.id, "deploy/release/" + encodeURIComponent(name));
  const latest = data.rows.every((row) => row.operation) ? data.rows[0]?.ref.name : undefined;
  const row = data.rows.find((row) => row.ref.name === reference);
  if (reference && !row) return <><DeployNav project={project.id} current="Releases" /><Empty>Release not found.</Empty></>;
  if (!row) return <>
    <DeployNav project={project.id} current="Releases" />
    <PageHead title="Releases" description="Each release names one exact state. Its notes are the changes since the release before it."
      actions={<ReadOnlyAction label="New release" command={`layr ref set release/<version> ${project.default_line ?? "main"}`} primary />} />
    <Table label="Releases" headers={["Release", "Changes", "Checks", "By", "Date", "Where"]} rows={data.rows.map((release) => [
      <><a class="mono" href={url(release.ref.name)}>{release.ref.name}</a>{release.ref.name === latest && <> <Badge kind="blue">latest</Badge></>}</>,
      release.notes ? <a href={url(release.ref.name)}>{release.notes.length}</a> : <span class="muted" title={release.error}>Not available</span>,
      <CheckSummary checks={release.checks} />,
      release.operation ? <Who name={release.operation.actor.name || personLabel(String(release.operation.actor.uid))} /> : <span class="muted">Not recorded</span>,
      release.operation ? when(release.operation.time) : <span class="muted">Not recorded</span>,
      release.environments.length ? release.environments.map((env) => <a class="ref" href={projectUrl(project.id, "deploy/environment/" + encodeURIComponent(env.name))}>{env.name}</a>) : <span class="muted">·</span>,
    ])} />
    {!data.rows.length && <Empty>No releases recorded.</Empty>}
  </>;
  const images = (project.images ?? []).filter((image) => image.state === row.ref.target || (!image.state && image.ref === row.ref.name));
  const checks = row.checks.map((check) => ({ check, names: checkNames(check, project.config) }));
  const hasCheckNames = checks.some(({ names }) => names.length > 0);
  return <>
    <DeployNav project={project.id} current="Releases" />
    <p class="note"><a href={projectUrl(project.id, "deploy/releases")}>Releases</a> /</p>
    <PageHead title={<><span class="mono release-name">{row.ref.name}</span>{row.ref.name === latest && <> <Badge kind="blue">latest</Badge></>}</>}
      description={<>{row.operation ? <>{row.operation.actor.name || personLabel(String(row.operation.actor.uid))} released state </> : "State "}<a class="mono" href={projectUrl(project.id, "state/" + row.ref.target)}>{short(row.ref.target)}</a>{row.operation && <> on {when(row.operation.time)}</>}.{row.environments.length > 0 && <> In {row.environments.map((env) => <a class="ref" href={projectUrl(project.id, "deploy/environment/" + encodeURIComponent(env.name))}>{env.name}</a>)}.</>}</>} />
    <div class="cols">
      <div class="stack">
        <section class="sect">
          <h2>{row.notes ? plural(row.notes.length, "change") : "Changes"}{row.previous ? <> since <a class="mono" href={url(row.previous.name)}>{row.previous.name}</a></> : " in this release"}</h2>
          {row.notes ? <>
            <Table label="Release notes" headers={["Change", "By", "State"]} rows={row.notes.map((state) => [
              <a href={projectUrl(project.id, "change/" + state.change_id)}>{shortText(state.message) || short(state.change_id)}</a>,
              <Who name={state.author.name || personLabel(String(state.author.uid ?? "")) || "Not recorded"} />,
              <a class="mono" href={projectUrl(project.id, "state/" + state.id)}>{short(state.id)}</a>,
            ])} />
            {!row.notes.length && <Empty>{row.previous ? "No changes since the previous release." : "No changes recorded for this release."}</Empty>}
          </> : <Empty>{row.error}</Empty>}
        </section>
        {row.previous && <section class="sect">
          <button class="btn" type="button" aria-expanded={compare} onClick={() => setCompare(!compare)}>Compare files with {row.previous.name}</button>
          {compare && <ReleaseFiles key={row.previous.target + row.ref.target} source={source} project={project.id} base={row.previous.target} head={row.ref.target} />}
        </section>}
        <section class="sect">
          <h2>Checks on <Mono>{short(row.ref.target)}</Mono></h2>
          <Table label="Release checks" headers={[...(hasCheckNames ? ["Check"] : []), "Result", "Runner", "Command"]} rows={checks.map(({ check, names }) => [
            ...(hasCheckNames ? [names.length ? <Mono>{names.join(", ")}</Mono> : <span class="muted">·</span>] : []),
            <Status value={check.exit === undefined ? "not run" : check.exit === 0 ? "passed" : "failed"} />,
            check.machine ? machineLabel(check.machine, data.machines) : <span class="muted">Not recorded</span>,
            <Mono>{check.argv.join(" ")}</Mono>,
          ])} />
          {!row.checks.length && <Empty>No checks recorded for this state.</Empty>}
        </section>
      </div>
      <aside class="side">
        <section><h2 class="side-title">Get this release</h2><Cmd text={`layr line new try-${row.ref.name.slice(8)} ${row.ref.name}`} /></section>
        <section><h2 class="side-title">Images</h2>
          {images.length ? <ul>{images.map((image) => <li><Mono>{image.name}:{image.tag}</Mono><span class="muted">{image.digest ?? "Digest not recorded"} · {bytes(image.bytes)}</span></li>)}</ul> : <p class="note">No images recorded for this state.</p>}
        </section>
      </aside>
    </div>
  </>;
}

function ReleaseFiles({ source, project, base, head }: { source: DataSource; project: string; base: string; head: string }) {
  const { data, error } = useData(() => source.diff(project, base, head), [source, project, base, head]);
  if (error) return <Empty>{error}</Empty>;
  if (!data) return <LoadError />;
  return <DiffBrowser source={source} project={project} base={base} head={head} files={data.files} />;
}
