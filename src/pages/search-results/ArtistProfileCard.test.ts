import { describe, it, expect } from "vitest";
import { profileMetaParts } from "./ArtistProfileCard.tsx";

const emptyProfile = {
  subscribers: null,
  monthly_audience: null,
  location: null,
} as const;

describe("profileMetaParts", () => {
  it("uses the profile's subscribers, monthly audience and location", () => {
    expect(
      profileMetaParts(
        { subscribers: 7_210_000, monthly_audience: 67_400_000, location: "Paris" },
        { subscriber_count: 67_400_000, audience_kind: "monthly_audience" },
      ),
    ).toEqual(["7.2M subscribers", "67.4M monthly audience", "Paris"]);
  });

  it("falls back to the search row's monthly audience, never labelled as subscribers", () => {
    const parts = profileMetaParts(
      emptyProfile,
      { subscriber_count: 67_400_000, audience_kind: "monthly_audience" },
    );
    expect(parts).toEqual(["67.4M monthly audience"]);
    expect(parts.some((p) => p.includes("subscribers"))).toBe(false);
  });

  it("falls back to the search row's subscribers when its kind says subscribers", () => {
    expect(
      profileMetaParts(emptyProfile, { subscriber_count: 6500, audience_kind: "subscribers" }),
    ).toEqual(["6,500 subscribers"]);
  });

  it("uses no row count when the audience kind is unknown", () => {
    expect(
      profileMetaParts(emptyProfile, { subscriber_count: 6500, audience_kind: null }),
    ).toEqual([]);
    expect(profileMetaParts(emptyProfile, { subscriber_count: 6500 })).toEqual([]);
  });

  it("shows the profile's followers (SoundCloud) before the location", () => {
    expect(
      profileMetaParts(
        { subscribers: null, monthly_audience: null, followers: 12_000_000, location: "Berlin, DE" },
        {},
      ),
    ).toEqual(["12M followers", "Berlin, DE"]);
  });

  it("falls back to the search row's followers when the profile has none", () => {
    expect(
      profileMetaParts(
        { subscribers: null, monthly_audience: null, followers: null, location: null },
        { subscriber_count: 5_000_000, audience_kind: "followers" },
      ),
    ).toEqual(["5M followers"]);
  });
});
