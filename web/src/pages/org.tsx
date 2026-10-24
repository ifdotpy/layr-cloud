import { useState } from "preact/hooks";
import type {
  Copy,
  DataSource,
  Line,
  Operation,
  Project,
  Ref,
} from "../data/source";
import {
  Badge,
  Card,
  Cmd,
  Empty,
  LoadError,
  Mono,
  PageHead,
  ReadOnlyAction,
  Section,
  Table,
  Who,
  bytes,
  useData,
  when,
} from "../components/ui";
import { projectUrl } from "../router";
import { plural } from "../labels";

interface ProjectSummary {
  project: Project;
  lines?: Line[];
  refs?: Ref[];
  copies?: Copy[];
  operations?: Operation[];
}

async function projectSummary(
  source: DataSource,
  project: Project,
): Promise<ProjectSummary> {
  const [lines, refs, copies, operations] = await Promise.allSettled([
    source.lines(project.id),
    source.refs(project.id),
    source.copies(project.id),
    source.opLog(project.id),
  ]);
  return {
    project,
    lines: lines.status === "fulfilled" ? lines.value : undefined,
    refs: refs.status === "fulfilled" ? refs.value : undefined,
    copies: copies.status === "fulfilled" ? copies.value : undefined,
    operations:
      operations.status === "fulfilled" ? operations.value : undefined,
  };
}

function lastCopy(copies?: Copy[]): number | undefined {
  const times = copies
    ?.filter((copy) => copy.kind === "store")
    .flatMap((copy) => (copy.time === undefined ? [] : [copy.time]));
  return times?.length ? Math.max(...times) : undefined;
}

function lastActivity(summary: ProjectSummary): number {
  return summary.operations?.length
    ? Math.max(...summary.operations.map((operation) => operation.time))
    : -Infinity;
}

function Projects({ source }: { source: DataSource }) {
  const result = useData(
    async () =>
      Promise.all(
        (await source.projects()).map((project) =>
          projectSummary(source, project),
        ),
      ),
    [source],
  );
  const [filter, setFilter] = useState("");
  const [sort, setSort] = useState("activity");
  const organizations = [
    ...new Set(
      result.data?.flatMap(({ project }) =>
        project.organization ? [project.organization] : [],
      ),
    ),
  ];
  const projects = result.data
    ?.filter(({ project }) =>
      `${project.name} ${project.description ?? ""}`
        .toLowerCase()
        .includes(filter.toLowerCase()),
    )
    .sort((a, b) =>
      sort === "name"
        ? a.project.name.localeCompare(b.project.name)
        : lastActivity(b) - lastActivity(a) ||
          a.project.name.localeCompare(b.project.name),
    );
  return (
    <>
      <PageHead
        title={organizations.length === 1 ? organizations[0] : "Projects"}
        actions={<ReadOnlyAction label="New project" primary />}
      />
      <div class="filters">
        <input
          class="input grow"
          type="search"
          aria-label="Find a project"
          placeholder="Find a project"
          value={filter}
          onInput={(event) => setFilter(event.currentTarget.value)}
        />
        <select
          class="input"
          aria-label="Sort projects"
          value={sort}
          onChange={(event) => setSort(event.currentTarget.value)}
        >
          <option value="activity">Last activity</option>
          <option value="name">Name</option>
        </select>
      </div>
      {!result.data ? (
        <LoadError error={result.error} />
      ) : !projects?.length ? (
        <Empty>
          {filter
            ? "No projects match this filter."
            : "No projects are available from the project records."}
        </Empty>
      ) : (
        <div class="list">
          {projects.map(({ project, lines, refs, copies, operations }) => {
            const production = refs?.find((ref) => ref.name === "env/prod");
            return (
              <div class="item" key={project.id}>
                <div class="main">
                  <a class="title" href={projectUrl(project.id)}>
                    {project.name}
                  </a>
                  <p class="note">
                    {project.description ?? "No description is recorded."}
                  </p>
                  <div class="meta">
                    <span>
                      line <Mono>{project.default_line ?? "Not recorded"}</Mono>
                    </span>
                    <span>
                      {lines
                        ? plural(lines.length, "line")
                        : "Line count not available"}
                    </span>
                    <span>
                      <Mono>env/prod</Mono>{" "}
                      {production ? (
                        <Mono>{production.target}</Mono>
                      ) : (
                        "not recorded"
                      )}
                    </span>
                    <span>
                      Last activity:{" "}
                      {when(
                        operations?.length
                          ? Math.max(
                              ...operations.map((operation) => operation.time),
                            )
                          : undefined,
                      )}
                    </span>
                  </div>
                </div>
                <div class="side">
                  <span class="muted small">
                    Encrypted copy: {when(lastCopy(copies))}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      )}
      <div class="row" style={{ marginTop: 16 }}>
        <ReadOnlyAction label="Import from Layr Desk" />
      </div>
    </>
  );
}

function People({ source }: { source: DataSource }) {
  const result = useData(() => source.people(), [source]);
  return (
    <>
      <PageHead
        title="People"
        actions={
          <>
            <ReadOnlyAction label="Grant access" />
            <ReadOnlyAction label="Invite person" primary />
          </>
        }
      />
      {!result.data ? (
        <LoadError error={result.error} />
      ) : !result.data.length ? (
        <Empty>No people are available from the organization records.</Empty>
      ) : (
        <Table
          label="Organization people"
          headers={[
            "Person",
            "Login",
            "Role",
            "Person key",
            "Projects",
            "Last seen",
          ]}
          rows={result.data.map((person) => [
            <Who name={person.name ?? person.login} />,
            <Mono>{person.login}</Mono>,
            person.role ?? "Not recorded",
            person.recipient ? (
              <Mono>{person.recipient}</Mono>
            ) : person.invited ? (
              <Badge kind="warn">invited · waiting for key</Badge>
            ) : (
              "Not recorded"
            ),
            person.projects ? (
              <Mono>{person.projects.length}</Mono>
            ) : (
              "Not recorded"
            ),
            when(person.time),
          ])}
        />
      )}
    </>
  );
}

function Infrastructure({ source }: { source: DataSource }) {
  const stores = useData(() => source.stores(), [source]);
  const machines = useData(() => source.machines(), [source]);
  const executors = machines.data?.filter(
    (machine) => machine.role === "executor",
  );
  return (
    <>
      <PageHead
        title="Infrastructure"
        description="Storage and Layr Cloud see ciphertext only."
      />
      <Section title="Stores">
        {!stores.data ? (
          <LoadError error={stores.error} />
        ) : !stores.data.length ? (
          <Empty>No stores are available from the store records.</Empty>
        ) : (
          <Table
            label="Project stores"
            headers={[
              "Project",
              "Store URL",
              "Region",
              "Encryption",
              "Size",
              "Objects",
              "Last write",
              "GC",
            ]}
            rows={stores.data.map((store) => [
              <a href={projectUrl(store.project)}>
                <Mono>{store.project}</Mono>
              </a>,
              <Mono>{store.address ?? "Not recorded"}</Mono>,
              store.region ?? "Not recorded",
              "age, end to end",
              <Mono>{bytes(store.bytes)}</Mono>,
              <Mono>{store.objects ?? "Not recorded"}</Mono>,
              when(store.time),
              "Not recorded",
            ])}
          />
        )}
      </Section>
      <Section
        title="Machines"
        description="Each machine signs its records with its own key."
      >
        {!machines.data ? (
          <LoadError error={machines.error} />
        ) : !machines.data.length ? (
          <Empty>No machines are available from the machine records.</Empty>
        ) : (
          <Table
            label="Organization machines"
            headers={[
              "Machine",
              "OS and backend",
              "Role",
              "Signing key",
              "Trust",
              "Last seen",
            ]}
            rows={machines.data.map((machine) => [
              <Mono>{machine.name ?? machine.id}</Mono>,
              [machine.os, machine.backend].filter(Boolean).join(" · ") ||
                "Not recorded",
              machine.role ?? "Not recorded",
              <Mono>{machine.fingerprint ?? "Not recorded"}</Mono>,
              machine.trusted === undefined ? (
                "Not recorded"
              ) : (
                <Badge kind={machine.trusted ? "ok" : "warn"}>
                  {machine.trusted ? "✓ trusted" : "○ untrusted"}
                </Badge>
              ),
              <div>
                {when(machine.time)}
                {machine.online !== undefined && (
                  <div>
                    <Badge kind={machine.online ? "ok" : ""}>
                      {machine.online ? "● online" : "○ offline"}
                    </Badge>
                  </div>
                )}
              </div>,
            ])}
          />
        )}
      </Section>
      <Section
        title="Gateway"
        description="Machines and people reach stores without AWS credentials."
      >
        <Empty>
          Gateway details are not available from the deployment records.
        </Empty>
      </Section>
      <Section title="Executors">
        {!machines.data ? (
          <LoadError error={machines.error} />
        ) : !executors?.length ? (
          <Empty>No executors are available from the machine records.</Empty>
        ) : (
          <Table
            label="Executor machines"
            headers={["Machine", "OS and backend", "Connection"]}
            rows={executors.map((machine) => [
              <Mono>{machine.name ?? machine.id}</Mono>,
              [machine.os, machine.backend].filter(Boolean).join(" · ") ||
                "Not recorded",
              machine.online === undefined
                ? "Not recorded"
                : machine.online
                  ? "● online"
                  : "○ offline",
            ])}
          />
        )}
        <p class="note" style={{ marginTop: 8 }}>
          Live jobs are not available from the executor reports.
        </p>
      </Section>
      <Section
        title="Deployment"
        description="Layr Cloud runs in your AWS account."
      >
        <Empty>
          Deployment details are not available from the CloudFormation records.
        </Empty>
        <Cmd text="layr-cloud deploy --help" />
      </Section>
    </>
  );
}

function OrganizationSettings({ source }: { source: DataSource }) {
  const result = useData(() => source.projects(), [source]);
  const organizations = [
    ...new Set(
      result.data?.flatMap((project) =>
        project.organization ? [project.organization] : [],
      ),
    ),
  ];
  return (
    <>
      <PageHead title="Organization settings" readOnly />
      <Card>
        {!result.data ? (
          <LoadError error={result.error} />
        ) : (
          <dl class="kv">
            <dt>Name</dt>
            <dd>
              {organizations.length === 1 ? (
                <Mono>{organizations[0]}</Mono>
              ) : (
                "Not recorded"
              )}
            </dd>
          </dl>
        )}
        <p class="note" style={{ marginTop: 16 }}>
          AWS account details are not available from the organization records.
        </p>
        <p class="note" style={{ marginTop: 8 }}>
          The default role is not available from the organization policy.
        </p>
        <div class="row" style={{ marginTop: 16 }}>
          <ReadOnlyAction label="Save settings" primary />
        </div>
      </Card>
      <div class="danger-zone" style={{ marginTop: 32 }}>
        <Card title="Danger zone">
          <div class="row">
            <b>Delete organization</b>
            <span class="spacer" />
            <ReadOnlyAction label="Delete organization" />
          </div>
        </Card>
      </div>
    </>
  );
}

function YouPage({ source }: { source: DataSource }) {
  const result = useData(() => source.you(), [source]);
  return (
    <>
      <PageHead
        title="You"
        readOnly
        description="Your person key, invites, disk and AWS access."
      />
      {!result.data ? (
        <LoadError error={result.error} />
      ) : (
        <>
          <Section
            title="Your person key"
            description="One key opens every project you may read. Layr Cloud never holds it."
          >
            {result.data.recipient ? (
              <Card>
                <dl class="kv">
                  <dt>Login</dt>
                  <dd>{result.data.login ?? "Not recorded"}</dd>
                  <dt>Recipient</dt>
                  <dd>
                    <Mono>{result.data.recipient}</Mono>
                  </dd>
                </dl>
                <div class="row" style={{ marginTop: 16 }}>
                  <ReadOnlyAction
                    label="Show recovery words"
                    command="layr person keys"
                  />
                </div>
              </Card>
            ) : (
              <Empty>No person key is available from your session.</Empty>
            )}
          </Section>
          <Section title="Your invites">
            {!result.data.invites.length ? (
              <Empty>No invites are available from your session.</Empty>
            ) : (
              <Table
                label="Your invites"
                headers={["Project", "Role", "Action"]}
                rows={result.data.invites.map((invite) => [
                  <Mono>{invite.project}</Mono>,
                  invite.role ?? "Not recorded",
                  <ReadOnlyAction label="Accept" />,
                ])}
              />
            )}
            {result.data.invite_errors?.map((invite) => (
              <p class="note">
                Cannot open invite <Mono>{invite.id}</Mono>: {invite.reason}
              </p>
            ))}
          </Section>
          <Section
            title="Your disk"
            description="Store lock state on your machine."
          >
            {!result.data.disk.length ? (
              <Empty>No disk records are available from your session.</Empty>
            ) : (
              <Table
                label="Your disk"
                headers={["Store", "Address", "Folder", "Lock state"]}
                rows={result.data.disk.map((store) => [
                  <Mono>{store.store}</Mono>,
                  store.address ? (
                    <Mono>{store.address}</Mono>
                  ) : (
                    <Badge kind="warn">no address</Badge>
                  ),
                  <Mono>{store.path ?? "Not recorded"}</Mono>,
                  <Badge>{store.locked ? "locked" : "unlocked"}</Badge>,
                ])}
              />
            )}
            <Cmd text="layr mount --all" />
          </Section>
          <Section title="AWS access">
            {!result.data.aws ? (
              <Empty>
                No AWS access record is available from your session.
              </Empty>
            ) : (
              <Card>
                <dl class="kv">
                  <dt>Profile</dt>
                  <dd>
                    <Mono>{result.data.aws.profile}</Mono>
                  </dd>
                  <dt>Region</dt>
                  <dd>
                    <Mono>{result.data.aws.region ?? "Not recorded"}</Mono>
                  </dd>
                  <dt>Status</dt>
                  <dd>{result.data.aws.status}</dd>
                </dl>
              </Card>
            )}
          </Section>
        </>
      )}
    </>
  );
}

export function OrgPage({
  source,
  page,
}: {
  source: DataSource;
  page: string;
}) {
  if (page === "people") return <People source={source} />;
  if (page === "infrastructure") return <Infrastructure source={source} />;
  if (page === "org-settings") return <OrganizationSettings source={source} />;
  if (page === "you") return <YouPage source={source} />;
  return <Projects source={source} />;
}
