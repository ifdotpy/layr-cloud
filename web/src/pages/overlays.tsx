import type {
  DataSource,
  Machine,
  Operation,
  Overlay,
  Person,
  Project,
} from "../data/source";
import {
  Badge,
  Cmd,
  Empty,
  LoadError,
  Mono,
  PageHead,
  ReadOnlyAction,
  Table,
  Who,
  bytes,
  short,
  useData,
  when,
} from "../components/ui";
import {
  machineLabel,
  operationLabel,
  personLabel,
  plural,
  shortText,
} from "../labels";
import { projectUrl } from "../router";

type PageProps = { source: DataSource; project: Project };
type Identities = {
  machines: Machine[];
  people: Person[];
  operations: Operation[];
};
const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;
const names = (overlay: Overlay) =>
  overlay.paths ?? overlay.files?.map((file) => file.path);
const fileCount = (overlay: Overlay) => {
  const count = names(overlay)?.length;
  return count === undefined
    ? "File count not recorded"
    : plural(count, "file");
};
const changed = (overlay: Overlay) =>
  overlay.history?.length
    ? Math.max(...overlay.history.map((operation) => operation.time))
    : undefined;

function useIdentities(source: DataSource, project: Project) {
  return useData(async () => {
    const [machines, people, operations] = await Promise.all([
      source.machines(),
      source.people(),
      source.opLog(project.id),
    ]);
    return { machines, people, operations };
  }, [source, project.id]);
}

function Principal({
  name,
  identities,
  machine = false,
}: {
  name: string;
  identities: Identities;
  machine?: boolean;
}) {
  const id = name.replace(/^machine:/, "");
  if (
    machine ||
    name.startsWith("machine:") ||
    identities.machines.some((item) => item.id === id)
  ) {
    return (
      <span title={name}>
        {shortText(machineLabel(id, identities.machines))}
      </span>
    );
  }
  const label = personLabel(name, identities.operations, identities.people);
  return label.startsWith("uid ") ? (
    <span class="muted" title={name}>
      {label}
    </span>
  ) : (
    <span title={name}>
      <Who name={shortText(label)} />
    </span>
  );
}

function Access({ overlay }: { overlay: Overlay }) {
  return (
    <Badge kind={overlay.readable ? "ok" : ""}>
      {overlay.readable ? "✓ you read it" : "you cannot read it"}
    </Badge>
  );
}

function WriteAction({ label, command }: { label: string; command: string }) {
  return <ReadOnlyAction label={label} command={command} />;
}

function ManagerRisk({ overlay }: { overlay: Overlay }) {
  return overlay.managers.length === 1 ? (
    <p class="note" style={{ marginTop: 8 }}>
      <span class="warn">
        Only 1 key manager. If it is lost, no one can change readers.
      </span>
    </p>
  ) : null;
}

function Epoch({ overlay, id }: { overlay: Overlay; id: string }) {
  // Missing dates cannot establish a chronological epoch number.
  const recorded = overlay.epochs ?? [];
  const epochs = recorded.every((epoch) => epoch.time !== undefined)
    ? [...recorded].sort((a, b) => a.time! - b.time!)
    : [];
  const index = epochs.findIndex((epoch) => epoch.id === id);
  const epoch = epochs[index];
  return (
    <span title={id}>
      {index < 0 ? (
        <>
          epoch <Mono>{short(id)}</Mono>
        </>
      ) : (
        <>
          epoch {index + 1} · {when(epoch.time)}
        </>
      )}
    </span>
  );
}

export function OverlaysPage({ source, project }: PageProps) {
  const { data: identities, error } = useIdentities(source, project);
  if (!identities) return <LoadError error={error} />;
  const overlays = project.overlays;
  const showKey = overlays?.some(
    (overlay) => overlay.readable && overlay.epoch,
  );
  const showChanged = overlays?.some(
    (overlay) => overlay.readable && changed(overlay) !== undefined,
  );
  return (
    <>
      <PageHead
        title="Overlays"
        description="Private files in this project. Each overlay has its own readers and key. Only readers see its files."
        actions={
          <WriteAction
            label="New overlay"
            command="layr overlay create <name> --readers <readers>"
          />
        }
      />
      {overlays === undefined ? (
        <Empty>No overlay records are available.</Empty>
      ) : !overlays.length ? (
        <Empty>No overlays are recorded.</Empty>
      ) : (
        <>
          <Table
            label="Project overlays"
            headers={[
              "Overlay",
              "Files",
              "Readers",
              "Your access",
              ...(showKey ? ["Key"] : []),
              ...(showChanged ? ["Changed"] : []),
            ]}
            rows={overlays.map((overlay) => [
              <div class="row">
                <a
                  href={projectUrl(
                    project.id,
                    `overlay/${encodeURIComponent(overlay.id)}`,
                  )}
                >
                  {shortText(overlay.name)}
                </a>
                {overlay.hidden && <Badge>hidden</Badge>}
              </div>,
              overlay.readable ? fileCount(overlay) : "Files are hidden",
              <div class="row" style={{ gap: 6 }}>
                {overlay.readers.length
                  ? overlay.readers.map((reader) => (
                      <Principal
                        key={reader}
                        name={reader}
                        identities={identities}
                      />
                    ))
                  : "No readers recorded"}
              </div>,
              <div>
                <div class="row">
                  <Access overlay={overlay} />
                  {overlay.readable && (
                    <Badge>
                      {plural(overlay.managers.length, "key manager")}
                    </Badge>
                  )}
                </div>
                {overlay.readable && <ManagerRisk overlay={overlay} />}
              </div>,
              ...(showKey
                ? [
                    overlay.readable ? (
                      overlay.epoch ? (
                        <Epoch overlay={overlay} id={overlay.epoch} />
                      ) : (
                        "Not recorded"
                      )
                    ) : (
                      "Hidden"
                    ),
                  ]
                : []),
              ...(showChanged
                ? [overlay.readable ? when(changed(overlay)) : "Hidden"]
                : []),
            ])}
          />
          {(!showKey || !showChanged) && (
            <p class="note" style={{ marginTop: 8 }}>
              {[
                !showKey && "Key epochs are not recorded.",
                !showChanged && "Overlay change dates are not recorded.",
              ]
                .filter(Boolean)
                .join(" ")}
            </p>
          )}
        </>
      )}
      <p class="footnote">
        Private overlays show their names to everyone. Only readers see their
        files. Hidden overlays appear only to their readers.
      </p>
    </>
  );
}

export function OverlayPage({
  source,
  project,
  id,
}: PageProps & { id: string }) {
  const { data: identities, error } = useIdentities(source, project);
  const overlay = project.overlays?.find(
    (item) => item.id === id || item.name === id,
  );
  if (!overlay)
    return (
      <>
        <PageHead title="Overlay" />
        <Empty>
          {project.overlays === undefined
            ? "No overlay records are available."
            : "Overlay not found."}
        </Empty>
      </>
    );
  if (!identities) return <LoadError error={error} />;
  const paths = names(overlay);
  const target = quote(overlay.name);
  const showReaderKey = overlay.readers.some((principal) =>
    overlay.reader_details?.some(
      (item) => item.principal === principal && item.key,
    ),
  );
  const showReaderSince = overlay.readers.some((principal) =>
    overlay.reader_details?.some(
      (item) => item.principal === principal && item.since !== undefined,
    ),
  );
  return (
    <>
      <p class="small" style={{ marginBottom: 8 }}>
        <a href={projectUrl(project.id, "overlays")}>Overlays</a>{" "}
        <span class="muted">/</span>
      </p>
      <PageHead
        title={
          <span class="mono" style={{ fontSize: 24 }}>
            {shortText(overlay.name)}
          </span>
        }
        description={
          <>
            <Access overlay={overlay} />{" "}
            {overlay.hidden && <Badge>hidden</Badge>}{" "}
            {overlay.readable && (
              <>
                {fileCount(overlay)} ·{" "}
                {plural(overlay.readers.length, "reader")} ·{" "}
                <Badge>{plural(overlay.managers.length, "key manager")}</Badge>{" "}
                ·{" "}
                {overlay.epoch ? (
                  <Epoch overlay={overlay} id={overlay.epoch} />
                ) : (
                  "Key epoch not recorded"
                )}{" "}
                · <span class="stamp accent">encrypted</span>
              </>
            )}
          </>
        }
        actions={
          overlay.readable && (
            <WriteAction
              label="Move files in"
              command={`layr overlay move ${target} <path>`}
            />
          )
        }
      />
      <div class={overlay.readable ? "cols" : ""}>
        <div class="stack">
          <section class="sect">
            <h2 style={{ marginBottom: 12 }}>Files</h2>
            {!overlay.readable ? (
              <Empty>Files are hidden. You cannot read this overlay.</Empty>
            ) : !paths ? (
              <Empty>No file list is recorded.</Empty>
            ) : !paths.length ? (
              <Empty>No files are recorded.</Empty>
            ) : (
              <>
                <Table
                  label="Overlay files"
                  headers={["Path", "Last state", "Size"]}
                  rows={paths.map((path) => {
                    const file = overlay.files?.find(
                      (item) => item.path === path,
                    );
                    return [
                      <Mono>{shortText(path)}</Mono>,
                      file?.state ? (
                        <a
                          href={projectUrl(
                            project.id,
                            `state/${encodeURIComponent(file.state)}`,
                          )}
                          title={file.state}
                        >
                          <Mono>{short(file.state)}</Mono>
                        </a>
                      ) : (
                        "Not recorded"
                      ),
                      bytes(file?.size),
                    ];
                  })}
                />
                <p class="note" style={{ marginTop: 8 }}>
                  File change dates are not recorded.
                </p>
              </>
            )}
            {overlay.readable && (
              <p class="note" style={{ marginTop: 8 }}>
                Routed here:{" "}
                {overlay.patterns.length
                  ? overlay.patterns.map((pattern) => (
                      <span style={{ marginRight: 8 }} key={pattern}>
                        <Mono>{shortText(pattern)}</Mono>
                      </span>
                    ))
                  : "No patterns recorded."}{" "}
                {overlay.patterns.length > 0 &&
                  "New files that match go to this overlay."}
              </p>
            )}
          </section>
          <section class="sect">
            <div class="row" style={{ marginBottom: 12 }}>
              <h2>Readers</h2>
              <span class="spacer" />
              {overlay.readable && (
                <WriteAction
                  label="Add reader"
                  command={`layr overlay readers ${target} <current-readers> <reader>`}
                />
              )}
            </div>
            {!overlay.readers.length ? (
              <Empty>No readers are recorded.</Empty>
            ) : (
              <>
                <Table
                  label="Overlay readers"
                  headers={[
                    "Reader",
                    "Kind",
                    ...(overlay.readable
                      ? [
                          ...(showReaderKey ? ["Key"] : []),
                          ...(showReaderSince ? ["Since"] : []),
                        ]
                      : []),
                  ]}
                  rows={overlay.readers.map((principal) => {
                    const details = overlay.reader_details?.find(
                      (item) => item.principal === principal,
                    );
                    const machine =
                      principal.startsWith("machine:") ||
                      identities.machines.some((item) => item.id === principal);
                    return [
                      <Principal name={principal} identities={identities} />,
                      machine ? "machine" : "person",
                      ...(overlay.readable
                        ? [
                            ...(showReaderKey
                              ? [
                                  details?.key ? (
                                    <span title={details.key}>
                                      <Mono>{shortText(details.key)}</Mono>
                                    </span>
                                  ) : (
                                    "Not recorded"
                                  ),
                                ]
                              : []),
                            ...(showReaderSince ? [when(details?.since)] : []),
                          ]
                        : []),
                    ];
                  })}
                />
                {overlay.readable && (!showReaderKey || !showReaderSince) && (
                  <p class="note" style={{ marginTop: 8 }}>
                    {!showReaderKey && !showReaderSince
                      ? "Reader keys and key dates are not recorded."
                      : !showReaderKey
                        ? "Reader keys are not recorded."
                        : "Reader key dates are not recorded."}
                  </p>
                )}
              </>
            )}
          </section>
          {overlay.readable && (
            <>
              <section class="sect">
                <h2 style={{ marginBottom: 12 }}>Key epochs</h2>
                {!overlay.epochs?.length ? (
                  <Empty>No key epoch history is recorded.</Empty>
                ) : (
                  <ul class="tl">
                    {overlay.epochs.map((epoch) => (
                      <li
                        key={epoch.id}
                        class={epoch.id === overlay.epoch ? "major" : ""}
                      >
                        <b>
                          <Epoch overlay={overlay} id={epoch.id} />
                        </b>
                        <p class="note">
                          {shortText(epoch.reason ?? "No reason is recorded.")}
                        </p>
                        {epoch.machine && (
                          <p class="note">
                            Machine:{" "}
                            <Principal
                              name={epoch.machine}
                              machine
                              identities={identities}
                            />
                          </p>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
              </section>
              <section class="sect">
                <h2 style={{ marginBottom: 12 }}>History</h2>
                {!overlay.history?.length ? (
                  <Empty>No signed overlay operations are recorded.</Empty>
                ) : (
                  <Table
                    label="Signed overlay operations"
                    headers={["When", "Who", "What", "Signature"]}
                    rows={overlay.history.map((operation) => [
                      when(operation.time),
                      <Principal
                        name={`user:${operation.actor.uid}`}
                        identities={identities}
                      />,
                      <span title={`${operation.op} · ${short(operation.id)}`}>
                        {operationLabel(operation)}
                      </span>,
                      <Badge kind="ok">✓ signed</Badge>,
                    ])}
                  />
                )}
                <Cmd text={`layr overlay log ${target}`} />
              </section>
            </>
          )}
        </div>
        {overlay.readable && (
          <aside class="side">
            <section>
              <h2 class="side-title">Key managers</h2>
              <WriteAction
                label="Add key manager"
                command={`layr overlay managers ${target} <current-managers> <machine>`}
              />
              {overlay.managers.length ? (
                <ul>
                  {overlay.managers.map((manager) => (
                    <li key={manager}>
                      <Principal
                        name={manager}
                        machine={!/^(person:|user:|uid:)/.test(manager)}
                        identities={identities}
                      />
                    </li>
                  ))}
                </ul>
              ) : (
                <p class="note">No key managers are recorded.</p>
              )}
              <ManagerRisk overlay={overlay} />
              <p class="note" style={{ marginTop: 8 }}>
                Only managers change readers and the key. Other machines can
                push data encrypted to it.
              </p>
            </section>
            <section>
              <h2 class="side-title">Peers that may hold it</h2>
              {overlay.peers === undefined ? (
                <p class="note">No peer records are available.</p>
              ) : overlay.peers.length ? (
                <ul>
                  {overlay.peers.map((peer) => (
                    <li key={peer}>
                      <Principal name={peer} machine identities={identities} />
                    </li>
                  ))}
                </ul>
              ) : (
                <p class="note">No peers are recorded.</p>
              )}
              <p class="note" style={{ marginTop: 8 }}>
                Other machines get the shared files only.
              </p>
            </section>
            <section>
              <h2 class="side-title">In stores</h2>
              {!overlay.slots?.length ? (
                <p class="note">No store slots are recorded.</p>
              ) : (
                <ul>
                  {overlay.slots.map((slot) => (
                    <li key={`${slot.store}/${slot.epoch}`}>
                      <span title={slot.store}>
                        <Mono>{shortText(slot.store)}</Mono>
                      </span>
                      <span class="r">
                        <Epoch overlay={overlay} id={slot.epoch} /> ·{" "}
                        {plural(slot.count, "slot")}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </aside>
        )}
      </div>
    </>
  );
}
