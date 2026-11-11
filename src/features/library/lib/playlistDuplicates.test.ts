import { splitDuplicates } from "./playlistDuplicates.ts";
import { describe, it, expect } from "vitest";

describe("splitDuplicates", () => {
  it("puts every new id in fresh when there is no overlap", () => {
    expect(splitDuplicates([1, 2], [3, 4])).toEqual({
      fresh: [3, 4],
      duplicates: [],
    });
  });

  it("puts every new id in duplicates when all are already existing", () => {
    expect(splitDuplicates([1, 2, 3], [2, 1])).toEqual({
      fresh: [],
      duplicates: [2, 1],
    });
  });

  it("keeps input order in both outputs on partial overlap", () => {
    expect(splitDuplicates([1, 3], [2, 3, 4, 1, 5])).toEqual({
      fresh: [2, 4, 5],
      duplicates: [3, 1],
    });
  });

  it("treats everything as fresh when existing is empty", () => {
    expect(splitDuplicates([], [1, 2])).toEqual({
      fresh: [1, 2],
      duplicates: [],
    });
  });

  it("returns empty outputs when newIds is empty", () => {
    expect(splitDuplicates([1, 2], [])).toEqual({
      fresh: [],
      duplicates: [],
    });
    expect(splitDuplicates([], [])).toEqual({
      fresh: [],
      duplicates: [],
    });
  });

  it("marks every repeated copy as duplicate when the id already exists", () => {
    expect(splitDuplicates([1], [1, 1, 2, 1])).toEqual({
      fresh: [2],
      duplicates: [1, 1, 1],
    });
  });

  it("marks every repeated copy as fresh when the id does not exist yet", () => {
    expect(splitDuplicates([5], [1, 1, 2])).toEqual({
      fresh: [1, 1, 2],
      duplicates: [],
    });
  });

  it("does not mutate the input arrays", () => {
    const existingIds = [1, 3];
    const newIds = [2, 3, 4, 1];
    const existingCopy = [...existingIds];
    const newCopy = [...newIds];

    splitDuplicates(existingIds, newIds);

    expect(existingIds).toEqual(existingCopy);
    expect(newIds).toEqual(newCopy);
  });
});
