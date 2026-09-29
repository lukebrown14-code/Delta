//! Datetime helpers. SQLite hands back naive datetimes; treat them as UTC.
//!
//! Port of `delta/core/time.py`. All persistence uses naive `NaiveDateTime`
//! values that are UTC by convention, matching the Python app.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

/// Return `ts` as an aware UTC datetime. Naive input is assumed to be UTC.
pub fn to_utc(ts: NaiveDateTime) -> DateTime<Utc> {
    DateTime::from_naive_utc_and_offset(ts, Utc)
}

/// `YYYY-MM-DD` -> naive UTC midnight (compare with Python's aware value via
/// [`to_utc`]).
pub fn parse_date(text: &str) -> Option<NaiveDateTime> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_date_midnight() {
        let ts = parse_date("2026-09-23").unwrap();
        assert_eq!(ts.and_utc().timestamp(), to_utc(ts).timestamp());
        assert_eq!(
            ts,
            NaiveDate::from_ymd_opt(2026, 9, 23)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
        );
    }

    #[test]
    fn bad_date_is_none() {
        assert!(parse_date("23/09/2026").is_none());
    }
}
