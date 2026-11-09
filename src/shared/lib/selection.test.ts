import { describe, it, expect } from "vitest";
import {
  EMPTY_SELECTION,
  nextSelection,
  pruneSelection,
  selectGroup,
  selectionForContextMenu,
  type SelectionState,
} from "./selection.ts";

const IDS = [1, 2, 3, 4, 5, 6];
const NONE = { ctrl: false, shift: false };
const CTRL = { ctrl: true, shift: false };
const SHIFT = { ctrl: false, shift: true };
const CTRL_SHIFT = { ctrl: true, shift: true };

const state = (selected: number[], anchor: number | null): SelectionState => ({
  selected: new Set(selected),
  anchor,
});

const sorted = (s: Set<number>) => [...s].sort((a, b) => a - b);

describe("EMPTY_SELECTION", () => {
  it("has no selected ids and no anchor", () => {
    expect(sorted(EMPTY_SELECTION.selected)).toEqual([]);
    expect(EMPTY_SELECTION.anchor).toBeNull();
  });

  it("is not mutated by nextSelection", () => {
    nextSelection(EMPTY_SELECTION, IDS, 1, CTRL);
    nextSelection(EMPTY_SELECTION, IDS, 1, NONE);
    expect(EMPTY_SELECTION.selected.size).toBe(0);
    expect(EMPTY_SELECTION.anchor).toBeNull();
  });
});

describe("nextSelection", () => {
  describe("plain click", () => {
    it("selects only the clicked id and sets it as the anchor", () => {
      const result = nextSelection(state([5], 5), IDS, 3, NONE);
      expect(sorted(result.selected)).toEqual([3]);
      expect(result.anchor).toBe(3);
    });

    it("replaces the previous selection when clicking another id", () => {
      const result = nextSelection(state([1, 2], 2), IDS, 4, NONE);
      expect(sorted(result.selected)).toEqual([4]);
      expect(result.anchor).toBe(4);
    });

    it("does not mutate the input state", () => {
      const input = state([1, 2], 2);
      nextSelection(input, IDS, 4, NONE);
      expect(sorted(input.selected)).toEqual([1, 2]);
      expect(input.anchor).toBe(2);
    });
  });

  describe("ctrl click", () => {
    it("adds an unselected id and moves the anchor to it", () => {
      const result = nextSelection(state([1], 1), IDS, 3, CTRL);
      expect(sorted(result.selected)).toEqual([1, 3]);
      expect(result.anchor).toBe(3);
    });

    it("removes an already selected id and moves the anchor to it", () => {
      const result = nextSelection(state([1, 3], 3), IDS, 1, CTRL);
      expect(sorted(result.selected)).toEqual([3]);
      expect(result.anchor).toBe(1);
    });

    it("does not mutate the input state", () => {
      const input = state([1, 3], 3);
      nextSelection(input, IDS, 1, CTRL);
      nextSelection(input, IDS, 5, CTRL);
      expect(sorted(input.selected)).toEqual([1, 3]);
      expect(input.anchor).toBe(3);
    });
  });

  describe("shift click", () => {
    it("selects the forward range from the anchor (anchor unchanged)", () => {
      const result = nextSelection(state([2], 2), IDS, 5, SHIFT);
      expect(sorted(result.selected)).toEqual([2, 3, 4, 5]);
      expect(result.anchor).toBe(2);
    });

    it("selects the backward range from the anchor (anchor unchanged)", () => {
      const result = nextSelection(state([5], 5), IDS, 2, SHIFT);
      expect(sorted(result.selected)).toEqual([2, 3, 4, 5]);
      expect(result.anchor).toBe(5);
    });

    it("selects a single item when shift-clicking the anchor itself", () => {
      const result = nextSelection(state([4], 4), IDS, 4, SHIFT);
      expect(sorted(result.selected)).toEqual([4]);
      expect(result.anchor).toBe(4);
    });

    it("falls back to a plain select when there is no anchor", () => {
      const result = nextSelection(state([1, 2], null), IDS, 3, SHIFT);
      expect(sorted(result.selected)).toEqual([3]);
      expect(result.anchor).toBe(3);
    });

    it("falls back to a plain select when the anchor is not in orderedIds", () => {
      const result = nextSelection(state([99], 99), IDS, 3, SHIFT);
      expect(sorted(result.selected)).toEqual([3]);
      expect(result.anchor).toBe(3);
    });

    it("falls back to a plain select when the clicked id is not in orderedIds", () => {
      const result = nextSelection(state([2], 2), IDS, 99, SHIFT);
      expect(sorted(result.selected)).toEqual([99]);
      expect(result.anchor).toBe(99);
    });

    it("does not mutate the input state", () => {
      const input = state([5], 5);
      nextSelection(input, IDS, 2, SHIFT);
      expect(sorted(input.selected)).toEqual([5]);
      expect(input.anchor).toBe(5);
    });
  });

  describe("ctrl + shift click", () => {
    it("adds the range to the existing selection", () => {
      const result = nextSelection(state([6], 2), IDS, 4, CTRL_SHIFT);
      expect(sorted(result.selected)).toEqual([2, 3, 4, 6]);
      expect(result.anchor).toBe(2);
    });

    it("does not mutate the input state", () => {
      const input = state([6], 2);
      nextSelection(input, IDS, 4, CTRL_SHIFT);
      expect(sorted(input.selected)).toEqual([6]);
      expect(input.anchor).toBe(2);
    });
  });
});

describe("selectionForContextMenu", () => {
  it("keeps the same object when the id is already selected", () => {
    const input = state([1, 2, 3], 2);
    const result = selectionForContextMenu(input, 2);
    expect(result).toBe(input);
  });

  it("replaces the selection with only the id when it is not selected", () => {
    const input = state([1, 2], 2);
    const result = selectionForContextMenu(input, 5);
    expect(sorted(result.selected)).toEqual([5]);
    expect(result.anchor).toBe(5);
  });

  it("does not mutate the input state when replacing", () => {
    const input = state([1, 2], 2);
    selectionForContextMenu(input, 5);
    expect(sorted(input.selected)).toEqual([1, 2]);
    expect(input.anchor).toBe(2);
  });
});

describe("pruneSelection", () => {
  it("removes selected ids that are not in the list", () => {
    const result = pruneSelection(state([1, 2, 9], 2), [1, 2, 3]);
    expect(sorted(result.selected)).toEqual([1, 2]);
    expect(result.anchor).toBe(2);
  });

  it("resets a missing anchor to null", () => {
    const result = pruneSelection(state([1], 9), [1, 2]);
    expect(sorted(result.selected)).toEqual([1]);
    expect(result.anchor).toBeNull();
  });

  it("returns the same object when nothing changed", () => {
    const input = state([1, 2], 1);
    expect(pruneSelection(input, [1, 2, 3])).toBe(input);
  });

  it("returns the same object when the anchor is null and nothing changed", () => {
    const input = state([1], null);
    expect(pruneSelection(input, [1])).toBe(input);
  });

  it("does not mutate the input state", () => {
    const input = state([1, 2, 9], 9);
    pruneSelection(input, [1, 2]);
    expect(sorted(input.selected)).toEqual([1, 2, 9]);
    expect(input.anchor).toBe(9);
  });
});

describe("selectGroup", () => {
  describe("plain", () => {
    it("replaces any existing selection with exactly the ids and anchors the first id", () => {
      const result = selectGroup(state([1, 5, 6], 6), [2, 3, 4], false);
      expect(sorted(result.selected)).toEqual([2, 3, 4]);
      expect(result.anchor).toBe(2);
    });

    it("returns the same state object when ids is empty", () => {
      const input = state([1, 2], 2);
      expect(selectGroup(input, [], false)).toBe(input);
    });
  });

  describe("additive", () => {
    it("adds ids to the existing selection and anchors the first id of the group", () => {
      const result = selectGroup(state([1], 1), [3, 4], true);
      expect(sorted(result.selected)).toEqual([1, 3, 4]);
      expect(result.anchor).toBe(3);
    });

    it("removes the ids when all of them are already selected, keeping the others and the previous anchor", () => {
      const result = selectGroup(state([1, 2, 3, 4], 6), [2, 3], true);
      expect(sorted(result.selected)).toEqual([1, 4]);
      expect(result.anchor).toBe(6);
    });

    it("adds only the missing ids when some are already selected, without removing any", () => {
      const result = selectGroup(state([2], 2), [1, 2, 3], true);
      expect(sorted(result.selected)).toEqual([1, 2, 3]);
      expect(result.anchor).toBe(1);
    });

    it("adds the ids to an empty selection", () => {
      const result = selectGroup(EMPTY_SELECTION, [4, 5], true);
      expect(sorted(result.selected)).toEqual([4, 5]);
      expect(result.anchor).toBe(4);
    });
  });

  it("does not mutate the input state", () => {
    const input = state([1, 2], 2);
    selectGroup(input, [3, 4], false);
    selectGroup(input, [3, 4], true);
    selectGroup(input, [1, 2], true);
    expect(sorted(input.selected)).toEqual([1, 2]);
    expect(input.anchor).toBe(2);
  });

  it("produces the same selected set regardless of the order of ids", () => {
    const forward = selectGroup(state([1], 1), [2, 3, 4], true);
    const reversed = selectGroup(state([1], 1), [4, 3, 2], true);
    expect(sorted(forward.selected)).toEqual(sorted(reversed.selected));

    const plainForward = selectGroup(state([], null), [2, 3, 4], false);
    const plainReversed = selectGroup(state([], null), [4, 3, 2], false);
    expect(sorted(plainForward.selected)).toEqual(sorted(plainReversed.selected));
  });
});
