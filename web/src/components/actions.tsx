import { useEffect, useState } from 'preact/hooks';
import type { DataSource, RequestAction, RequestStatus } from '../data/source';
import { Cmd, Mono, Status, useData, when } from './ui';
import { parseRoute, projectUrl, queryUrl } from '../router';
import { personLabel } from '../labels';

// Preserve pending identities when a page or a version selection is reopened.
const pending = new WeakMap<DataSource, Map<string, RequestStatus>>();
export function RequestActionButton({source, project, action, label, command, onApplied, primary = false, disabled = false, requestKey}: {
  source: DataSource; project: string; action: RequestAction; label: string; command?: string;
  onApplied?: () => void; primary?: boolean; disabled?: boolean; requestKey?: string;
}) {
  const key = project + (requestKey ?? JSON.stringify(action));
  let requests = pending.get(source);
  if (!requests) { requests = new Map(); pending.set(source, requests); }
  const [request, setRequest] = useState<RequestStatus | undefined>(requests.get(key));
  const [sending, setSending] = useState(false), [error, setError] = useState('');
  const {data: access, error: accessError} = useData(() => source.actionAccess(project), [source, project]);
  const remember = (value: RequestStatus) => { requests!.set(key, value); setRequest(value); };
  useEffect(() => { setRequest(requests!.get(key)); setError(''); }, [source, key]);
  useEffect(() => {
    if (request?.status !== 'pending') return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    async function check() {
      try {
        const value = await source.requestStatus(project, request!.id);
        if (cancelled) return;
        remember(value); setError('');
        if (value.status === 'applied') onApplied?.();
        if (value.status !== 'pending') return;
      } catch (error) { if (!cancelled) setError(error instanceof Error ? error.message : String(error)); }
      if (!cancelled) timer = setTimeout(check, 2000);
    }
    timer = setTimeout(check, 500);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [source, project, request?.id, request?.status]);
  async function send() {
    setSending(true); setError('');
    try { remember(await source.submitRequest(project, action)); }
    catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setSending(false); }
  }
  return <div class="request-action">
    <div class="action-row"><button type="button" class={`btn${primary ? ' primary' : ''}`} disabled={disabled || !access?.available || sending || request?.status === 'pending'} onClick={send}>{sending ? 'Sending…' : label}</button>{command && <Cmd text={command} />}</div>
    {!access?.available && <p class="note">{access?.reason || accessError?.message || 'Checking your signing key…'}</p>}
    {request && <div class="request-outcome" role="status"><Status value={request.status} /> <span>{request.status === 'pending' ? 'Waiting for the executor.' : request.status === 'applied' ? 'Recorded by the executor.' : request.reason || 'The executor refused this request.'}</span><span class="note">Request <Mono>{request.id}</Mono></span>{request.status === 'pending' && request.reason && <p class="note">{request.reason}</p>}</div>}
    {request?.status === 'pending' && request.reason && <button class="btn" type="button" disabled={sending} onClick={async () => {
      setSending(true);
      try { remember(await source.retryRequest(project,request.id)); } catch (error) { setError(error instanceof Error ? error.message : String(error)); }
      finally { setSending(false); }
    }}>Retry delivery</button>}
    {error && <p class="note" role="alert">{error}</p>}
  </div>;
}

export function CommentComposer({source, project, change, version, path, line, replyTo, onApplied, versionName}: {
  source: DataSource; project: string; change: string; version: string;
  path?: string; line?: number; replyTo?: string; onApplied: () => void; versionName?: string;
}) {
  const [text, setText] = useState('');
  const command = `layr request comment ${change} <text> --person${versionName ? ` --version ${versionName}` : ''}${path ? ` --path ${path}` : ''}${line ? ` --line ${line}` : ''}${replyTo ? ` --reply-to ${replyTo}` : ''}`;
  const action: RequestAction = {kind:'comment', change_id:change, version, text, ...(path ? {path} : {}), ...(line ? {line} : {}), ...(replyTo ? {reply_to:replyTo} : {})};
  return <div class="comment-composer">
    <label>Comment<textarea class="input" aria-label={replyTo ? 'Reply' : line ? `Comment on line ${line}` : 'Comment'} rows={3} maxLength={65536} value={text} onInput={event => setText(event.currentTarget.value)} /></label>
    {text.trim() ? <RequestActionButton source={source} project={project} action={action} label={replyTo ? 'Send reply' : 'Send comment'} command={command} onApplied={() => {
      const route = parseRoute(location.hash);
      if (path && line && route.page === 'change' && route.id === 'files') location.hash = queryUrl(projectUrl(project,`change/${change}/files`),{state:version,path,line:String(line)});
      setText(current => current === text ? '' : current); onApplied();
    }} /> : <div class="action-row"><button type="button" class="btn" disabled>{replyTo ? 'Send reply' : 'Send comment'}</button><Cmd text={command} /></div>}
  </div>;
}
export function CommentThread({source, project, change, version, path, line, refresh = 0, versions}: {
  source: DataSource; project: string; change: string; version: string; path?: string; line?: number; refresh?: number; versions: {version:string;state:{id:string}}[];
}) {
  const [revision, setRevision] = useState(0), [reply, setReply] = useState('');
  const {data: comments, error} = useData(() => source.comments(project, change), [source, project, change, revision, refresh]);
  const rows = (comments ?? []).filter(comment => path ? comment.path === path && comment.version === version && (line === undefined || comment.line === line) : true);
  const applied = () => { setReply(''); setRevision(value => value + 1); };
  return <div class="conversation">
    {error ? <p class="note" role="alert">{error.message}</p> : !comments ? <p class="note">Loading comments…</p> : !rows.length ? <p class="note">No comments yet.</p> : rows.map(comment => {
      const parent = comments!.find(parent => parent.id === comment.reply_to);
      return <article class={`comment${comment.reply_to ? ' reply' : ''}`} key={comment.id} id={`comment-${comment.id}`}>
        <div class="meta"><b>{personLabel(comment.actor.name)}</b><span>{when(comment.time)}</span><span title={comment.version}>on {versions.find(version => version.state.id === comment.version)?.version ?? "an unavailable version"}</span></div>
        {parent && <p class="note">Reply to {personLabel(parent.actor.name)}: <a href={`#comment-${parent.id}`} onClick={event => {event.preventDefault(); document.getElementById(`comment-${parent.id}`)?.scrollIntoView();}}>{parent.text.slice(0,120)}</a></p>}
        {comment.path && <p class="note mono"><a href={queryUrl(projectUrl(project,`change/${change}/files`),{state:comment.version,path:comment.path,line:comment.line ? String(comment.line) : undefined})}>{comment.path}{comment.line ? `:${comment.line}` : ''}</a></p>}
        <p class="comment-text">{comment.text}</p>
        <button type="button" class="btn sm" onClick={() => setReply(reply === comment.id ? '' : comment.id)}>Reply</button>
        {reply === comment.id && <CommentComposer source={source} project={project} change={change} version={comment.version} versionName={versions.find(version => version.state.id === comment.version)?.version} path={comment.path} line={comment.line} replyTo={comment.id} onApplied={applied} />}
      </article>;
    })}
    <CommentComposer source={source} project={project} change={change} version={version} versionName={versions.find(value => value.state.id === version)?.version} path={path} line={line} onApplied={applied} />
  </div>;
}
export function RefMove({source, project, name, expected, onApplied}: {source: DataSource; project: string; name: string; expected: string | null; onApplied?: () => void}) {
  const [target, setTarget] = useState('');
  return <div class="stack-s"><label>Target state<input class="input mono" value={target} onInput={event => setTarget(event.currentTarget.value.trim())} /></label>
    <p class="note">Expected state: <Mono>{expected ?? 'none'}</Mono></p>
    <RequestActionButton source={source} project={project} label={`Move ${name}`} disabled={!target} requestKey={`ref:${name}`} action={{kind:'ref_move',name,target,expected}} command={`layr request ref-move ${name} ${target || '<state>'} --expect ${expected ?? 'none'} --person`} onApplied={onApplied} />
  </div>;
}
