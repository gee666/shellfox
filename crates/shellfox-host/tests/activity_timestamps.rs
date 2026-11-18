use shellfox_host::{activity_unix_ms, activity_utc_timestamp, ErrorCode};
#[test]
fn activity_times_round_trip_as_canonical_utc_milliseconds() {
    for (at, expected) in [
        (0, "1970-01-01T00:00:00.000Z"),
        (1, "1970-01-01T00:00:00.001Z"),
        (1234, "1970-01-01T00:00:01.234Z"),
        (946_684_800_000, "2000-01-01T00:00:00.000Z"),
        (253_402_300_799_999, "9999-12-31T23:59:59.999Z"),
    ] {
        assert_eq!(activity_utc_timestamp(at).unwrap(), expected);
        assert_eq!(activity_unix_ms(expected).unwrap(), at);
    }
}
#[test]
fn accepted_precision_is_formatted_with_exactly_three_millisecond_digits() {
    for (raw, expected) in [
        ("1970-01-01T00:00:01Z", "1970-01-01T00:00:01.000Z"),
        ("1970-01-01T00:00:01.2Z", "1970-01-01T00:00:01.200Z"),
        ("1970-01-01T00:00:01.234567890Z", "1970-01-01T00:00:01.234Z"),
    ] {
        assert_eq!(
            activity_utc_timestamp(activity_unix_ms(raw).unwrap()).unwrap(),
            expected
        );
    }
}
#[test]
fn utc_validation_and_pre_epoch_checked_conversion_reject_invalid_unsigned_times() {
    for raw in [
        "",
        "invalid",
        "1970-01-01T00:00:00+00:00",
        "1970-01-01T00:00:00+02:00",
        "1969-12-31T23:59:59.999Z",
    ] {
        assert_eq!(
            activity_unix_ms(raw).unwrap_err().code,
            ErrorCode::Validation,
            "{raw}"
        );
    }
}
#[test]
fn integer_overflow_and_unsupported_calendar_range_are_errors_not_wrapping_or_panics() {
    for millis in [
        u64::MAX,
        u64::try_from(i64::MAX).unwrap(),
        253_402_300_800_000,
    ] {
        assert_eq!(
            activity_utc_timestamp(millis).unwrap_err().code,
            ErrorCode::Validation
        );
    }
}
