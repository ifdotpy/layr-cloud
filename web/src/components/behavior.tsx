import { useState } from "preact/hooks";
import type { Behavior, Json, ProbeResult, ScreenDiff } from "../data/source";
import {
  Badge,
  Card,
  Cmd,
  Empty,
  Mono,
  Status,
  Table,
  ToggleButtons,
  short,
} from "./ui";
import { plural } from "../labels";

const object = (value: Json | undefined): Record<string, Json> =>
  value && typeof value === "object" && !Array.isArray(value) ? value : {};
interface Metric {
  name: string;
  before: number;
  after: number;
  relative?: number;
}
function metrics(probe: ProbeResult): Metric[] {
  const out: Metric[] = [];
  for (const group of probe.changes ?? []) {
    const g = object(group);
    for (const change of Array.isArray(g.changes) ? g.changes : [group]) {
      const c = object(change);
      if (typeof c.before === "number" && typeof c.after === "number")
        out.push({
          name: String(c.metric ?? g.path ?? "value"),
          before: c.before,
          after: c.after,
          relative:
            typeof c.relative_change === "number"
              ? c.relative_change
              : undefined,
        });
    }
  }
  return out;
}
export function BehaviorCards({
  behavior,
  screens = [],
}: {
  behavior: Behavior;
  screens?: ScreenDiff[];
}) {
  return (
    <div class="stack">
      {behavior.reason && <div class="callout">{behavior.reason}</div>}
      {screens.length > 0 && <VisualDiff screens={screens} />}{" "}
      {(behavior.probes ?? []).filter((p) => !screens.some((s) => s.probe === p.probe)).map((p) => (
        <ProbeCard key={p.probe} probe={p} behavior={behavior} />
      ))}
      {!behavior.probes?.length && <Empty>No probe results recorded.</Empty>}
    </div>
  );
}
function ProbeCard({
  probe,
  behavior,
}: {
  probe: ProbeResult;
  behavior: Behavior;
}) {
  const rows = metrics(probe),
    [mode, setMode] = useState("Base");
  const base = object(probe.base),
    head = object(probe.head);
  const argv = Array.isArray(head.argv)
    ? head.argv.join(" ")
    : Array.isArray(base.argv)
      ? base.argv.join(" ")
      : "";
  return (
    <Card
      title={<Mono>{probe.probe}</Mono>}
      actions={<Status value={probe.status} />}
    >
      <div class="stack-s">
        {rows.length > 0 ? (
          <>
            <div class="row">
              <span class="note">Toggle to see the change</span>
              <span class="spacer" />
              <ToggleButtons
                label={`${probe.probe} metric state`}
                items={["Base", "Head"]}
                value={mode}
                onChange={setMode}
              />
            </div>
            <div class="bars">
              {rows.map((m, i) => (
                <div class="bar-row">
                  <span>{m.name}</span>
                  <span class="bar">
                    <i
                      style={{
                        width: `${((mode === "Base" ? m.before : m.after) / Math.max(1, m.before, m.after)) * 87}%`,
                      }}
                    />
                  </span>
                  <span class="num mono">
                    {mode === "Base" ? m.before : m.after}
                  </span>
                </div>
              ))}
            </div>
            <Table
              headers={["Metric", "Base", "Head", "Change"]}
              rows={rows.map((m) => [
                m.name,
                <Mono>{m.before}</Mono>,
                <Mono>{m.after}</Mono>,
                m.relative === undefined
                  ? "Not recorded"
                  : `${m.relative >= 0 ? "+" : ""}${(m.relative * 100).toFixed(1)}%`,
              ])}
            />
          </>
        ) : probe.changes?.length ? (
          <pre class="code probe-json">
            {JSON.stringify(probe.changes, null, 2)}
          </pre>
        ) : (
          <p class="note">
            {probe.status === "same"
              ? "No change."
              : probe.status === "not run"
                ? "No stored run on these exact states."
                : probe.reason
                  ? String(probe.reason)
                  : "No output differences recorded."}
          </p>
        )}
        {argv && (
          <p class="note">
            <Mono>{argv}</Mono>
          </p>
        )}
        <p class="note">
          Exact states: {short(behavior.base)} → {short(behavior.head)}
        </p>
        <Cmd
          text={`layr probe diff ${behavior.base ?? ""} ${behavior.head ?? ""} --probe ${probe.probe}`}
        />
      </div>
    </Card>
  );
}
export function VisualDiff({ screens }: { screens: ScreenDiff[] }) {
  const [index, setIndex] = useState(0),
    [mode, setMode] = useState("Side by side"),
    [range, setRange] = useState(50);
  const s = screens[Math.min(index, screens.length - 1)];
  if (!s) return null;
  const modes = ["Side by side", "Swipe", "Onion skin", "Difference", "Structure"];
  const width = Math.max(s.before?.width ?? 0, s.after?.width ?? 0),
    height = Math.max(s.before?.height ?? 0, s.after?.height ?? 0);
  const both = Boolean(s.base_url && s.head_url);
  const image = (side: "base" | "head") => {
    const url = side === "base" ? s.base_url : s.head_url;
    const dimensions = side === "base" ? s.before : s.after;
    return url && dimensions ? <img src={url} alt={`${side === "base" ? "Base" : "Head"} screen of ${s.name}`} width={dimensions.width} height={dimensions.height} />
      : <p class="note">No {side} PNG captured.</p>;
  };
  return (
    <Card title="UI diffs" actions={<Badge>{screens.filter((s) => s.changed).length} of {plural(screens.length, "screen")} changed</Badge>}>
      <div class="vd">
        <aside class="side vd-list" aria-label="UI differences">
          <section><h3>Screens</h3><ul>
            {screens.map((screen, i) => (
              <li key={`${screen.probe}:${screen.path}`}>
                <button class="btn ghost sm" type="button" aria-pressed={index === i} onClick={() => setIndex(i)}>{screen.name}</button>
                <span class="r">{screen.changed_share === null ? "Structure only" : `${(screen.changed_share * 100).toFixed(1)}% of pixels`} · {screen.changed ? "changed" : "unchanged"}</span>
              </li>
            ))}
          </ul></section>
        </aside>
        <div class="vd-main">
          <div class="row"><b class="mono">{s.name}</b><span class="spacer" />
            <ToggleButtons label="UI diff view" items={modes} value={mode} onChange={setMode} />
          </div>
          {mode === "Structure" ? (
            s.structure_available ? <div class="stack-s">
              <p>{plural(s.elements_added, "element")} added · {plural(s.elements_removed, "element")} removed · {plural(s.text_changes, "text change")}</p>
              {s.structure_diff ? <pre class="code structure-diff" tabIndex={0} role="region" aria-label={`Structure diff of ${s.name}`}>{s.structure_diff.split("\n").map((line, i) => <div key={i} class={line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : ""}>{line || " "}</div>)}</pre>
                : <p class="note">No structure changes.</p>}
              {s.structure_regions.length > 0 && <p class="note">{plural(s.structure_regions.length, "changed node")} outlined in Difference.</p>}
            </div> : <Empty>No structure captured for this screen.</Empty>
          ) : !s.base_url && !s.head_url ? <Empty>No PNG captured. Open Structure to review this screen.</Empty>
            : mode === "Side by side" || !both ? <div class="vd-side">
              <figure><figcaption>base</figcaption>{image("base")}</figure>
              <figure><figcaption>head</figcaption>{image("head")}</figure>
            </div> : <>
              <div class="vd-stack" style={{aspectRatio: `${width} / ${height}`}}>
                <img src={mode === "Difference" ? s.head_url : s.base_url} class={mode === "Difference" ? "dim-screen" : ""} alt={`${mode === "Difference" ? "Head" : "Base"} screen of ${s.name}`} width={width} height={height} />
                {mode !== "Difference" ? <img class="vd-top" style={mode === "Swipe" ? {clipPath: `inset(0 0 0 ${range}%)`} : {opacity: range / 100}} src={s.head_url} alt={`Head screen of ${s.name}`} width={width} height={height} />
                  : <svg class="vd-top diff-regions" viewBox={`0 0 ${width} ${height}`} role="img" aria-label={`${plural(s.regions.length, "changed pixel region")}, ${plural(s.structure_regions.length, "changed node")}`}>
                    {s.regions.map((r) => <rect x={r.x} y={r.y} width={r.width} height={r.height} />)}
                    {s.structure_regions.map((r) => <rect class="structure-region" x={r.x} y={r.y} width={r.width} height={r.height}><title>{r.change}</title></rect>)}
                  </svg>}
              </div>
              {mode !== "Difference" ? <label class="vd-range">base<input type="range" aria-label={mode === "Swipe" ? "Swipe position" : "Head opacity"} min="0" max="100" value={range} onInput={(e) => setRange(Number(e.currentTarget.value))} />head</label>
                : <p class="note">{plural(s.regions.length, "changed pixel region")}. {plural(s.structure_regions.length, "changed node")}. The stored head is dimmed behind the outlines.</p>}
            </>}
        </div>
      </div>
    </Card>
  );
}
