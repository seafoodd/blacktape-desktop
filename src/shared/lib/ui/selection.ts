/** Multi-select helpers for lists (click, Ctrl/Cmd+click, Shift+click). Pure, no React. */

export interface SelectionState {
  selected: Set<number>;
  /** The item a Shift+click range starts from. */
  anchor: number | null;
}

export const EMPTY_SELECTION: SelectionState = {
  selected: new Set(),
  anchor: null,
};

export interface ClickModifiers {
  ctrl: boolean;
  shift: boolean;
}

/**
 * Selection after clicking `id`. `orderedIds` is the list in display order (needed
 * for Shift ranges).
 * - plain click: only `id`
 * - Ctrl/Cmd: toggles `id`, which becomes the anchor
 * - Shift: range from the anchor to `id` (replaces the selection, or is added to it
 *   with Ctrl)
 */
export function nextSelection(
  state: SelectionState,
  orderedIds: number[],
  id: number,
  { ctrl, shift }: ClickModifiers,
): SelectionState {
  const anchorIndex =
    state.anchor === null ? -1 : orderedIds.indexOf(state.anchor);
  const clickedIndex = orderedIds.indexOf(id);

  if (shift && anchorIndex !== -1 && clickedIndex !== -1) {
    const [from, to] =
      anchorIndex < clickedIndex
        ? [anchorIndex, clickedIndex]
        : [clickedIndex, anchorIndex];
    const range = orderedIds.slice(from, to + 1);
    const selected = new Set(ctrl ? state.selected : []);
    range.forEach((rangeId) => selected.add(rangeId));
    return { selected, anchor: state.anchor };
  }

  if (ctrl) {
    const selected = new Set(state.selected);
    if (selected.has(id)) selected.delete(id);
    else selected.add(id);
    return { selected, anchor: id };
  }

  return { selected: new Set([id]), anchor: id };
}

/**
 * Selection for a right click on `id`: keeps the current selection when `id` is part
 * of it, otherwise selects only `id`.
 */
export function selectionForContextMenu(
  state: SelectionState,
  id: number,
): SelectionState {
  if (state.selected.has(id)) return state;
  return { selected: new Set([id]), anchor: id };
}

/** Drops ids that are no longer in the list (e.g. after a delete or refresh). */
export function pruneSelection(
  state: SelectionState,
  orderedIds: number[],
): SelectionState {
  const present = new Set(orderedIds);
  const selected = new Set([...state.selected].filter((id) => present.has(id)));
  const anchor =
    state.anchor !== null && present.has(state.anchor) ? state.anchor : null;
  if (selected.size === state.selected.size && anchor === state.anchor) {
    return state;
  }
  return { selected, anchor };
}

/**
 * Selects a whole group of items at once (e.g. all tracks of an album).
 * - plain: the selection becomes exactly `ids`
 * - additive (Ctrl/Cmd): adds `ids`, or removes them when they are all selected already
 */
export function selectGroup(
  state: SelectionState,
  ids: number[],
  additive: boolean,
): SelectionState {
  if (ids.length === 0) return state;
  if (!additive) return { selected: new Set(ids), anchor: ids[0] };

  const allSelected = ids.every((id) => state.selected.has(id));
  const selected = new Set(state.selected);
  for (const id of ids) {
    if (allSelected) selected.delete(id);
    else selected.add(id);
  }
  return { selected, anchor: allSelected ? state.anchor : ids[0] };
}
