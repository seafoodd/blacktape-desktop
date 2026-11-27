import { useEffect, useId, useState, type MouseEvent } from "react";
import clsx from "clsx";
import {
  formatElapsed,
  SOURCE_NAMES,
  type SearchRun,
  type SourceProgress,
} from "@/shared/lib/search/searchProgress.ts";
import styles from "./search-progress.module.css";

const TICK_MS = 100;

/** The current time, refreshed while `running`. */
function useNow(running: boolean): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(timer);
  }, [running]);
  return now;
}

function describe(source: SourceProgress, now: number): string {
  const elapsed = source.elapsedMs === null ? "" : formatElapsed(source.elapsedMs);
  switch (source.state) {
    case "waiting":
      return "waiting";
    case "running":
      return `searching ${formatElapsed(now - (source.startedAt ?? now))}`;
    case "done":
      return `${elapsed} · ${source.count ?? 0} found`;
    case "failed":
      return `failed after ${elapsed}`;
    case "unknown":
      return "no timing";
  }
}

/** For clicks inside the progress: the search field keeps the focus, so the dropdown stays open. */
const keepFocus = (e: MouseEvent) => e.preventDefault();

/** One chip per source: what is being asked, how long it takes and what it found. */
export const SourceChips = ({ sources }: { sources: SourceProgress[] }) => {
  const now = useNow(sources.some((s) => s.state === "running"));
  if (sources.length === 0) return null;
  return (
    <ul className={styles.sources}>
      {sources.map((source) => (
        <li key={source.source} className={clsx(styles.chip, styles[source.state])}>
          <span className={styles.dot} aria-hidden="true" />
          <span className={styles.name}>{SOURCE_NAMES[source.source]}</span>
          <span>{describe(source, now)}</span>
        </li>
      ))}
    </ul>
  );
};

/**
 * The progress inside the search field: a counter of the sources that have answered, a line with one segment
 * per source under the field, and the timing of each source while the counter is pointed at or focused.
 */
export const SearchPulse = ({ run }: { run: SearchRun }) => {
  const now = useNow(run.finishedAt === null);
  const total = formatElapsed((run.finishedAt ?? now) - run.startedAt);
  const answered = run.sources.filter((s) => s.state !== "waiting" && s.state !== "running").length;
  const detailsId = useId();
  return (
    <>
      <div className={styles.pulse} onMouseDown={keepFocus}>
        <button type="button" className={styles.counter} aria-describedby={detailsId}>
          {run.cached ? (
            "saved"
          ) : (
            <>
              {run.sources.length > 0 && (
                <span className={styles.name}>
                  {answered}/{run.sources.length}
                </span>
              )}
              <span>{total}</span>
            </>
          )}
        </button>
        <div id={detailsId} className={styles.details} role="tooltip">
          <p className={styles.summary}>
            {run.cached ? "Saved results, nothing was requested" : `“${run.query}”`}
          </p>
          <SourceChips sources={run.sources} />
        </div>
      </div>
      <div className={styles.line} aria-hidden="true">
        {run.sources.map((source) => (
          <span key={source.source} className={clsx(styles.segment, styles[source.state])} />
        ))}
      </div>
    </>
  );
};

/** What is being searched for, for how long, and the sources behind it. */
export const SearchProgress = ({ run }: { run: SearchRun }) => {
  const now = useNow(run.finishedAt === null);
  const total = formatElapsed((run.finishedAt ?? now) - run.startedAt);
  return (
    <div className={styles.progress} onMouseDown={keepFocus}>
      <p className={clsx(styles.summary, "truncate")}>
        {run.cached
          ? `“${run.query}”: saved results, nothing was requested`
          : run.finishedAt === null
            ? `Searching “${run.query}”… ${total}`
            : `Searched “${run.query}” in ${total}`}
      </p>
      <SourceChips sources={run.sources} />
    </div>
  );
};
