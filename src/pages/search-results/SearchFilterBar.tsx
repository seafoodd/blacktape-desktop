import { useEffect, useState } from "react";
import clsx from "clsx";
import { BiFilterAlt, BiSortDown, BiSortUp, BiX } from "react-icons/bi";
import type {
  NumberRange,
  PlatformChoice,
  SearchPageFilters,
  SearchPageSort,
  SortKey,
  TypeTab,
} from "@/features/search/model/searchPageFilters.ts";
import {
  formatDurationField,
  parseDurationInput,
  parseYearInput,
} from "./searchResultsFormat.ts";
import styles from "./search-results.module.css";

export interface FilterTab {
  value: TypeTab;
  label: string;
  /** Count text ("12", "12+") or null while the section loads. */
  count: string | null;
}

const PLATFORM_OPTIONS: { value: PlatformChoice; label: string }[] = [
  { value: "All", label: "All" },
  { value: "youtube", label: "YouTube" },
  { value: "bandcamp", label: "Bandcamp" },
];

const SORT_OPTIONS: { value: SortKey; label: string }[] = [
  { value: "relevance", label: "Relevance" },
  { value: "name", label: "Name" },
  { value: "year", label: "Year" },
  { value: "popularity", label: "Popularity" },
  { value: "duration", label: "Duration" },
];

interface SearchFilterBarProps {
  tabs: FilterTab[];
  filters: SearchPageFilters;
  sort: SearchPageSort;
  yearEnabled: boolean;
  durationEnabled: boolean;
  hasActiveFilters: boolean;
  onFiltersChange: (patch: Partial<SearchPageFilters>) => void;
  onSortChange: (sort: SearchPageSort) => void;
  /** Clears platform, year, duration and text; keeps the tab and the sort. */
  onResetFilters: () => void;
}

/** Duration bound input ("m:ss", "h:mm:ss" or minutes). Commits on blur or Enter; Escape reverts. */
const DurationField = ({
  label,
  value,
  enabled,
  onCommit,
}: {
  label: string;
  value: number | undefined;
  enabled: boolean;
  onCommit: (secs: number | undefined) => void;
}) => {
  const [draft, setDraft] = useState(() => formatDurationField(value));
  const [invalid, setInvalid] = useState(false);

  useEffect(() => {
    setDraft(formatDurationField(value));
    setInvalid(false);
  }, [value]);

  const commit = () => {
    const parsed = parseDurationInput(draft);
    if (parsed === null) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    onCommit(parsed.value);
  };

  return (
    <input
      type="text"
      className={clsx(styles.input, styles.rangeInput, invalid && styles.inputInvalid)}
      aria-label={label}
      aria-invalid={invalid}
      placeholder={label === "Duration from" ? "0:00" : "any"}
      title={
        enabled
          ? "m:ss, h:mm:ss or minutes"
          : "Duration filter applies to tracks"
      }
      value={draft}
      disabled={!enabled}
      onChange={(e) => {
        setDraft(e.target.value);
        setInvalid(false);
      }}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") {
          setDraft(formatDurationField(value));
          setInvalid(false);
        }
      }}
    />
  );
};

export const SearchFilterBar = ({
  tabs,
  filters,
  sort,
  yearEnabled,
  durationEnabled,
  hasActiveFilters,
  onFiltersChange,
  onSortChange,
  onResetFilters,
}: SearchFilterBarProps) => {
  const [rangesOpen, setRangesOpen] = useState(false);
  const rangeCount =
    (filters.year.min != null || filters.year.max != null ? 1 : 0) +
    (filters.durationSecs.min != null || filters.durationSecs.max != null ? 1 : 0);
  const setYear = (range: NumberRange) => onFiltersChange({ year: range });
  const setDuration = (range: NumberRange) => onFiltersChange({ durationSecs: range });

  return (
    <div className={styles.filterBar}>
      <div className={styles.filterRow}>
        <div className={styles.tabs} role="tablist" aria-label="Result type">
          {tabs.map((tab) => {
            const selected = filters.tab === tab.value;
            return (
              <button
                key={tab.value}
                type="button"
                role="tab"
                aria-selected={selected}
                className={clsx(styles.tab, selected && styles.tabActive)}
                onClick={() => onFiltersChange({ tab: tab.value })}
              >
                {tab.label}
                {tab.count !== null && <span className={styles.count}>{tab.count}</span>}
              </button>
            );
          })}
        </div>

        <div className={styles.filterGroupEnd} role="group" aria-label="Platform">
          {PLATFORM_OPTIONS.map((opt) => {
            const selected = filters.platform === opt.value;
            return (
              <button
                key={opt.value}
                type="button"
                aria-pressed={selected}
                className={clsx(styles.pill, selected && styles.pillActive)}
                onClick={() => onFiltersChange({ platform: opt.value })}
              >
                {opt.label}
              </button>
            );
          })}
        </div>
      </div>

      <div className={styles.filterRow}>
        <button
          type="button"
          className={clsx(styles.filtersToggle, rangesOpen && styles.filtersToggleOpen)}
          aria-expanded={rangesOpen}
          aria-controls="search-results-ranges"
          aria-label="Year and duration filters"
          title="Year and duration filters"
          onClick={() => setRangesOpen((v) => !v)}
        >
          <BiFilterAlt size={18} />
          {rangeCount > 0 && <span className={styles.filtersDot}>{rangeCount}</span>}
        </button>

        <input
          type="search"
          className={clsx(styles.input, styles.textInput)}
          placeholder="Filter results..."
          aria-label="Filter results"
          value={filters.text}
          onChange={(e) => onFiltersChange({ text: e.target.value })}
        />

        <div className={styles.filterGroupEnd}>
          <div className={styles.group}>
            <label className={styles.groupLabel} htmlFor="search-results-sort">
              Sort
            </label>
            <select
              id="search-results-sort"
              className={clsx(styles.input, styles.selectInput)}
              value={sort.key}
              onChange={(e) => onSortChange({ key: e.target.value as SortKey, dir: sort.dir })}
            >
              {SORT_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
            <button
              type="button"
              className={styles.iconBtn}
              aria-label={sort.dir === "asc" ? "Sorted ascending, switch to descending" : "Sorted descending, switch to ascending"}
              title={sort.dir === "asc" ? "Ascending" : "Descending"}
              onClick={() =>
                onSortChange({ key: sort.key, dir: sort.dir === "asc" ? "desc" : "asc" })
              }
            >
              {sort.dir === "asc" ? <BiSortUp size={18} /> : <BiSortDown size={18} />}
            </button>
          </div>

          {hasActiveFilters && (
            <button type="button" className={styles.resetBtn} onClick={onResetFilters}>
              <BiX size={16} />
              Reset
            </button>
          )}
        </div>
      </div>

      {rangesOpen && (
        <div className={styles.filterRow} id="search-results-ranges">
        <div className={styles.group} title={yearEnabled ? undefined : "Year filter applies to albums"}>
          <span className={styles.groupLabel}>Year</span>
          <input
            type="number"
            inputMode="numeric"
            className={clsx(styles.input, styles.rangeInput)}
            aria-label="Year from"
            placeholder="from"
            value={filters.year.min ?? ""}
            disabled={!yearEnabled}
            onChange={(e) =>
              setYear({ ...filters.year, min: parseYearInput(e.target.value) })
            }
          />
          <span className={styles.dash}>-</span>
          <input
            type="number"
            inputMode="numeric"
            className={clsx(styles.input, styles.rangeInput)}
            aria-label="Year to"
            placeholder="to"
            value={filters.year.max ?? ""}
            disabled={!yearEnabled}
            onChange={(e) =>
              setYear({ ...filters.year, max: parseYearInput(e.target.value) })
            }
          />
        </div>

        <div className={styles.group}>
          <span className={styles.groupLabel}>Duration</span>
          <DurationField
            label="Duration from"
            value={filters.durationSecs.min}
            enabled={durationEnabled}
            onCommit={(secs) => setDuration({ ...filters.durationSecs, min: secs })}
          />
          <span className={styles.dash}>-</span>
          <DurationField
            label="Duration to"
            value={filters.durationSecs.max}
            enabled={durationEnabled}
            onCommit={(secs) => setDuration({ ...filters.durationSecs, max: secs })}
          />
        </div>
        </div>
      )}
    </div>
  );
};
