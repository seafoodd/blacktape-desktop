//! Pure helpers for editing the play queue (no player state, easy to test).
//!
//! The player keeps `queue` (songs) and `play_order` (indices into `queue`, in the
//! order they play) plus a `cursor` (position in `play_order` of the current song).
//! The UI addresses songs by position in the play order.

use crate::playlists::ordering::move_block_order;

/// Result of removing songs from the play order.
#[derive(Debug, PartialEq, Eq)]
pub struct Removal {
    /// New play order, renumbered to index the compacted queue.
    pub order: Vec<usize>,
    /// Old `queue` indices that survive, ascending; the new queue is `queue[keep]`.
    pub keep: Vec<usize>,
    /// New cursor. When the current song was removed this is the song that followed
    /// it (None if there is none left); otherwise it still points at the same song.
    pub cursor: Option<usize>,
    pub removed_current: bool,
}

/// Removes the songs at `positions` (positions in `order`; out-of-range and duplicate
/// values are ignored).
pub fn remove_positions(order: &[usize], cursor: Option<usize>, positions: &[usize]) -> Removal {
    let mut removed: Vec<usize> = positions.iter().copied().filter(|&p| p < order.len()).collect();
    removed.sort_unstable();
    removed.dedup();

    let survivors: Vec<usize> = (0..order.len())
        .filter(|p| removed.binary_search(p).is_err())
        .map(|p| order[p])
        .collect();

    let mut keep = survivors.clone();
    keep.sort_unstable();
    let new_order: Vec<usize> = survivors
        .iter()
        .map(|src| keep.binary_search(src).unwrap_or(0))
        .collect();

    let removed_current = cursor.is_some_and(|c| removed.binary_search(&c).is_ok());
    let new_cursor = cursor.and_then(|c| {
        let before = removed.iter().filter(|&&p| p < c).count();
        let pos = c - before;
        (pos < new_order.len()).then_some(pos)
    });

    Removal {
        order: new_order,
        keep,
        cursor: new_cursor,
        removed_current,
    }
}

/// Moves the songs at `positions` as a block to just before position `insert_before`
/// (an index in the CURRENT order, `len` = the end). Returns the new order and where the
/// cursor (the current song) ended up.
pub fn move_positions(
    order: &[usize],
    cursor: Option<usize>,
    positions: &[usize],
    insert_before: usize,
) -> (Vec<usize>, Option<usize>) {
    let perm = move_block_order(order.len(), positions, insert_before);
    let new_order: Vec<usize> = perm.iter().map(|&old| order[old]).collect();
    let new_cursor = cursor.and_then(|c| perm.iter().position(|&old| old == c));
    (new_order, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::{move_positions, remove_positions};

    /// Deterministic permutation of 0..len that is not the identity for len >= 2.
    fn shuffled(len: usize) -> Vec<usize> {
        let mut v: Vec<usize> = (0..len).collect();
        v.reverse();
        if len > 1 {
            v.rotate_left(1);
        }
        v
    }

    fn sorted(mut v: Vec<usize>) -> Vec<usize> {
        v.sort_unstable();
        v
    }

    fn positions_of_mask(len: usize, mask: u32) -> Vec<usize> {
        (0..len).filter(|&p| (mask & (1u32 << p)) != 0).collect()
    }

    fn cursors_for(len: usize) -> Vec<Option<usize>> {
        std::iter::once(None).chain((0..len).map(Some)).collect()
    }

    // ---- remove_positions ----

    #[test]
    fn remove_non_current_before_cursor_shifts_cursor_left() {
        // play order [2,0,3,1], current = position 2 (song 3); remove position 0 (song 2).
        let r = remove_positions(&[2, 0, 3, 1], Some(2), &[0]);
        assert_eq!(r.order, vec![0, 2, 1]);
        assert_eq!(r.keep, vec![0, 1, 3]);
        assert_eq!(r.cursor, Some(1));
        assert!(!r.removed_current);
    }

    #[test]
    fn remove_non_current_after_cursor_keeps_cursor() {
        // play order [2,0,3,1], current = position 1 (song 0); remove position 3 (song 1).
        let r = remove_positions(&[2, 0, 3, 1], Some(1), &[3]);
        assert_eq!(r.order, vec![1, 0, 2]);
        assert_eq!(r.keep, vec![0, 2, 3]);
        assert_eq!(r.cursor, Some(1));
        assert!(!r.removed_current);
    }

    #[test]
    fn remove_current_in_middle_moves_cursor_to_next_song() {
        // current = position 2 (song 3), followed by song 1.
        let r = remove_positions(&[2, 0, 3, 1], Some(2), &[2]);
        assert_eq!(r.order, vec![2, 0, 1]);
        assert_eq!(r.keep, vec![0, 1, 2]);
        assert_eq!(r.cursor, Some(2));
        assert_eq!(r.keep[r.order[2]], 1);
        assert!(r.removed_current);
    }

    #[test]
    fn remove_current_when_last_leaves_cursor_none() {
        // current = position 3 (song 1), the last in play order.
        let r = remove_positions(&[2, 0, 3, 1], Some(3), &[3]);
        assert_eq!(r.order, vec![1, 0, 2]);
        assert_eq!(r.keep, vec![0, 2, 3]);
        assert_eq!(r.cursor, None);
        assert!(r.removed_current);
    }

    #[test]
    fn remove_all_songs_empties_everything() {
        let r = remove_positions(&[2, 0, 3, 1], Some(1), &[0, 1, 2, 3]);
        assert!(r.order.is_empty());
        assert!(r.keep.is_empty());
        assert_eq!(r.cursor, None);
        assert!(r.removed_current);
    }

    #[test]
    fn duplicate_and_out_of_range_positions_are_ignored() {
        // Only position 1 (song 0) is removed; cursor at position 2 (song 3) shifts to 1.
        let r = remove_positions(&[2, 0, 3, 1], Some(2), &[1, 1, 1, 99, 100]);
        assert_eq!(r.order, vec![1, 2, 0]);
        assert_eq!(r.keep, vec![1, 2, 3]);
        assert_eq!(r.cursor, Some(1));
        assert!(!r.removed_current);
    }

    #[test]
    fn empty_positions_change_nothing() {
        let r = remove_positions(&[2, 0, 3, 1], Some(2), &[]);
        assert_eq!(r.order, vec![2, 0, 3, 1]);
        assert_eq!(r.keep, vec![0, 1, 2, 3]);
        assert_eq!(r.cursor, Some(2));
        assert!(!r.removed_current);
    }

    #[test]
    fn cursor_none_stays_none() {
        let r = remove_positions(&[2, 0, 3, 1], None, &[0]);
        assert_eq!(r.order, vec![0, 2, 1]);
        assert_eq!(r.keep, vec![0, 1, 3]);
        assert_eq!(r.cursor, None);
        assert!(!r.removed_current);
    }

    #[test]
    fn shuffled_removal_keeps_survivors_in_play_order() {
        // order [4,3,2,1,0,5]; remove positions 0 (song 4) and 3 (song 1).
        let order = shuffled(6);
        assert_eq!(order, vec![4, 3, 2, 1, 0, 5]);
        let r = remove_positions(&order, Some(2), &[0, 3]);
        assert_eq!(r.keep, vec![0, 2, 3, 5]);
        assert_eq!(r.order, vec![2, 1, 0, 3]);
        let played: Vec<usize> = r.order.iter().map(|&k| r.keep[k]).collect();
        assert_eq!(played, vec![3, 2, 0, 5]);
        assert_eq!(r.cursor, Some(1));
    }

    #[test]
    fn remove_exhaustive_invariants() {
        for len in 0..=6usize {
            let order = shuffled(len);
            for mask in 0u32..(1u32 << len) {
                let positions = positions_of_mask(len, mask);
                let survivors: Vec<usize> = (0..len)
                    .filter(|&p| (mask & (1u32 << p)) == 0)
                    .map(|p| order[p])
                    .collect();

                for cursor in cursors_for(len) {
                    let r = remove_positions(&order, cursor, &positions);
                    let ctx = format!("len={len} order={order:?} mask={mask:b} cursor={cursor:?}");

                    assert_eq!(sorted(r.order.clone()), (0..r.keep.len()).collect::<Vec<_>>(), "{ctx}");
                    assert!(r.keep.windows(2).all(|w| w[0] < w[1]), "{ctx}");
                    assert_eq!(r.keep, sorted(survivors.clone()), "{ctx}");
                    let played: Vec<usize> = r.order.iter().map(|&k| r.keep[k]).collect();
                    assert_eq!(played, survivors, "{ctx}");

                    let cursor_removed = cursor.is_some_and(|c| (mask & (1u32 << c)) != 0);
                    assert_eq!(r.removed_current, cursor_removed, "{ctx}");

                    match cursor {
                        None => assert_eq!(r.cursor, None, "{ctx}"),
                        Some(c) if cursor_removed => {
                            // Cursor lands on the song that followed the removed one.
                            let next_song =
                                ((c + 1)..len).find(|&p| (mask & (1u32 << p)) == 0).map(|p| order[p]);
                            match (r.cursor, next_song) {
                                (None, None) => {}
                                (Some(k), Some(song)) => assert_eq!(r.keep[r.order[k]], song, "{ctx}"),
                                other => panic!("{ctx}: {other:?}"),
                            }
                        }
                        Some(c) => {
                            let k = r.cursor.unwrap_or_else(|| panic!("{ctx}"));
                            assert_eq!(r.keep[r.order[k]], order[c], "{ctx}");
                        }
                    }
                }
            }
        }
    }

    // ---- move_positions ----

    #[test]
    fn move_block_before_cursor_to_end_keeps_cursor_on_same_song() {
        // play order [2,0,3,1], current = position 2 (song 3).
        // Move position 0 (song 2) to the end -> [0,3,1,2]; song 3 is now at position 1.
        let (order, cursor) = move_positions(&[2, 0, 3, 1], Some(2), &[0], 4);
        assert_eq!(order, vec![0, 3, 1, 2]);
        assert_eq!(cursor, Some(1));
    }

    #[test]
    fn move_block_after_cursor_to_front_shifts_cursor_right() {
        // current = position 0 (song 2). Move positions 2,3 (songs 3,1) to the front.
        let (order, cursor) = move_positions(&[2, 0, 3, 1], Some(0), &[2, 3], 0);
        assert_eq!(order, vec![3, 1, 2, 0]);
        assert_eq!(cursor, Some(2));
        assert_eq!(order[cursor.unwrap()], 2);
    }

    #[test]
    fn move_current_song_to_front_cursor_follows() {
        let (order, cursor) = move_positions(&[2, 0, 3, 1], Some(2), &[2], 0);
        assert_eq!(order, vec![3, 2, 0, 1]);
        assert_eq!(cursor, Some(0));
    }

    #[test]
    fn move_current_song_to_end_cursor_follows() {
        let (order, cursor) = move_positions(&[2, 0, 3, 1], Some(2), &[2], 4);
        assert_eq!(order, vec![2, 0, 1, 3]);
        assert_eq!(cursor, Some(3));
    }

    #[test]
    fn move_other_song_across_current_changes_cursor_index_not_song() {
        // current = position 1 (song 0). Move position 3 (song 1) in front of it.
        let (order, cursor) = move_positions(&[2, 0, 3, 1], Some(1), &[3], 0);
        assert_eq!(order, vec![1, 2, 0, 3]);
        assert_eq!(cursor, Some(2));
        assert_eq!(order[cursor.unwrap()], 0);
    }

    #[test]
    fn move_other_song_behind_current_moves_cursor_left() {
        // current = position 2 (song 3). Move position 0 (song 2) behind it.
        let (order, cursor) = move_positions(&[2, 0, 3, 1], Some(2), &[0], 4);
        assert_eq!(order[cursor.unwrap()], 3);
        assert_eq!(cursor, Some(1));
    }

    #[test]
    fn move_with_cursor_none_stays_none() {
        let (order, cursor) = move_positions(&[2, 0, 3, 1], None, &[0], 4);
        assert_eq!(order, vec![0, 3, 1, 2]);
        assert_eq!(cursor, None);
    }

    #[test]
    fn move_insert_past_len_clamps_to_end() {
        let clamped = move_positions(&[2, 0, 3, 1], Some(2), &[0], 100);
        assert_eq!(clamped, move_positions(&[2, 0, 3, 1], Some(2), &[0], 4));
        assert_eq!(clamped, (vec![0, 3, 1, 2], Some(1)));
    }

    #[test]
    fn move_empty_positions_change_nothing() {
        for insert_before in [0, 2, 4, 7] {
            assert_eq!(
                move_positions(&[2, 0, 3, 1], Some(2), &[], insert_before),
                (vec![2, 0, 3, 1], Some(2))
            );
        }
    }

    #[test]
    fn move_exhaustive_invariants() {
        for len in 0..=6usize {
            let order = shuffled(len);
            for mask in 0u32..(1u32 << len) {
                let positions = positions_of_mask(len, mask);
                for insert_before in 0..=len + 1 {
                    for cursor in cursors_for(len) {
                        let (new_order, new_cursor) =
                            move_positions(&order, cursor, &positions, insert_before);
                        let ctx = format!(
                            "len={len} order={order:?} sel={positions:?} at={insert_before} cursor={cursor:?}"
                        );

                        assert_eq!(sorted(new_order.clone()), sorted(order.clone()), "{ctx}");
                        if positions.is_empty() {
                            assert_eq!(new_order, order, "{ctx}");
                        }
                        match cursor {
                            None => assert_eq!(new_cursor, None, "{ctx}"),
                            Some(c) => {
                                let k = new_cursor.unwrap_or_else(|| panic!("{ctx}"));
                                assert_eq!(new_order[k], order[c], "{ctx}");
                            }
                        }
                    }
                }
            }
        }
    }
}
