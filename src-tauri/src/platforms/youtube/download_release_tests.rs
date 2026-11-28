use super::*;

#[test]
fn parses_two_and_three_component_durations() {
    assert_eq!(parse_duration_ms_str("3:07"), Some(187_000));
    assert_eq!(parse_duration_ms_str("1:02:03"), Some(3_723_000));
}

#[test]
fn rejects_zero_and_malformed_duration_components() {
    for input in ["0:00", "1", "1:2:3:4", "x:10", "1:x", "x:10:00"] {
        assert_eq!(parse_duration_ms_str(input), None, "input: {input}");
    }
}

#[test]
fn rejects_overflow_in_duration_components_and_milliseconds() {
    for input in [
        "18446744073709551615:00",
        "1:18446744073709551615",
        "18446744073709551615:00:00",
        "18446744073709551:00",
    ] {
        assert_eq!(parse_duration_ms_str(input), None, "input: {input}");
    }
}
