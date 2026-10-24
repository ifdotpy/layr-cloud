import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import type { DataSource, Overlay, Project } from "../data/source";
import {
  Badge,
  Cmd,
  Dialog,
  Empty,
  Mono,
  PageHead,
  Table,
  Who,
  bytes,
  when,
  operationName,
} from "../components/ui";
import { projectUrl } from "../router";

type PageProps = { source: DataSource; project: Project };
const readOnlyReason = "Read only here. Use the CLI command.";
const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;
const words = (value: string) => value.split(/[\s,]+/).filter(Boolean);
const names = (overlay: Overlay) =>
  overlay.paths ?? overlay.files?.map((file) => file.path);
const count = (overlay: Overlay) => names(overlay)?.length;
const fileCount = (overlay: Overlay) =>
  count(overlay) === undefined
    ? "File count not recorded"
    : `${count(overlay)} ${count(overlay) === 1 ? "file" : "files"}`;
const changed = (overlay: Overlay) =>
  overlay.history?.length
    ? Math.max(...overlay.history.map((operation) => operation.time))
    : undefined;
const kind = (principal: string) =>
  principal.startsWith("machine:") ? "machine" : "person";

function Principal({ name }: { name: string }) {
  return kind(name) === "machine" ? (
    <span class="ref">{name}</span>
  ) : (
    <Who name={name.replace(/^person:/, "")} />
  );
}

function Access({ overlay }: { overlay: Overlay }) {
  return (
    <Badge kind={overlay.readable ? "ok" : ""}>
      {overlay.readable ? "✓ you read it" : "you cannot read it"}
    </Badge>
  );
}

function PhaseTwo({
  label,
  danger = false,
}: {
  label: string;
  danger?: boolean;
}) {
  return (
    <div class="stack-s">
      <p class="note">{readOnlyReason}</p>
      <button
        class={`btn ${danger ? "danger" : "primary"}`}
        type="button"
        disabled
        title={readOnlyReason}
      >
        {label}
      </button>
    </div>
  );
}

function CommandDialog({
  title,
  open,
  onClose,
  children,
  label = title,
  danger = false,
}: {
  title: string;
  open: boolean;
  onClose: () => void;
  children: ComponentChildren;
  label?: string;
  danger?: boolean;
}) {
  return (
    <Dialog title={title} open={open} onClose={onClose}>
      {children}
      <PhaseTwo label={label} danger={danger} />
    </Dialog>
  );
}

function NewOverlay({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [name, setName] = useState("");
  const [readers, setReaders] = useState("");
  const [patterns, setPatterns] = useState("");
  const [hidden, setHidden] = useState(false);
  const target = quote(name.trim() || "<name>");
  const create = `layr overlay create ${target} --readers ${quote(words(readers).join(",") || "<readers>")}`;
  return (
    <CommandDialog
      title="New overlay"
      label="Create overlay"
      open={open}
      onClose={onClose}
    >
      <p class="muted">
        Private files have their own history, readers and key. They travel
        encrypted with each push.
      </p>
      <label class="field">
        <span>Name</span>
        <input
          class="input mono"
          value={name}
          placeholder="Overlay name"
          onInput={(event) => setName(event.currentTarget.value)}
        />
      </label>
      <label class="field">
        <span>Readers</span>
        <input
          class="input mono"
          value={readers}
          placeholder="Person and machine names, separated by commas"
          onInput={(event) => setReaders(event.currentTarget.value)}
        />
      </label>
      <label class="field">
        <span>Files that belong to it</span>
        <input
          class="input mono"
          value={patterns}
          placeholder=".env*, config/*.key"
          onInput={(event) => setPatterns(event.currentTarget.value)}
        />
      </label>
      <label class="row">
        <input
          type="checkbox"
          role="switch"
          checked={hidden}
          onChange={(event) => setHidden(event.currentTarget.checked)}
        />
        <span>Hidden: only its readers know it exists</span>
      </label>
      <p class="note">A visible overlay cannot become hidden later.</p>
      <p class="note">
        This machine becomes the first key manager. Add a second manager so a
        lost laptop cannot lock the key.
      </p>
      <Cmd text={create + (hidden ? " --hidden" : "")} />
      {!hidden && <Cmd text={create + " --hidden"} />}
      <Cmd
        text={`layr overlay route ${target} ${(words(patterns).length ? words(patterns) : ["<pattern>"]).map(quote).join(" ")}`}
      />
    </CommandDialog>
  );
}

export function OverlaysPage({ project }: PageProps) {
  const [create, setCreate] = useState(false);
  return (
    <>
      <PageHead
        title="Overlays"
        description="Private files in this project. Each overlay has its own readers and key. Only readers see its files."
        actions={
          <button
            class="btn primary"
            type="button"
            onClick={() => setCreate(true)}
          >
            New overlay
          </button>
        }
      />
      {project.overlays === undefined ? (
        <Empty>No overlay records are available.</Empty>
      ) : !project.overlays.length ? (
        <Empty>No overlays are recorded.</Empty>
      ) : (
        <Table
          label="Project overlays"
          headers={[
            "Overlay",
            "Files",
            "Readers",
            "Your access",
            "Key",
            "Changed",
          ]}
          rows={project.overlays.map((overlay) => [
            <div class="row">
              <a
                href={projectUrl(
                  project.id,
                  `overlay/${encodeURIComponent(overlay.id)}`,
                )}
              >
                {overlay.name}
              </a>
              {overlay.hidden && <Badge kind="purple">hidden</Badge>}
            </div>,
            overlay.readable ? fileCount(overlay) : "Files are hidden",
            <div class="row" style={{ gap: 6 }}>
              {overlay.readers.length
                ? overlay.readers.map((reader) => (
                    <Principal key={reader} name={reader} />
                  ))
                : "No readers recorded"}
            </div>,
            <div class="row">
              <Access overlay={overlay} />
              {overlay.readable && overlay.managers.length === 1 && (
                <Badge kind="warn">● 1 key manager</Badge>
              )}
            </div>,
            overlay.readable ? (
              overlay.epoch ? (
                <Mono>epoch {overlay.epoch}</Mono>
              ) : (
                "Not recorded"
              )
            ) : (
              "Hidden"
            ),
            overlay.readable ? when(changed(overlay)) : "Hidden",
          ])}
        />
      )}
      <p class="footnote">
        Private overlays show their names to everyone. Only readers see their
        files. Hidden overlays appear only to their readers.
      </p>
      <NewOverlay open={create} onClose={() => setCreate(false)} />
    </>
  );
}

function OverlayDialogs({
  overlay,
  action,
  reader,
  onClose,
}: {
  overlay: Overlay;
  action: string;
  reader: string;
  onClose: () => void;
}) {
  const [addition, setAddition] = useState("");
  const [paths, setPaths] = useState("");
  const [manager, setManager] = useState("");
  const target = quote(overlay.name);
  const readersCommand = (values: string[]) =>
    `layr overlay readers ${target}${values.length ? " " + values.map(quote).join(" ") : ""}`;
  return (
    <>
      <CommandDialog
        title="Add reader"
        open={action === "add"}
        onClose={onClose}
      >
        <label class="field">
          <span>Person or machine</span>
          <input
            class="input mono"
            value={addition}
            placeholder="person:name or machine:name"
            onInput={(event) => setAddition(event.currentTarget.value)}
          />
        </label>
        <p class="muted">
          At the next push, Layr seals the overlay key to the reader's key in
          each store. No new epoch is needed.
        </p>
        <Cmd
          text={readersCommand([
            ...overlay.readers,
            addition.trim() || "<reader>",
          ])}
        />
      </CommandDialog>
      <CommandDialog
        title="Remove reader"
        open={action === "remove"}
        onClose={onClose}
        danger
      >
        <p>
          Remove <Mono>{reader}</Mono> from <Mono>{overlay.name}</Mono>.
        </p>
        <p>
          The next push starts a new key epoch. The removed reader cannot open
          data saved after that.
        </p>
        <div class="callout warn">
          <b>● Already copied files stay readable to this reader.</b>
          <p>Rotate the actual secrets, such as passwords and API keys.</p>
        </div>
        <Cmd
          text={readersCommand(
            overlay.readers.filter((value) => value !== reader),
          )}
        />
      </CommandDialog>
      <CommandDialog
        title="Move files into this overlay"
        label="Move"
        open={action === "move"}
        onClose={onClose}
      >
        <label class="field">
          <span>Paths</span>
          <input
            class="input mono"
            value={paths}
            placeholder="config/sentry.dsn"
            onInput={(event) => setPaths(event.currentTarget.value)}
          />
        </label>
        <p class="muted">
          From the next state, these files live in the overlay. Earlier states
          keep them in shared history. Layr does not rewrite history.
        </p>
        <Cmd
          text={`layr overlay move ${target} ${(words(paths).length ? words(paths) : ["<path>"]).map(quote).join(" ")}`}
        />
      </CommandDialog>
      <CommandDialog
        title="Add key manager"
        open={action === "manager"}
        onClose={onClose}
      >
        <label class="field">
          <span>Machine</span>
          <input
            class="input mono"
            value={manager}
            placeholder="machine:name"
            onInput={(event) => setManager(event.currentTarget.value)}
          />
        </label>
        <p class="note">Only key managers change readers and the key.</p>
        <Cmd
          text={`layr overlay managers ${target} ${[...overlay.managers, manager.trim() || "<machine>"].map(quote).join(" ")}`}
        />
      </CommandDialog>
    </>
  );
}

export function OverlayPage({ project, id }: PageProps & { id: string }) {
  const [action, setAction] = useState("");
  const [reader, setReader] = useState("");
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
  const paths = names(overlay);
  return (
    <>
      <p class="small" style={{ marginBottom: 8 }}>
        <a href={projectUrl(project.id, "overlays")}>Overlays</a>{" "}
        <span class="muted">/</span>
      </p>
      <PageHead
        title={
          <span class="mono" style={{ fontSize: 24 }}>
            {overlay.name}
          </span>
        }
        description={
          <>
            <Access overlay={overlay} />{" "}
            {overlay.hidden && <Badge kind="purple">hidden</Badge>}{" "}
            {overlay.readable && (
              <>
                {fileCount(overlay)} · {overlay.readers.length}{" "}
                {overlay.readers.length === 1 ? "reader" : "readers"} ·{" "}
                {overlay.managers.length}{" "}
                {overlay.managers.length === 1 ? "key manager" : "key managers"}{" "}
                ·{" "}
                {overlay.epoch
                  ? `key epoch ${overlay.epoch}`
                  : "Key epoch not recorded"}{" "}
                · <span class="stamp accent">encrypted</span>
              </>
            )}
          </>
        }
        actions={
          overlay.readable && (
            <button class="btn" type="button" onClick={() => setAction("move")}>
              Move files in
            </button>
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
              <Table
                label="Overlay files"
                headers={["Path", "Last state", "Size", "Changed"]}
                rows={paths.map((path) => {
                  const file = overlay.files?.find(
                    (item) => item.path === path,
                  );
                  return [
                    <Mono>{path}</Mono>,
                    file?.state ? (
                      <a
                        href={projectUrl(
                          project.id,
                          `state/${encodeURIComponent(file.state)}`,
                        )}
                      >
                        <Mono>{file.state}</Mono>
                      </a>
                    ) : (
                      "Not recorded"
                    ),
                    bytes(file?.size),
                    "Not recorded",
                  ];
                })}
              />
            )}
            {overlay.readable && (
              <p class="note" style={{ marginTop: 8 }}>
                Routed here:{" "}
                {overlay.patterns.length
                  ? overlay.patterns.map((pattern) => (
                      <span style={{ marginRight: 8 }} key={pattern}>
                        <Mono>{pattern}</Mono>
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
                <button
                  class="btn sm"
                  type="button"
                  onClick={() => setAction("add")}
                >
                  Add reader
                </button>
              )}
            </div>
            {!overlay.readers.length ? (
              <Empty>No readers are recorded.</Empty>
            ) : (
              <Table
                label="Overlay readers"
                headers={
                  overlay.readable
                    ? ["Reader", "Kind", "Key", "Since", "Action"]
                    : ["Reader", "Kind"]
                }
                rows={overlay.readers.map((principal) => {
                  const details = overlay.reader_details?.find(
                    (item) => item.principal === principal,
                  );
                  const identity = [
                    <Principal name={principal} />,
                    kind(principal),
                  ];
                  return overlay.readable
                    ? [
                        ...identity,
                        <Mono>{details?.key ?? "Not recorded"}</Mono>,
                        when(details?.since),
                        <button
                          class="btn sm danger"
                          type="button"
                          aria-label={`Remove reader ${principal}`}
                          onClick={() => {
                            setReader(principal);
                            setAction("remove");
                          }}
                        >
                          Remove
                        </button>,
                      ]
                    : identity;
                })}
              />
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
                          epoch <Mono>{epoch.id}</Mono>
                        </b>{" "}
                        <span class="when">· {when(epoch.time)}</span>
                        <p class="note">
                          {epoch.reason ?? "No reason is recorded."}
                        </p>
                        {epoch.machine && (
                          <p class="note">
                            Machine: <Mono>{epoch.machine}</Mono>
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
                    headers={["When", "Who", "What", "Operation", "Signature"]}
                    rows={overlay.history.map((operation) => [
                      when(operation.time),
                      <Who name={operation.actor.name} />,
                      <Mono>{operationName(operation.op)}</Mono>,
                      <Mono>{operation.id}</Mono>,
                      <span class="stamp accent">signed</span>,
                    ])}
                  />
                )}
                <Cmd text={`layr overlay log ${quote(overlay.name)}`} />
              </section>
            </>
          )}
        </div>
        {overlay.readable && (
          <aside class="side">
            <section>
              <h3>
                Key managers <span class="spacer" />
                <button
                  class="btn sm"
                  type="button"
                  onClick={() => setAction("manager")}
                >
                  Add
                </button>
              </h3>
              {overlay.managers.length ? (
                <ul>
                  {overlay.managers.map((manager) => (
                    <li key={manager}>
                      <Mono>{manager}</Mono>
                    </li>
                  ))}
                </ul>
              ) : (
                <p class="note">No key managers are recorded.</p>
              )}
              {overlay.managers.length === 1 && (
                <p style={{ marginTop: 8 }}>
                  <Badge kind="warn">● 1 key manager</Badge>
                </p>
              )}
              <p class="note" style={{ marginTop: 8 }}>
                Only managers change readers and the key. Other machines can
                push data encrypted to it.
              </p>
            </section>
            <section>
              <h3>Peers that may hold it</h3>
              {overlay.peers === undefined ? (
                <p class="note">No peer records are available.</p>
              ) : overlay.peers.length ? (
                <ul>
                  {overlay.peers.map((peer) => (
                    <li key={peer}>
                      <Mono>{peer}</Mono>
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
              <h3>In stores</h3>
              {!overlay.slots?.length ? (
                <p class="note">No store slots are recorded.</p>
              ) : (
                <ul>
                  {overlay.slots.map((slot) => (
                    <li key={`${slot.store}/${slot.epoch}`}>
                      <Mono>{slot.store}</Mono>
                      <span class="r">
                        epoch {slot.epoch} · {slot.count} slots
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </aside>
        )}
      </div>
      {overlay.readable && (
        <OverlayDialogs
          key={overlay.id}
          overlay={overlay}
          action={action}
          reader={reader}
          onClose={() => setAction("")}
        />
      )}
    </>
  );
}
