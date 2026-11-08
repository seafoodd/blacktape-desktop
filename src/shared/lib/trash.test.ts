import {
  describe,
  it,
  expect,
} from "vitest";
import {
  formatBytes,
  trashDaysLeft,
  formatDaysLeft,
  formatTrashedDate,
  describeTrashPolicy,
  gbToMb,
  mbToGb,
  RETENTION_OPTIONS,
  MAX_SIZE_OPTIONS,
} from "./trash.ts";

const DAY = 24 * 60 * 60;
const KB = 1024;
const MB = KB * 1024;
const GB = MB * 1024;
const TB = GB * 1024;

describe("formatBytes", () => {
  it("returns 0 B for zero", () => {
    expect(formatBytes(0)).toBe("0 B");
  });

  it("returns 0 B for negative values", () => {
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(-GB)).toBe("0 B");
  });

  it("returns 0 B for NaN and infinities", () => {
    expect(formatBytes(NaN)).toBe("0 B");
    expect(formatBytes(Infinity)).toBe("0 B");
    expect(formatBytes(-Infinity)).toBe("0 B");
  });

  it("keeps values below 1024 in bytes without decimals", () => {
    expect(formatBytes(1)).toBe("1 B");
    expect(formatBytes(1023)).toBe("1023 B");
  });

  it("switches to KB at exactly 1024", () => {
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
  });

  it("uses one decimal below 10 and none at 10 or above", () => {
    expect(formatBytes(9.5 * KB)).toBe("9.5 KB");
    expect(formatBytes(10 * KB)).toBe("10 KB");
    expect(formatBytes(500 * KB)).toBe("500 KB");
  });

  it("switches to MB and GB at their boundaries", () => {
    expect(formatBytes(MB)).toBe("1.0 MB");
    expect(formatBytes(1023 * MB)).toBe("1023 MB");
    expect(formatBytes(GB)).toBe("1.0 GB");
    expect(formatBytes(5 * GB)).toBe("5.0 GB");
    expect(formatBytes(12 * GB)).toBe("12 GB");
  });

  it("switches to TB at 1024 GB", () => {
    expect(formatBytes(TB)).toBe("1.0 TB");
  });

  it("does not go past TB, so very large values stay in TB", () => {
    expect(formatBytes(1024 * TB)).toBe("1024 TB");
  });
});

describe("formatBytes rounding", () => {
  it("carries over to the next unit when rounding reaches 1024", () => {
    expect(formatBytes(1024 * 1024 - 1)).toBe("1.0 MB");
  });
});

describe("trashDaysLeft", () => {
  const trashedAt = 1_000_000;

  it("returns null when retention is never", () => {
    expect(trashDaysLeft(trashedAt, null, trashedAt)).toBeNull();
    expect(trashDaysLeft(trashedAt, null, trashedAt + 999 * DAY)).toBeNull();
  });

  it("returns the full number of days when no time has passed", () => {
    expect(trashDaysLeft(trashedAt, 7, trashedAt)).toBe(7);
    expect(trashDaysLeft(trashedAt, 30, trashedAt)).toBe(30);
  });

  it("returns 0 exactly at the deletion time", () => {
    expect(trashDaysLeft(trashedAt, 7, trashedAt + 7 * DAY)).toBe(0);
  });

  it("returns 0 when overdue, never negative", () => {
    expect(trashDaysLeft(trashedAt, 7, trashedAt + 8 * DAY)).toBe(0);
    expect(trashDaysLeft(trashedAt, 7, trashedAt + 365 * DAY)).toBe(0);
  });

  it("rounds a partial day up", () => {
    // One second short of a day left still counts as one day.
    expect(trashDaysLeft(0, 1, 1)).toBe(1);
    // 1.5 days left rounds up to 2.
    expect(trashDaysLeft(0, 2, DAY / 2)).toBe(2);
    // One second left is one day.
    expect(trashDaysLeft(0, 1, DAY - 1)).toBe(1);
  });

  it("returns exact full days after elapsed whole days", () => {
    expect(trashDaysLeft(0, 7, 2 * DAY)).toBe(5);
    expect(trashDaysLeft(0, 7, 6 * DAY)).toBe(1);
  });

  it("returns 0 for a zero-day retention", () => {
    expect(trashDaysLeft(trashedAt, 0, trashedAt)).toBe(0);
  });
});

describe("formatDaysLeft", () => {
  it("says deletes today for 0", () => {
    expect(formatDaysLeft(0)).toBe("Deletes today");
  });

  it("says deletes today for negative values", () => {
    expect(formatDaysLeft(-3)).toBe("Deletes today");
  });

  it("uses the singular for 1 day", () => {
    expect(formatDaysLeft(1)).toBe("Deletes in 1 day");
  });

  it("uses the plural for n days", () => {
    expect(formatDaysLeft(2)).toBe("Deletes in 2 days");
    expect(formatDaysLeft(30)).toBe("Deletes in 30 days");
  });
});

describe("formatTrashedDate", () => {
  // Locale and timezone dependent, so only the year is asserted. The
  // timestamps are chosen away from new year and midnight edges.
  it("contains the year for a late-2023 timestamp", () => {
    const result = formatTrashedDate(1_700_000_000);
    expect(typeof result).toBe("string");
    expect(result).toContain("2023");
  });

  it("contains the year for a mid-year timestamp", () => {
    const result = formatTrashedDate(1_686_000_000);
    expect(result).toContain("2023");
  });
});

describe("describeTrashPolicy", () => {
  it("describes keeping items forever with no size limit", () => {
    expect(
      describeTrashPolicy({ retention_days: null, max_size_mb: null }),
    ).toBe("Items are kept until you delete them.");
  });

  it("describes an age limit with no size limit, singular", () => {
    expect(
      describeTrashPolicy({ retention_days: 1, max_size_mb: null }),
    ).toBe("Items are deleted automatically after 1 day.");
  });

  it("describes an age limit with no size limit, plural", () => {
    expect(
      describeTrashPolicy({ retention_days: 30, max_size_mb: null }),
    ).toBe("Items are deleted automatically after 30 days.");
  });

  it("describes a size limit with no age limit", () => {
    expect(
      describeTrashPolicy({ retention_days: null, max_size_mb: 1024 }),
    ).toBe(
      "Items are kept until you delete them. The Trash is limited to 1.0 GB.",
    );
  });

  it("describes both an age limit (plural) and a size limit", () => {
    expect(
      describeTrashPolicy({ retention_days: 7, max_size_mb: 2048 }),
    ).toBe(
      "Items are deleted automatically after 7 days. The Trash is limited to 2.0 GB.",
    );
  });

  it("describes both an age limit (singular) and a size limit in MB", () => {
    expect(
      describeTrashPolicy({ retention_days: 1, max_size_mb: 500 }),
    ).toBe(
      "Items are deleted automatically after 1 day. The Trash is limited to 500 MB.",
    );
  });
});

describe("gbToMb / mbToGb", () => {
  it("converts GB to MB using binary units", () => {
    expect(gbToMb(1)).toBe(1024);
    expect(gbToMb(0)).toBe(0);
    expect(gbToMb(0.5)).toBe(512);
  });

  it("converts MB to GB using binary units", () => {
    expect(mbToGb(1024)).toBe(1);
    expect(mbToGb(0)).toBe(0);
    expect(mbToGb(512)).toBe(0.5);
  });

  it("round trips GB -> MB -> GB", () => {
    for (const gb of [0, 0.5, 1, 2, 5, 10, 20, 50]) {
      expect(mbToGb(gbToMb(gb))).toBe(gb);
    }
  });

  it("round trips MB -> GB -> MB for whole GB multiples", () => {
    for (const mb of [0, 1024, 2048, 5120, 51200]) {
      expect(gbToMb(mbToGb(mb))).toBe(mb);
    }
  });
});

describe("RETENTION_OPTIONS", () => {
  it("starts with Never, which has a null day count", () => {
    expect(RETENTION_OPTIONS[0]).toEqual({ label: "Never", days: null });
  });

  it("has strictly increasing day counts after the Never option", () => {
    const days = RETENTION_OPTIONS.slice(1).map((o) => o.days as number);
    expect(days.every((d) => typeof d === "number")).toBe(true);
    for (let i = 1; i < days.length; i++) {
      expect(days[i]).toBeGreaterThan(days[i - 1]);
    }
  });

  it("only uses null for the Never option", () => {
    const nulls = RETENTION_OPTIONS.filter((o) => o.days === null);
    expect(nulls).toHaveLength(1);
  });
});

describe("MAX_SIZE_OPTIONS", () => {
  it("starts with Unlimited, which has a null size", () => {
    expect(MAX_SIZE_OPTIONS[0]).toEqual({ label: "Unlimited", gb: null });
  });

  it("has strictly increasing sizes after the Unlimited option", () => {
    const sizes = MAX_SIZE_OPTIONS.slice(1).map((o) => o.gb as number);
    expect(sizes.every((s) => typeof s === "number")).toBe(true);
    for (let i = 1; i < sizes.length; i++) {
      expect(sizes[i]).toBeGreaterThan(sizes[i - 1]);
    }
  });

  it("only uses null for the Unlimited option", () => {
    const nulls = MAX_SIZE_OPTIONS.filter((o) => o.gb === null);
    expect(nulls).toHaveLength(1);
  });
});
