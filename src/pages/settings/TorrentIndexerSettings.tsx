import { useCallback, useEffect, useState, type FormEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  getRuTrackerStatus,
  listTorrentIndexers,
  openRuTrackerLogin,
  setTorrentIndexerEnabled,
  signInRuTracker,
  signOutRuTracker,
  testTorrentIndexer,
  type RuTrackerStatus,
  type TorrentIndexer,
  type TorrentIndexerTest,
} from "@/shared/lib/audio.ts";
import styles from "./settings.module.css";

function testStatus(result: TorrentIndexerTest): string {
  if (result.error) return result.error;
  if (result.hits === 0) return "Reachable · No sample results";
  return `Reachable · ${result.hits} sample result${result.hits === 1 ? "" : "s"}`;
}

export function TorrentIndexerSettings() {
  const [indexers, setIndexers] = useState<TorrentIndexer[]>();
  const [ruStatus, setRuStatus] = useState<RuTrackerStatus>();
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [checks, setChecks] = useState<Partial<Record<TorrentIndexer["id"], string>>>({});
  const [testing, setTesting] = useState<TorrentIndexer["id"]>();

  const [showRuForm, setShowRuForm] = useState(false);
  const [ruUsername, setRuUsername] = useState("");
  const [ruPassword, setRuPassword] = useState("");
  const [signingIn, setSigningIn] = useState(false);
  const [signingOut, setSigningOut] = useState(false);
  const [openingBrowser, setOpeningBrowser] = useState(false);
  const [ruError, setRuError] = useState<string>();
  const [offerBrowserFallback, setOfferBrowserFallback] = useState(false);

  const refresh = useCallback(async () => {
    const [nextIndexers, nextStatus] = await Promise.all([
      listTorrentIndexers(),
      getRuTrackerStatus().catch(() => undefined),
    ]);
    setIndexers(nextIndexers);
    if (nextStatus) setRuStatus(nextStatus);
  }, []);

  useEffect(() => {
    let current = true;
    refresh().catch((reason) => {
      if (current) setError(String(reason));
    });
    return () => { current = false; };
  }, [loadAttempt, refresh]);

  useEffect(() => {
    let disposed = false;
    const unlistenPromise = Promise.all([
      listen("rutracker-status-changed", () => {
        if (disposed) return;
        setShowRuForm(false);
        setRuError(undefined);
        setOfferBrowserFallback(false);
        setChecks((prev) => ({ ...prev, ru_tracker: undefined }));
        void refresh();
      }),
      listen("torrent-indexers-changed", () => {
        if (!disposed) void refresh();
      }),
    ]);
    return () => {
      disposed = true;
      void unlistenPromise.then((unlisteners) => unlisteners.forEach((fn) => fn()));
    };
  }, [refresh]);

  async function toggle(indexer: TorrentIndexer, enabled: boolean) {
    if (indexer.id === "ru_tracker" && enabled && !ruStatus?.signedIn) {
      setShowRuForm(true);
      return;
    }
    setSaving(true);
    setError(undefined);
    try {
      setIndexers(await setTorrentIndexerEnabled(indexer.id, enabled));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSaving(false);
    }
  }

  async function test(id: TorrentIndexer["id"]) {
    setTesting(id);
    try {
      const result = await testTorrentIndexer(id);
      setChecks((checks) => ({ ...checks, [id]: testStatus(result) }));
    } catch (reason) {
      setChecks((checks) => ({ ...checks, [id]: String(reason) }));
    } finally {
      setTesting(undefined);
    }
  }

  async function handleCredentialSignIn(event: FormEvent) {
    event.preventDefault();
    if (!ruUsername.trim() || !ruPassword) {
      setRuError("Enter your RuTracker username and password.");
      return;
    }
    setSigningIn(true);
    setRuError(undefined);
    setOfferBrowserFallback(false);
    try {
      const result = await signInRuTracker(ruUsername.trim(), ruPassword);
      if (result.signedIn) {
        setRuPassword("");
        setShowRuForm(false);
        setChecks((prev) => ({ ...prev, ru_tracker: undefined }));
        await refresh();
      } else {
        setRuError(result.error ?? "Could not sign in to RuTracker.");
        setOfferBrowserFallback(result.offerBrowserFallback);
      }
    } catch (reason) {
      setRuError(String(reason));
      setOfferBrowserFallback(true);
    } finally {
      setSigningIn(false);
    }
  }

  async function openBrowserFallback() {
    setOpeningBrowser(true);
    setRuError(undefined);
    try {
      await openRuTrackerLogin();
      setChecks((checks) => ({
        ...checks,
        ru_tracker: "Waiting for RuTracker sign-in in the browser window…",
      }));
    } catch (reason) {
      setRuError(String(reason));
    } finally {
      setOpeningBrowser(false);
    }
  }

  async function handleSignOut() {
    setSigningOut(true);
    setRuError(undefined);
    try {
      await signOutRuTracker();
      setRuStatus({ signedIn: false, username: null, hasSavedCredentials: false });
      setChecks((prev) => ({ ...prev, ru_tracker: undefined }));
      setRuPassword("");
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSigningOut(false);
    }
  }

  function subtitleFor(indexer: TorrentIndexer): string {
    if (testing === indexer.id) return "Testing…";
    const check = checks[indexer.id];
    if (check) return check;
    if (indexer.id === "ru_tracker") {
      if (ruStatus?.signedIn) {
        return ruStatus.username ? `Signed in as ${ruStatus.username}` : "Signed in";
      }
      return "Requires a RuTracker account";
    }
    return "Built-in";
  }

  return (
    <section className={styles.section} aria-labelledby="torrent-indexer-heading">
      <h2 id="torrent-indexer-heading">Torrent indexers</h2>
      {error && <p role="alert">{error}</p>}
      {!indexers && (error ? (
        <button className={styles.actionButton} type="button" onClick={() => {
          setError(undefined);
          setLoadAttempt((attempt) => attempt + 1);
        }}>Retry</button>
      ) : <p role="status" className={styles.mutedText}>Loading indexers…</p>)}
      {indexers?.every((indexer) => !indexer.enabled) &&
        <p className={styles.mutedText}>Torrent discovery is off.</p>}
      {indexers?.map((indexer) => {
        const isRuTracker = indexer.id === "ru_tracker";
        const signedIn = isRuTracker && Boolean(ruStatus?.signedIn);
        return (
          <div key={indexer.id} className={isRuTracker ? styles.indexerCard : styles.settingRow}>
            <div className={isRuTracker ? styles.indexerHeader : styles.libraryInfo} style={isRuTracker ? undefined : { display: "contents" }}>
              <div className={styles.libraryInfo}>
                <label htmlFor={`torrent-indexer-${indexer.id}`}>{indexer.name}</label>
                <span className={styles.mutedText} role="status">
                  {subtitleFor(indexer)}
                </span>
              </div>
              <div className={styles.indexerActions}>
                {isRuTracker && (signedIn ? (
                  <button
                    className={styles.actionButton}
                    type="button"
                    disabled={signingOut || testing !== undefined}
                    onClick={() => void handleSignOut()}
                  >
                    {signingOut ? "Signing out…" : "Sign out"}
                  </button>
                ) : (
                  <button
                    className={styles.actionButton}
                    type="button"
                    disabled={signingIn || openingBrowser}
                    onClick={() => {
                      setShowRuForm((open) => !open);
                      setRuError(undefined);
                    }}
                  >
                    {showRuForm ? "Cancel" : "Sign in"}
                  </button>
                ))}
                <input
                  id={`torrent-indexer-${indexer.id}`}
                  type="checkbox"
                  checked={indexer.enabled}
                  disabled={saving}
                  onChange={(event) => void toggle(indexer, event.target.checked)}
                />
                <button
                  className={styles.actionButton}
                  type="button"
                  disabled={testing !== undefined}
                  aria-label={`Test ${indexer.name} connection`}
                  onClick={() => void test(indexer.id)}
                >
                  Test
                </button>
              </div>
            </div>

            {isRuTracker && !signedIn && showRuForm && (
              <form className={styles.rutrackerForm} onSubmit={(e) => void handleCredentialSignIn(e)}>
                <div className={styles.rutrackerInputs}>
                  <input
                    className={styles.textInput}
                    type="text"
                    placeholder="RuTracker username"
                    aria-label="RuTracker username"
                    autoComplete="username"
                    value={ruUsername}
                    disabled={signingIn}
                    onChange={(e) => setRuUsername(e.target.value)}
                  />
                  <input
                    className={styles.textInput}
                    type="password"
                    placeholder="Password"
                    aria-label="RuTracker password"
                    autoComplete="current-password"
                    value={ruPassword}
                    disabled={signingIn}
                    onChange={(e) => setRuPassword(e.target.value)}
                  />
                  <button
                    className={styles.actionButton}
                    type="submit"
                    disabled={signingIn || openingBrowser}
                  >
                    {signingIn ? "Signing in…" : "Sign in"}
                  </button>
                </div>
                {ruError && (
                  <p className={styles.errorText} role="alert">
                    {ruError}
                  </p>
                )}
                <div className={styles.rutrackerFormActions}>
                  <span className={styles.mutedText}>
                    Saved in your system credential store for automatic session renewal.
                  </span>
                  {offerBrowserFallback ? (
                    <button
                      className={styles.actionButton}
                      type="button"
                      disabled={openingBrowser || signingIn}
                      onClick={() => void openBrowserFallback()}
                    >
                      Sign in in browser
                    </button>
                  ) : (
                    <button
                      className={styles.linkButton}
                      type="button"
                      disabled={openingBrowser || signingIn}
                      onClick={() => void openBrowserFallback()}
                    >
                      Sign in in browser
                    </button>
                  )}
                </div>
              </form>
            )}
          </div>
        );
      })}
    </section>
  );
}
