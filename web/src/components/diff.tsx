import { Fragment } from "preact";
import { CommentThread } from "./actions";
import { useEffect, useState } from "preact/hooks";
import type { DataSource, FileDiff } from "../data/source";
import { CodeText, Empty, Icon, LoadError, ToggleButtons, useData } from "./ui";
import { textDiff } from "../text-diff";
import { plural } from "../labels";
export type DiffComments = {source: DataSource; project: string; change: string; version: string; versions:{version:string;state:{id:string}}[]; focusPath?:string;focusLine?:number};
function InlineThread({comments,path,line}: {comments: DiffComments;path:string;line:number}) {
  return <CommentThread {...comments} path={path} line={line} />;
}
/** Text arrives from DataSource. It is always escaped by Preact. */
export function DiffFiles({
  files,
  total = files.length,
  mode: controlledMode,
  toolbar = true,
  comments,
}: {
  files: FileDiff[];
  total?: number;
  mode?: string;
  toolbar?: boolean;
  comments?: DiffComments;
}) {
  const [localMode, setMode] = useState("Unified");
  const mode = controlledMode ?? localMode;
  return (
    <div class={`diffs ${mode === "Split" ? "split" : ""}`}>
      {toolbar && (
        <div class="row diff-toolbar">
          <h2>{plural(total, "file")} changed</h2>
          <span class="spacer" />
          <ToggleButtons
            label="Diff view"
            items={["Unified", "Split"]}
            value={mode}
            onChange={setMode}
          />
        </div>
      )}
      {files.length ? (
        files.map((file) => (
          <DiffFile key={file.path} file={file} mode={mode} comments={comments} />
        ))
      ) : (
        <Empty>No file differences.</Empty>
      )}
    </div>
  );
}
function DiffFile({ file, mode, comments }: { file: FileDiff; mode: string; comments?: DiffComments }) {
  const [commentLine, setCommentLine] = useState(comments?.focusPath === file.path ? comments.focusLine ?? 0 : 0);
  useEffect(() => {
    if (comments?.focusPath === file.path && comments.focusLine) {
      setCommentLine(comments.focusLine);
      requestAnimationFrame(() => document.getElementById(`diff-line-${comments.focusLine}`)?.scrollIntoView({block:'center'}));
    }
  },[comments?.focusPath,comments?.focusLine,file.path]);
  const [show, setShow] = useState(!file.derived);
  const lines = textDiff(file.before ?? "", file.after ?? "");
  const modeLabel =
    file.before_mode !== file.after_mode &&
    (file.before_mode || file.after_mode)
      ? !file.before_mode
        ? "new file"
        : !file.after_mode
          ? "deleted"
          : `mode ${file.before_mode.slice(-3)} → ${file.after_mode.slice(-3)}`
      : file.before === undefined && file.after !== undefined
        ? "new file"
        : file.after === undefined && file.before !== undefined
          ? "deleted"
          : undefined;
  return (
    <div class="frame">
      <div class="fh">
        <span>{file.path}</span>
        {file.added !== undefined && <span class="add">+{file.added}</span>}
        {file.removed !== undefined && <span class="del">−{file.removed}</span>}
        {file.derived && <span class="badge">derived</span>}
        {modeLabel && <span class="muted small">{modeLabel}</span>}
        <span class="spacer" />
        {file.derived && (
          <button class="btn sm" type="button" onClick={() => setShow(!show)}>
            {show ? "Hide diff" : "Show diff"}
          </button>
        )}
      </div>
      {file.binary ? (
        <div class="collapsed-note">Binary file changed.</div>
      ) : !show ? (
        <div class="collapsed-note">Derived by {file.derived}.</div>
      ) : mode === "Split" ? (
        <div class="diff-split">
          <div>
            <div class="muted small diff-label">Base</div>
            <CodeText text={file.before ?? ""} />
          </div>
          <div>
            <div class="muted small diff-label">Head</div>
            {comments ? <><pre class="code" tabIndex={0}>{(file.after ?? '').split('\n').map((text,index) => <span class="l"><button class="comment-line" aria-label={`Comment on ${file.path} line ${index+1}`} onClick={() => setCommentLine(commentLine === index+1 ? 0 : index+1)}>+</button> {text || ' '}</span>)}</pre>{commentLine > 0 && <InlineThread comments={comments} path={file.path} line={commentLine} />}</> : <CodeText text={file.after ?? ""} />}
          </div>
        </div>
      ) : (
        <div
          class="diff-scroll"
          tabIndex={0}
          role="region"
          aria-label={`Diff of ${file.path}`}
        >
          <table class="diff" aria-label={`Diff of ${file.path}`}>
            <tbody>
              {lines.map((line) => (
                <Fragment>
                <tr id={line.after ? `diff-line-${line.after}` : undefined}
                  class={
                    line.kind === "add"
                      ? "a"
                      : line.kind === "remove"
                        ? "d"
                        : ""
                  }
                >
                  <td class="ln">{line.before ?? ""}</td>
                  <td class="ln">{comments && line.after ? <button class="comment-line" aria-label={`Comment on ${file.path} line ${line.after}`} onClick={() => setCommentLine(commentLine === line.after ? 0 : line.after!)}>{line.after}</button> : line.after ?? ""}</td>
                  <td class="sg">
                    {line.kind === "add"
                      ? "+"
                      : line.kind === "remove"
                        ? "−"
                        : " "}
                  </td>
                  <td class="tx">{line.text || " "}</td>
                </tr>
                {comments && line.after && commentLine === line.after && <tr class="inline-comment"><td colSpan={4}><InlineThread comments={comments} path={file.path} line={line.after} /></td></tr>}
                </Fragment>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

/** Metadata lists paths first. Only the selected path requests decrypted content. */
export function DiffBrowser({
  source,
  project,
  base,
  head,
  files,
  mode,
  comments,
}: {
  source: DataSource;
  project: string;
  base: string;
  head: string;
  files: FileDiff[];
  mode?: string;
  comments?: DiffComments;
}) {
  const [selected, setSelected] = useState(comments?.focusPath ?? "");
  const path = files.some((f) => f.path === selected)
    ? selected
    : files[0]?.path;
  const { data, error } = useData(
    async () => (path ? source.diff(project, base, head, path) : undefined),
    [source, project, base, head, path],
  );
  if (!files.length) return <DiffFiles files={[]} />;
  return (
    <div class="files-grid">
      <aside class="side" aria-label="Compared files">
        <section>
          <h2 class="side-title">{plural(files.length, "file")}</h2>
          <ul>
            {files.map((file) => (
              <li>
                <span class="filecell">
                  <Icon name="file" />
                  <button
                    class="file-link mono"
                    type="button"
                    aria-current={path === file.path ? "true" : undefined}
                    onClick={() => setSelected(file.path)}
                  >
                    {file.path}
                  </button>
                </span>
              </li>
            ))}
          </ul>
        </section>
      </aside>
      <div>
        {data ? (
          <DiffFiles
            files={data.files}
            total={files.length}
            mode={mode}
            toolbar={mode === undefined}
            comments={comments}
          />
        ) : (
          <LoadError error={error} />
        )}
      </div>
    </div>
  );
}
export function InterdiffBrowser({
  source,
  project,
  change,
  from,
  to,
  files,
  mode,
  comments,
}: {
  source: DataSource;
  project: string;
  change: string;
  from: string;
  to: string;
  files: import("../data/source").ChangeDiff["files"];
  mode: string;
  comments?: DiffComments;
}) {
  const [commentLine, setCommentLine] = useState(0);
  // The canonical from_trees comparison uses exact paths and does not detect renames.
  const [selected, setSelected] = useState(comments?.focusPath ?? "");
  const [unchanged, setUnchanged] = useState<Set<string>>(new Set());
  const path = files.some((file) => file.path === selected)
    ? selected
    : files[0]?.path;
  const { data, error } = useData(
    async () =>
      path ? source.changeDiff(project, change, from, to, path) : undefined,
    [source, project, change, from, to, path],
  );
  useEffect(() => {
    if (data && path && !data.files.length)
      setUnchanged((previous) =>
        previous.has(path) ? previous : new Set([...previous, path]),
      );
  }, [data, path]);
  if (!files.length)
    return <Empty>No code changes between these versions.</Empty>;
  const file = data?.files[0];
  let nextLine = 0;
  const patchLines = (file?.interdiff ?? '').split('\n').map(text => {
    const hunk = /^@@ -\d+(?:,\d+)? \+(\d+)/.exec(text);
    if (hunk) { nextLine = Number(hunk[1]); return {text}; }
    if (!nextLine || text.startsWith('---') || text.startsWith('+++') || text.startsWith('-') || text.startsWith('\\')) return {text};
    return {text, line: nextLine++};
  });
  return (
    <div class="files-grid">
      <aside class="side" aria-label="Compared files">
        <section>
          <h2 class="side-title">{plural(files.length, "file")}</h2>
          <ul>
            {files.map((file) => (
              <li>
                <span class="filecell">
                  <Icon name="file" />
                  <button
                    class={`file-link mono ${unchanged.has(file.path) ? "unchanged-file" : ""}`}
                    type="button"
                    aria-current={path === file.path ? "true" : undefined}
                    onClick={() => setSelected(file.path)}
                  >
                    {file.path}
                  </button>
                </span>
              </li>
            ))}
          </ul>
        </section>
      </aside>
      <div>
        {file ? (
          <div class="frame">
            <div class="fh">
              {file.path}
              {file.added !== undefined && (
                <span class="add">+{file.added}</span>
              )}
              {file.removed !== undefined && (
                <span class="del">−{file.removed}</span>
              )}
            </div>
            <div
              class="diff-scroll"
              tabIndex={0}
              role="region"
              aria-label={`Diff of ${file.path}`}
            >
              <table class="diff" aria-label={`Diff of ${file.path}`}>
                <tbody>
                  {patchLines.map(({text: line, line: number}) => <Fragment>
                    {mode === "Split" ? (
                      line.startsWith("@@") ? (
                        <tr class="h">
                          <td colspan={2}>{line}</td>
                        </tr>
                      ) : (
                        <tr>
                          <td class={line.startsWith("-") ? "tx del" : "tx"}>
                            {line.startsWith("+")
                              ? " "
                              : line.startsWith("-")
                                ? line.slice(1)
                                : line || " "}
                          </td>
                          <td class={line.startsWith("+") ? "tx add" : "tx"}>
                            {comments && number && <button class="comment-line" aria-label={`Comment on ${file.path} line ${number}`} onClick={() => setCommentLine(commentLine === number ? 0 : number)}>+</button>}
                            {line.startsWith("-")
                              ? " "
                              : line.startsWith("+")
                                ? line.slice(1)
                                : line || " "}
                          </td>
                        </tr>
                      )
                    ) : (
                      <tr
                        class={
                          line.startsWith("@@")
                            ? "h"
                            : line.startsWith("+") && !line.startsWith("+++")
                              ? "a"
                              : line.startsWith("-") && !line.startsWith("---")
                                ? "d"
                                : ""
                        }
                      >
                        <td class="tx">{comments && number && <button class="comment-line" aria-label={`Comment on ${file.path} line ${number}`} onClick={() => setCommentLine(commentLine === number ? 0 : number)}>+</button>} {line || " "}</td>
                      </tr>
                    )}
                    {comments && number && commentLine === number && <tr class="inline-comment"><td colSpan={mode === 'Split' ? 2 : 1}><InlineThread comments={comments} path={file.path} line={number} /></td></tr>}
                    </Fragment>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        ) : (
          <>
            {data ? (
              <Empty>No code changes in this file.</Empty>
            ) : (
              <LoadError error={error} />
            )}
          </>
        )}
      </div>
    </div>
  );
}
