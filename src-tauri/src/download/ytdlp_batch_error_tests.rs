use super::empty_batch_error;

#[test]
fn empty_batch_error_preserves_real_failures_and_handles_empty_or_cancelled_batches() {
    assert_eq!(empty_batch_error(true, &[]), "Task cancelled");
    assert_eq!(empty_batch_error(false, &[]), "No tracks could be downloaded.");
    assert_eq!(
        empty_batch_error(false, &["network failed".into(), "save failed".into()]),
        "network failed; save failed"
    );
}
