import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { App } from "./app";
import { FixtureSource } from "./data/fixture-source";
import { ModelSource } from "./data/model-source";
import { Card, Empty, PageHead } from "./components/ui";

const fixtureFiles = import.meta.glob("../fixtures/*.json", {
  eager: true,
  import: "default",
});
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
  useEffect(() => {
    const close = () => {
      void source.lock();
    };
    addEventListener("pagehide", close);
    return () => removeEventListener("pagehide", close);
  }, [source]);
  if (unlocked)
    return (
      <App
        key={epoch}
        source={source}
        onLock={() => {
          setUnlocked(false);
          setEpoch((e) => e + 1);
        }}
      />
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
}: {
  source: ModelSource;
  onUnlock: () => void;
}) {
  const [words, setWords] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  async function unlock(passkey: boolean) {
    setBusy(true);
    setError("");
    try {
      if (passkey) await source.unlockPasskey();
      else {
        const input = words;
        setWords("");
        await source.unlockRecovery(input);
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
          title="Open your person key"
          description="One key opens every project you may read. Layr Cloud never holds it."
        />
        <Card title="Unlock">
          <div class="stack-s">
            <button
              class="btn primary"
              type="button"
              disabled={busy}
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
                disabled={busy || !words.trim()}
              >
                Unlock with recovery words
              </button>
            </form>
            {busy && (
              <p role="status" class="muted">
                Opening your key…
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
