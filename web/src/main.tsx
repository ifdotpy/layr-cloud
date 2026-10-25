import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { App } from "./app";
import { FixtureSource } from "./data/fixture-source";
import { ModelSource } from "./data/model-source";
import { Card, Empty, PageHead, LoadError, DataRevision } from "./components/ui";

const fixtureFiles = import.meta.glob("../fixtures/*.json", {
  eager: true,
  import: "default",
});
const publicAlias = new URLSearchParams(location.search).get("project");
const fixtureMode =
  new URLSearchParams(location.search).get("source") === "fixture";
function Root() {
  const [unlocked, setUnlocked] = useState(fixtureMode),
    [epoch, setEpoch] = useState(0);
  const [source] = useState(() =>
    fixtureMode
      ? new FixtureSource(fixtureFiles)
      : new ModelSource({
          onLock: () => {
            setUnlocked(false);
            setEpoch((e) => e + 1);
          },
        }),
  );
  const [publicError, setPublicError] = useState<Error>();
  const [openingPublic,setOpeningPublic] = useState(Boolean(publicAlias));
  const [signIn, setSignIn] = useState(false);
  useEffect(() => source instanceof ModelSource ? source.subscribe(() => setEpoch((e) => e + 1)) : undefined, [source]);
  useEffect(() => {
    if (!publicAlias || !(source instanceof ModelSource)) return;
    let active = true;
    void source.openPublic(publicAlias).then((id) => {
      if (!active) return;
      if (!location.hash || location.hash === "#/") location.hash = `#/project/${id}/code`;
      setUnlocked(true);
    }).catch((error) => { if (active) setPublicError(error); }).finally(() => { if(active) setOpeningPublic(false); });
    return () => { active = false; };
  }, [source]);
  useEffect(() => {
    const close = () => {
      void source.lock();
    };
    addEventListener("pagehide", close);
    return () => removeEventListener("pagehide", close);
  }, [source]);
  if (signIn && source instanceof ModelSource)
    return <Unlock source={source} onUnlock={() => { setSignIn(false); setUnlocked(true); setEpoch((e)=>e+1); }} onCancel={() => { void (async () => { await source.lock(); if (publicAlias) { await source.openPublic(publicAlias); setUnlocked(true); } setSignIn(false); })().catch(setPublicError); }} />;
  if (!unlocked && openingPublic) return <main class="page"><p class="note" role="status">Opening public project…</p></main>;
  if (!unlocked && publicError) return <main class="page"><PageHead title="This project is not public" /><button class="btn" type="button" onClick={() => setSignIn(true)}>Sign in</button></main>;
  if (unlocked)
    return (
      <DataRevision.Provider value={epoch}><App
        source={source}
        onSignIn={() => setSignIn(true)}
        onLock={() => {
          setUnlocked(false);
          setEpoch((e) => e + 1);
        }}
      /></DataRevision.Provider>
    );
  return source instanceof ModelSource ? (
    <Unlock source={source} onUnlock={() => setUnlocked(true)} />
  ) : (
    <main class="page">
      <PageHead title="Your session is locked" />
      <p class="muted">Reload the page to open the test project.</p>
    </main>
  );
}
function Unlock({
  source,
  onUnlock,
  onCancel,
}: {
  source: ModelSource;
  onUnlock: () => void;
  onCancel?: () => void;
}) {
  const [login, setLogin] = useState(''), [registering, setRegistering] = useState(false);
  const [invite, setInvite] = useState(''), [newWords, setNewWords] = useState(''), [saved, setSaved] = useState(false);
  const [words, setWords] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  async function unlock(passkey: boolean) {
    setBusy(true);
    setError("");
    try {
      if (passkey) await source.unlockPasskey(login.trim());
      else {
        const input = words;
        setWords("");
        await source.unlockRecovery(input,login.trim());
      }
      onUnlock();
    } catch (e) {
      setError(
        e instanceof Error ? e.message : "Cannot unlock your person key.",
      );
    } finally {
      setBusy(false);
      setWords("");
    }
  }
  return (
    <>
      <header class="topbar">
        <a class="wordmark" href="#/projects">
          Layr
        </a>
      </header>
      <main class="page unlock-page">
        <PageHead
          title={registering ? "Create your person key" : "Sign in"}
          description="One key opens every project you may read. Layr Cloud never holds it."
        />
        <Card title={registering ? "Join with an invite" : "Open your person key"}>
          <div class="stack-s">
            <label class="field"><span>Login</span><input class="input" autoComplete="username" autoCapitalize="off" spellcheck={false} value={login} disabled={busy || Boolean(newWords)} onInput={e => setLogin(e.currentTarget.value)} /></label>
            {registering ? <form class="stack-s" onSubmit={async e => {
              e.preventDefault(); setBusy(true); setError('');
              try {
                if (!newWords) { const result = await source.prepareRegistration(login.trim()); setNewWords(result.words); }
                else { const token = invite; setInvite(''); await source.registerPerson(token); setNewWords(''); onUnlock(); }
              } catch(e) { setError(e instanceof Error ? e.message : 'Could not create your person key.'); }
              finally { setBusy(false); }
            }}>
              <label class="field"><span>Invite token</span><input class="input" type="password" autoComplete="off" value={invite} disabled={busy} onInput={e => setInvite(e.currentTarget.value)} /></label>
              {newWords && <><label class="field"><span>Your 24 recovery words</span><textarea class="input" rows={4} readOnly value={newWords} autoComplete="off" spellcheck={false} /></label><p class="note">Keep these words somewhere private. They open your person key.</p><label><input type="checkbox" checked={saved} onChange={e => setSaved(e.currentTarget.checked)} /> I saved my recovery words</label></>}
              <button class="btn primary" disabled={busy || !login.trim() || !invite.trim() || (Boolean(newWords) && !saved)}>{newWords ? 'Create account and sign in' : 'Create person key'}</button>
            </form> : <>
            <button
              class="btn primary"
              type="button"
              disabled={busy || !login.trim()}
              onClick={() => unlock(true)}
            >
              Unlock with a passkey
            </button>
            <form
              class="stack-s"
              onSubmit={(e) => {
                e.preventDefault();
                void unlock(false);
              }}
            >
              <label class="field">
                <span>24 recovery words</span>
                <textarea
                  class="input"
                  rows={4}
                  value={words}
                  onInput={(e) => setWords(e.currentTarget.value)}
                  autoComplete="off"
                  autoCapitalize="off"
                  spellcheck={false}
                />
              </label>
              <button
                class="btn"
                type="submit"
                disabled={busy || !login.trim() || !words.trim()}
              >
                Unlock with recovery words
              </button>
            </form>
            </>}
            <button class="btn" type="button" disabled={busy} onClick={() => { setRegistering(value => !value); setError(''); setWords(''); setInvite(''); setNewWords(''); setSaved(false); }}>{registering ? 'Use an existing person key' : 'Join with an invite'}</button>
            {onCancel && <button class="btn" type="button" onClick={onCancel}>Back to public project</button>}
            {busy && (
              <p role="status" class="muted">
                Signing in…
              </p>
            )}
            {error && (
              <div class="callout bad" role="alert">
                {error}
              </div>
            )}
            <p class="note">
              Closing the tab or 30 minutes without use locks your key.
            </p>
          </div>
        </Card>
      </main>
    </>
  );
}
render(<Root />, document.getElementById("app")!);
