//! Pure helpers for reordering playlist rows.

/// New row order after moving a block of rows.
///
/// `selected` are 0-based indices of the rows to move (any order, duplicates and
/// out-of-range values ignored); `insert_at` is the 0-based index in the ORIGINAL list
/// the block is dropped before (`len` = after the last row). The block keeps its
/// internal order. Returns the old index of every row in its new place.
pub fn move_block_order(len: usize, selected: &[usize], insert_at: usize) -> Vec<usize> {
    let mut block: Vec<usize> = selected.iter().copied().filter(|&i| i < len).collect();
    block.sort_unstable();
    block.dedup();

    let insert_at = insert_at.min(len);
    // Rows before the drop point that are leaving shift the drop point left.
    let adjusted = insert_at - block.iter().filter(|&&i| i < insert_at).count();

    let mut remaining: Vec<usize> = (0..len).filter(|i| block.binary_search(i).is_err()).collect();
    remaining.splice(adjusted..adjusted, block);
    remaining
}

#[cfg(test)]
mod tests {
    use super::move_block_order;

    fn identity(len: usize) -> Vec<usize> {
        (0..len).collect()
    }

    #[test]
    fn single_row_moved_down() {
        // [A B C D E]: move B before E -> [A C D B E]
        assert_eq!(move_block_order(5, &[1], 4), vec![0, 2, 3, 1, 4]);
    }

    #[test]
    fn single_row_moved_up() {
        // [A B C D E]: move D before B -> [A D B C E]
        assert_eq!(move_block_order(5, &[3], 1), vec![0, 3, 1, 2, 4]);
    }

    #[test]
    fn single_row_moved_to_end() {
        // move A to the end -> [B C D E A]
        assert_eq!(move_block_order(5, &[0], 5), vec![1, 2, 3, 4, 0]);
    }

    #[test]
    fn block_of_two_moved_down() {
        // move B,C before E -> [A D B C E]
        assert_eq!(move_block_order(5, &[1, 2], 4), vec![0, 3, 1, 2, 4]);
    }

    #[test]
    fn block_of_two_moved_up() {
        // move D,E before B -> [A D E B C]
        assert_eq!(move_block_order(5, &[3, 4], 1), vec![0, 3, 4, 1, 2]);
    }

    #[test]
    fn non_contiguous_selection_dropped_at_start() {
        // move B,D before A -> [B D A C E]
        assert_eq!(move_block_order(5, &[1, 3], 0), vec![1, 3, 0, 2, 4]);
    }

    #[test]
    fn non_contiguous_selection_dropped_at_end() {
        // move B,D to the end -> [A C E B D]
        assert_eq!(move_block_order(5, &[1, 3], 5), vec![0, 2, 4, 1, 3]);
    }

    #[test]
    fn drop_inside_block_span_keeps_order() {
        for insert_at in 1..=3 {
            assert_eq!(move_block_order(5, &[1, 2], insert_at), identity(5));
        }
    }

    #[test]
    fn empty_selection_is_identity() {
        assert_eq!(move_block_order(5, &[], 0), identity(5));
        assert_eq!(move_block_order(5, &[], 2), identity(5));
        assert_eq!(move_block_order(5, &[], 5), identity(5));
    }

    #[test]
    fn duplicates_and_out_of_range_are_ignored() {
        // block is {1, 4}; move before A -> [B E A C D]
        assert_eq!(move_block_order(5, &[4, 4, 1, 99], 0), vec![1, 4, 0, 2, 3]);
    }

    #[test]
    fn insert_at_past_len_clamps_to_end() {
        assert_eq!(move_block_order(5, &[0], 100), move_block_order(5, &[0], 5));
        assert_eq!(move_block_order(5, &[0], 100), vec![1, 2, 3, 4, 0]);
    }

    #[test]
    fn all_rows_selected_is_identity() {
        assert_eq!(move_block_order(5, &[0, 1, 2, 3, 4], 2), identity(5));
    }

    #[test]
    fn empty_list_returns_empty() {
        assert_eq!(move_block_order(0, &[], 0), Vec::<usize>::new());
        assert_eq!(move_block_order(0, &[0], 0), Vec::<usize>::new());
    }

    #[test]
    fn result_is_always_a_permutation_with_block_contiguous_and_ordered() {
        for len in 0..=7usize {
            for mask in 0u32..(1u32 << len) {
                let selected: Vec<usize> = (0..len).filter(|&i| (mask & (1u32 << i)) != 0).collect();
                for insert_at in 0..=len + 1 {
                    let order = move_block_order(len, &selected, insert_at);

                    let mut sorted = order.clone();
                    sorted.sort_unstable();
                    assert_eq!(sorted, identity(len), "len={len} sel={selected:?} at={insert_at}");

                    if selected.is_empty() {
                        assert_eq!(order, identity(len));
                        continue;
                    }

                    // Selected rows form one contiguous run, in their original relative order.
                    let slots: Vec<usize> =
                        (0..len).filter(|&k| selected.contains(&order[k])).collect();
                    let values: Vec<usize> = slots.iter().map(|&k| order[k]).collect();
                    assert_eq!(values, selected, "len={len} sel={selected:?} at={insert_at}");
                    assert_eq!(
                        slots.last().unwrap() - slots.first().unwrap() + 1,
                        slots.len(),
                        "block not contiguous: len={len} sel={selected:?} at={insert_at}"
                    );
                }
            }
        }
    }
}
