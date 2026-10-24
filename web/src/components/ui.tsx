import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { operationLabel, plural } from "../labels";

export const operationName = operationLabel;
export const short = (id?: string) =>
  id ? id.replaceAll("-", "").slice(0, 12) : "Unknown";
export const when = (time?: number) =>
  time === undefined
    ? "Not recorded"
    : new Date(time).toLocaleString("en-GB", {
        timeZone: "UTC",
        day: "2-digit",
        month: "short",
        hour: "2-digit",
        minute: "2-digit",
      }) + " UTC";
export const bytes = (n?: number) =>
  n === undefined
    ? "Not recorded"
    : n < 1024
      ? `${n} B`
      : n < 1048576
        ? `${(n / 1024).toFixed(1)} KiB`
        : `${(n / 1048576).toFixed(1)} MiB`;
export function Mono({ children }: { children: ComponentChildren }) {
  return <span class="mono">{children}</span>;
}
export function Badge({
  children,
  kind = "",
}: {
  children: ComponentChildren;
  kind?: string;
}) {
  return <span class={`badge ${kind}`}>{children}</span>;
}
export function Status({ value }: { value?: string }) {
  const label = value ?? "Not recorded";
  const ok = ["passed", "done", "same", "approved", "merged"].includes(label);
  const bad = ["failed", "refused", "conflict"].includes(label);
  const pending = ["pending", "running", "changed", "queued"].includes(label);
  return (
    <Badge
      kind={
        label === "merged"
          ? "purple"
          : ok
            ? "ok"
            : bad
              ? "bad"
              : pending
                ? "warn"
                : ""
      }
    >
      {ok ? "✓ " : bad ? "✕ " : pending ? "● " : ""}
      {label === "done" ? "merged" : label}
    </Badge>
  );
}
export function Avatar({ name }: { name: string }) {
  return (
    <span class="avatar c4 sm" aria-hidden="true">
      {name[0]?.toUpperCase() ?? "?"}
    </span>
  );
}
export function Who({ name }: { name: string }) {
  return (
    <span class="row">
      <Avatar name={name} />
      <span>{name}</span>
    </span>
  );
}
export function Icon({
  name,
}: {
  name: "file" | "folder" | "copy" | "search" | "close";
}) {
  return (
    <svg
      class="icon"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {name === "file" ? (
        <>
          <path d="M3.75 1.75h5.5l3 3v9.5h-8.5z" />
          <path d="M9.25 1.75v3h3" />
        </>
      ) : name === "folder" ? (
        <path d="M1.75 3.75h4.5l1.5 1.5h6.5v7.5h-12.5z" />
      ) : name === "copy" ? (
        <>
          <rect x="5.25" y="5.25" width="8.5" height="8.5" rx="1.5" />
          <path d="M10.75 5.25v-2a1 1 0 0 0-1-1h-6.5a1 1 0 0 0-1 1v6.5a1 1 0 0 0 1 1h2" />
        </>
      ) : name === "search" ? (
        <>
          <circle cx="7" cy="7" r="4.5" />
          <path d="M10.5 10.5 14 14" />
        </>
      ) : (
        <path d="m4 4 8 8M12 4l-8 8" />
      )}
    </svg>
  );
}
export function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      class="iconbtn"
      type="button"
      aria-label={copied ? "Copied" : "Copy command"}
      title={copied ? "Copied" : text}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setCopied(true);
        } catch {
          setCopied(false);
        }
      }}
    >
      <Icon name="copy" />
    </button>
  );
}
export function Cmd({ text }: { text: string }) {
  return (
    <div class="cmd">
      <span class="chip" title={text}>
        CLI
      </span>
      <span class="t">{text}</span>
      <CopyButton text={text} />
    </div>
  );
}
export function PageHead({
  title,
  description,
  actions,
  readOnly = Boolean(actions),
}: {
  title: ComponentChildren;
  description?: ComponentChildren;
  actions?: ComponentChildren;
  readOnly?: boolean;
}) {
  return (
    <div class="pagehead">
      <div>
        <h1>{title}</h1>
        {readOnly && <ReadOnlyNote />}
        {description && <p class="lede">{description}</p>}
      </div>
      {actions && <div class="right">{actions}</div>}
    </div>
  );
}
export function Empty({ children }: { children: ComponentChildren }) {
  return (
    <div class="empty">
      <p class="muted">{children}</p>
    </div>
  );
}
export function Card({
  title,
  children,
  actions,
}: {
  title?: ComponentChildren;
  children: ComponentChildren;
  actions?: ComponentChildren;
}) {
  return (
    <div class="card">
      {title && (
        <div class="hd">
          <b>{title}</b>
          <span class="spacer" />
          {actions}
        </div>
      )}
      <div class="bd">{children}</div>
    </div>
  );
}
export function Section({
  title,
  description,
  children,
}: {
  title: string;
  description?: ComponentChildren;
  children: ComponentChildren;
}) {
  return (
    <section class="sect">
      <h2>{title}</h2>
      {description && <p class="lede">{description}</p>}
      {children}
    </section>
  );
}
export function Table({
  headers,
  rows,
  stacked = false,
  label,
}: {
  headers: string[];
  rows: ComponentChildren[][];
  stacked?: boolean;
  label?: string;
}) {
  return (
    <div
      class="tbl-frame"
      tabIndex={0}
      role="region"
      aria-label={label ?? headers.join(", ")}
    >
      <table class={stacked ? "stacked" : ""} aria-label={label}>
        <thead>
          <tr>
            {headers.map((h, i) => (
              <th key={i} scope="col">
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i}>
              {row.map((cell, j) => (
                <td
                  key={j}
                  class={j === 0 ? "first" : ""}
                  data-l={stacked && j > 0 ? headers[j] : undefined}
                >
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
export function Segments({
  items,
  current,
}: {
  items: [string, string][];
  current: string;
}) {
  return (
    <div class="seg subnav">
      {items.map(([name, href]) => (
        <a href={href} aria-current={current === name ? "true" : undefined}>
          {name}
        </a>
      ))}
    </div>
  );
}
export function ToggleButtons({
  items,
  value,
  onChange,
  label,
}: {
  items: string[];
  value: string;
  onChange: (v: string) => void;
  label: string;
}) {
  return (
    <div class="seg" role="group" aria-label={label}>
      {items.map((item) => (
        <button
          type="button"
          aria-pressed={value === item}
          onClick={() => onChange(item)}
        >
          {item}
        </button>
      ))}
    </div>
  );
}
export function Dialog({
  title,
  children,
  open,
  onClose,
}: {
  title: string;
  children: ComponentChildren;
  open: boolean;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const label = useRef("dialog-" + Math.random().toString(36).slice(2));
  useEffect(() => {
    const d = ref.current;
    if (open && d && !d.open) d.showModal();
    if (!open && d?.open) d.close();
  }, [open]);
  return (
    <dialog
      ref={ref}
      aria-labelledby={label.current}
      onClose={onClose}
      onClick={(e) => {
        if (e.target === ref.current) onClose();
      }}
    >
      <div class="dh">
        <h2 id={label.current}>{title}</h2>
        <button
          class="iconbtn"
          type="button"
          onClick={onClose}
          aria-label="Close"
        >
          <Icon name="close" />
        </button>
      </div>
      <div class="db">{children}</div>
      <div class="df">
        <button class="btn" type="button" onClick={onClose}>
          Close
        </button>
      </div>
    </dialog>
  );
}
export function ReadOnlyNote() {
  return <p class="note readonly-note">Read only here. Use the CLI command.</p>;
}
export function ReadOnlyAction({
  label,
  command,
  primary = false,
}: {
  label: string;
  command?: string;
  primary?: boolean;
}) {
  return (
    <div class="readonly-action">
      <button
        class={`btn ${primary ? "primary" : ""}`}
        type="button"
        disabled
        title="Read only here. Use the CLI command."
      >
        {label}
      </button>
      {command && <Cmd text={command} />}
    </div>
  );
}
export function CodeText({ text }: { text: string }) {
  return (
    <pre class="code" tabIndex={0}>
      {text.split("\n").map((line, i) => (
        <span class="l" key={i}>
          {line || " "}
        </span>
      ))}
    </pre>
  );
}
/** Async results are cancelled when the route changes, preventing old project data from flashing. */
export function useData<T>(load: () => Promise<T>, deps: unknown[]) {
  const token = useMemo(() => ({}), deps);
  const [result, setResult] = useState<{
    token?: object;
    data?: T;
    error?: Error;
  }>({});
  useEffect(() => {
    let active = true;
    setResult({});
    load().then(
      (data) => {
        if (active) setResult({ token, data });
      },
      (cause) => {
        if (active)
          setResult({
            token,
            error: cause instanceof Error ? cause : new Error(String(cause)),
          });
      },
    );
    return () => {
      active = false;
    };
  }, deps);
  return result.token === token ? result : {};
}
export function LoadError({ error }: { error?: Error }) {
  return error ? (
    <div class="callout bad" role="alert">
      {error.message}
    </div>
  ) : (
    <p role="status" class="muted">
      Loading…
    </p>
  );
}
export function CopyRing({ copy }: { copy: import("../data/source").Copy }) {
  if (copy.present === undefined || copy.expected === undefined) return null;
  const total = copy.expected,
    have = copy.present,
    full = have === total;
  const circumference = 2 * Math.PI * 34;
  const fraction =
    total === 0 ? (have === 0 ? 1 : 0) : Math.min(1, Math.max(0, have / total));
  return (
    <div class="ring-box">
      <svg
        class="ring"
        viewBox="0 0 84 84"
        role="img"
        aria-label={`${have} of ${plural(total, "state")}`}
      >
        <circle cx="42" cy="42" r="34" class="ring-bg" />
        <circle
          cx="42"
          cy="42"
          r="34"
          class={`ring-fg ${full ? "full" : ""}`}
          stroke-dasharray={circumference}
          stroke-dashoffset={circumference * (1 - fraction)}
        />
        <text x="42" y="42" text-anchor="middle" class="ring-v">
          {have}
        </text>
        <text x="42" y="56" text-anchor="middle" class="ring-s">
          of {total}
        </text>
      </svg>
      <div>
        <b>{copy.name}</b>
        <div>
          {full ? (
            <span class="stamp green">complete</span>
          ) : (
            <span class="muted small">
              {plural(Math.max(0, total - have), "state")} not here yet
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
export function Dropdown({
  label,
  children,
}: {
  label: ComponentChildren;
  children: ComponentChildren | ((close: () => void) => ComponentChildren);
}) {
  const [open, setOpen] = useState(false),
    ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!open) return;
    ref.current
      ?.querySelector<HTMLElement>(
        ".menu button, .menu input, .menu select, .menu a",
      )
      ?.focus();
    const click = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        ref.current
          ?.querySelector<HTMLButtonElement>(":scope > button")
          ?.focus();
      }
    };
    document.addEventListener("click", click);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("click", click);
      document.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <div class="menu-wrap" ref={ref}>
      <button
        class="btn"
        type="button"
        aria-haspopup="true"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        {label} ▾
      </button>
      <div class={`menu ${open ? "open" : ""}`}>
        {typeof children === "function"
          ? children(() => setOpen(false))
          : children}
      </div>
    </div>
  );
}
