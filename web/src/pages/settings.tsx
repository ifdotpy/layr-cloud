import { machineLabel, personLabel } from "../labels";
import type { DataSource, Project } from "../data/source";
import {
  CopyRing,
  Empty,
  LoadError,
  Mono,
  Section,
  Table,
  bytes,
  useData,
  when,
} from "../components/ui";
import { projectUrl } from "../router";

const PAGES: [string, string][] = [
  ["Access", "access"],
  ["Protection", "protection"],
  ["Private files", "private-files"],
  ["Files", "files"],
  ["Copies", "copies"],
];

function Access({ project, source }: { project: Project; source: DataSource }) {
  const { data: identities } = useData(async () => {
    const [operations, machines, people] = await Promise.all([
      source.opLog(project.id).catch(() => []),
      source.machines().catch(() => []),
      source.people().catch(() => []),
    ]);
    return { operations, machines, people };
  }, [source, project.id]);
  const principalName = (principal: string) => {
    const machineId = principal.replace(/^machine:/, "");
    const name =
      principal.startsWith("machine:") ||
      identities?.machines.some((machine) => machine.id === machineId)
        ? machineLabel(machineId, identities?.machines ?? [])
        : personLabel(principal, identities?.operations, identities?.people);
    return (
      <span
        title={principal}
        class={name.startsWith("uid ") ? "muted" : undefined}
      >
        {name}
      </span>
    );
  };
  const access = Object.entries(project.policy?.access ?? {}).filter(
    ([, role]) => role !== "none",
  );
  const grants = Object.entries(project.policy?.grants ?? {}).filter(
    ([, [role]]) => role !== "none",
  );
  const reads = (principal: string) => {
    const overlays = project.overlays?.filter((overlay) =>
      overlay.readers.includes(principal),
    );
    return overlays?.length
      ? overlays.map((overlay) => (
          <div key={overlay.id}>
            <Mono>{overlay.name}</Mono>
          </div>
        ))
      : "Not recorded";
  };
  return (
    <>
      <Section
        title="Access"
        description="Project roles and time limits from the access policy."
      >
        {!access.length && !grants.length ? (
          <Empty>No project roles are available from the access policy.</Empty>
        ) : (
          <Table
            label="Project access"
            headers={[
              "Person or machine",
              "Role",
              "Reads overlays",
              "Source",
              "Until",
            ]}
            rows={[
              ...access.map(([principal, role]) => [
                principalName(principal),
                role,
                reads(principal),
                "Access policy",
                "No time limit",
              ]),
              ...grants.map(([principal, [role, until]]) => [
                principalName(principal),
                role,
                reads(principal),
                "Temporary grant",
                when(until),
              ]),
            ]}
          />
        )}
      </Section>
      <Section
        title="Registry tokens"
        description="Tokens let Docker, Helm and Kubernetes pull images. They cannot write."
      >
        <Empty>No registry tokens are available from the access service.</Empty>
      </Section>
    </>
  );
}

function Protection({ project }: { project: Project }) {
  const rules = Object.entries(project.policy?.protect ?? {});
  return (
    <Section title="Protection" description="Rules for lines and refs.">
      {!rules.length ? (
        <Empty>
          No protection rules are available from the project policy.
        </Empty>
      ) : (
        <Table
          label="Protection rules"
          headers={[
            "Line or ref",
            "Required checks",
            "Approvals",
            "Approvers",
            "Paths",
            "Direct saves",
            "Who may move",
            "Expected state",
            "Rewrite role",
          ]}
          rows={rules.map(([target, rule]) => [
            <Mono>{target}</Mono>,
            rule.check === undefined
              ? "Not recorded"
              : rule.check.length
                ? rule.check.join(", ")
                : "None",
            <Mono>{rule.approvals ?? "Not recorded"}</Mono>,
            rule.approvers === undefined
              ? "Not recorded"
              : rule.approvers.join(", ") || "None",
            rule.paths === undefined
              ? "Not recorded"
              : Object.entries(rule.paths).length
                ? Object.entries(rule.paths).map(([path, role]) => (
                    <div key={path}>
                      <Mono>{path}</Mono> → {role} needed to change it
                    </div>
                  ))
                : "None",
            rule.direct === undefined
              ? "Not recorded"
              : rule.direct
                ? "allowed"
                : "not allowed",
            rule.update ?? "Not recorded",
            rule.expect === undefined
              ? "Not recorded"
              : rule.expect
                ? "required"
                : "not required",
            rule.rewrite ?? "Not recorded",
          ])}
        />
      )}
    </Section>
  );
}

function PrivateFiles({ project }: { project: Project }) {
  const raw = project.config?.["secrets.paths"];
  const paths =
    typeof raw === "string"
      ? raw
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean)
      : Array.isArray(raw)
        ? raw.filter((x): x is string => typeof x === "string")
        : [];
  return (
    <>
      <Section title="Private files">
        <p class="lede">
          Open <a href={projectUrl(project.id, "overlays")}>Overlays</a> for
          files, history, readers and key managers.
        </p>
      </Section>
      <Section title="Secret detector">
        {paths.length ? (
          <Table
            label="Secret detector paths"
            headers={["Pattern", "Action"]}
            rows={paths.map((path) => [
              <Mono>{path}</Mono>,
              "refuse in shared files",
            ])}
          />
        ) : (
          <Empty>
            No detector paths are available from the project configuration.
          </Empty>
        )}
      </Section>
      <Section
        title="Declassified values"
        description="Only a fingerprint is shown, never the value."
      >
        <Empty>
          No declassified values are available from the signed records.
        </Empty>
      </Section>
      <Section title="Recent refusals">
        <Empty>No refusals are available from the operation records.</Empty>
      </Section>
    </>
  );
}

function Files({ project }: { project: Project }) {
  return (
    <>
      <Section
        title="Merge rules"
        description="Derived files are rebuilt after a merge."
      >
        <Empty>
          No merge rules are available from the project configuration.
        </Empty>
      </Section>
      <Section
        title="Caches"
        description="Regenerable folders do not enter a state."
      >
        {!project.disk?.length ? (
          <p class="note">Disk use is not recorded for this store.</p>
        ) : (
          <Table
            label="Cache use by line"
            headers={["Line", "Cache size"]}
            rows={project.disk.map((disk) => [
              <Mono>{disk.line}</Mono>,
              <Mono>{bytes(disk.cache_bytes)}</Mono>,
            ])}
          />
        )}
      </Section>
    </>
  );
}

function Copies({ source, project }: { source: DataSource; project: Project }) {
  const copies = useData(() => source.copies(project.id), [source, project.id]);
  const lines = useData(() => source.lines(project.id), [source, project.id]);
  const machines = useData(() => source.machines(), [source]);
  const machineIds = [
    ...new Set([
      ...(lines.data?.map((line) => line.machine) ?? []),
      ...(copies.data?.flatMap((copy) => copy.machines ?? []) ?? []),
    ]),
  ].sort();
  const completeCopies = copies.data?.filter(
    (copy) => copy.present !== undefined && copy.expected !== undefined,
  );
  return (
    <>
      <Section
        title="Copies"
        description="Where this project is kept. Machines report copies when they push or sync."
      >
        <div class="callout" style={{ marginBottom: 16 }}>
          <span class="muted">Each machine has its own remote list. </span>
          <code>layr remote</code>
          <span class="muted"> manages that list.</span>
        </div>
        {!copies.data ? (
          <LoadError error={copies.error} />
        ) : !copies.data.length ? (
          <Empty>No copies are recorded.</Empty>
        ) : (
          <>
            {!!completeCopies?.length && (
              <div class="rings">
                {completeCopies.map((copy) => (
                  <CopyRing key={copy.id} copy={copy} />
                ))}
              </div>
            )}
            <Table
              label="Project copies"
              headers={["Copy", "Kind", "Where", "Last update", "By"]}
              rows={copies.data.map((copy) => [
                copy.name,
                copy.kind === "git" ? "Git mirror" : copy.kind,
                <Mono>{copy.address ?? "Not recorded"}</Mono>,
                when(copy.time),
                "Not recorded",
              ])}
            />
            {copies.data.some(
              (copy) =>
                copy.present === undefined || copy.expected === undefined,
            ) && (
              <p class="note" style={{ marginTop: 8 }}>
                Copy completeness is not available for all copies from the sync
                records.
              </p>
            )}
          </>
        )}
      </Section>
      <Section
        title="Machines with this project"
        description="Machines keep the lines they use. Overlay access is separate."
      >
        {!lines.data && !copies.data ? (
          <LoadError error={lines.error ?? copies.error} />
        ) : !machineIds.length && (lines.error || copies.error) ? (
          <LoadError error={lines.error ?? copies.error} />
        ) : !machineIds.length ? (
          <Empty>No machines are recorded for this project.</Empty>
        ) : (
          <Table
            label="Machines with this project"
            headers={["Machine", "Holds", "Last sync"]}
            rows={machineIds.map((id) => {
              const machine = machines.data?.find((item) => item.id === id);
              const heldLines =
                lines.data
                  ?.filter((line) => line.machine === id)
                  .map((line) => line.name) ?? [];
              const heldCopies =
                copies.data?.filter((copy) => copy.machines?.includes(id)) ??
                [];
              const times = heldCopies.flatMap((copy) =>
                copy.time === undefined ? [] : [copy.time],
              );
              return [
                <Mono>{machine?.name ?? id}</Mono>,
                <Mono>
                  {[...heldLines, ...heldCopies.map((copy) => copy.name)].join(
                    ", ",
                  ) || "Not recorded"}
                </Mono>,
                when(times.length ? Math.max(...times) : undefined),
              ];
            })}
          />
        )}
        <p class="note" style={{ marginTop: 8 }}>
          <a href="#/infrastructure">All machines</a>
        </p>
      </Section>
    </>
  );
}

export function SettingsPage({
  source,
  project,
  page,
}: {
  source: DataSource;
  project: Project;
  page: string;
}) {
  return (
    <div class="settings">
      <nav class="snav" aria-label="Settings">
        <div class="grp">Project</div>
        {PAGES.map(([name, path]) => (
          <a
            key={path}
            href={projectUrl(project.id, `settings/${path}`)}
            aria-current={page === path ? "page" : undefined}
          >
            {name}
          </a>
        ))}
      </nav>
      <div>
        {page === "access" ? (
          <Access project={project} source={source} />
        ) : page === "protection" ? (
          <Protection project={project} />
        ) : page === "private-files" ? (
          <PrivateFiles project={project} />
        ) : page === "files" ? (
          <Files project={project} />
        ) : page === "copies" ? (
          <Copies source={source} project={project} />
        ) : (
          <Empty>Settings page not found.</Empty>
        )}
      </div>
    </div>
  );
}
