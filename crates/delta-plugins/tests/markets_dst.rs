//! DST tests for the market session clocks (decision D5 / finding
//! rust-plugins #6): both markets are pinned to their IANA zones
//! (`America/New_York`, `Australia/Sydney`) and must agree with Python's
//! `zoneinfo` behaviour on both sides of each transition.
//!
//! 2026 reference points: US spring-forward Sun Mar 8 / fall-back Sun Nov 1;
//! AEDT ends Sun Apr 5, starts Sun Oct 4.

use chrono::{DateTime, NaiveDateTime, Utc};
use delta_plugins::{AsxMarket, UsMarket};

fn utc(s: &str) -> DateTime<Utc> {
    let naive: NaiveDateTime = s.parse().unwrap();
    DateTime::from_naive_utc_and_offset(naive, Utc)
}

#[test]
fn us_winter_is_est() {
    let market = UsMarket::default();
    // Wed 2026-01-07, EST (UTC-5): 09:30-16:00 local = 14:30-21:00 UTC.
    assert!(market.is_open(utc("2026-01-07T14:30:00")));
    assert!(!market.is_open(utc("2026-01-07T14:29:00")));
    assert!(market.is_open(utc("2026-01-07T21:00:00")));
    assert!(!market.is_open(utc("2026-01-07T21:01:00")));
}

#[test]
fn us_summer_is_edt() {
    let market = UsMarket::default();
    // Mon 2026-03-09, EDT (UTC-4): 09:30-16:00 local = 13:30-20:00 UTC.
    assert!(market.is_open(utc("2026-03-09T13:30:00")));
    assert!(!market.is_open(utc("2026-03-09T13:29:00")));
    assert!(market.is_open(utc("2026-03-09T20:00:00")));
    assert!(!market.is_open(utc("2026-03-09T20:01:00")));
}

#[test]
fn us_next_open_crosses_spring_forward() {
    let market = UsMarket::default();
    // Fri 2026-03-06 10:00 EST (15:00 UTC): next open is Monday's local 09:30,
    // which is EDT after the Sunday shift = 13:30 UTC.
    let next = market.next_open(utc("2026-03-06T15:00:00"));
    assert_eq!(next, utc("2026-03-09T13:30:00"));

    // Friday after the close (16:01 EST = 21:01 UTC): Monday open, EDT.
    let next = market.next_open(utc("2026-03-06T21:01:00"));
    assert_eq!(next, utc("2026-03-09T13:30:00"));
}

#[test]
fn us_next_open_crosses_fall_back() {
    let market = UsMarket::default();
    // Fri 2026-10-30 after the close (16:01 EDT = 20:01 UTC): Monday's local
    // open is EST after the Sunday shift = 14:30 UTC.
    let next = market.next_open(utc("2026-10-30T20:01:00"));
    assert_eq!(next, utc("2026-11-02T14:30:00"));
}

#[test]
fn asx_summer_is_aedt() {
    let market = AsxMarket::default();
    // Fri 2026-03-06, AEDT (UTC+11): 10:00-16:00 local = 23:00 (prev day) -
    // 05:00 UTC.
    assert!(market.is_open(utc("2026-03-05T23:00:00")));
    assert!(!market.is_open(utc("2026-03-05T22:59:00")));
    assert!(market.is_open(utc("2026-03-06T05:00:00")));
    assert!(!market.is_open(utc("2026-03-06T05:01:00")));
}

#[test]
fn asx_winter_is_aest() {
    let market = AsxMarket::default();
    // Mon 2026-04-06, AEST (UTC+10): 10:00-16:00 local = 00:00-06:00 UTC.
    assert!(market.is_open(utc("2026-04-06T00:00:00")));
    assert!(!market.is_open(utc("2026-04-05T23:59:00")));
    assert!(market.is_open(utc("2026-04-06T06:00:00")));
    assert!(!market.is_open(utc("2026-04-06T06:01:00")));
}

#[test]
fn asx_next_open_crosses_aest_start() {
    let market = AsxMarket::default();
    // Fri 2026-04-03 10:30 AEDT (2026-04-02T23:30 UTC): Saturday follows, so
    // the next session is Monday 10:00, which is AEST after the Sunday shift
    // = 2026-04-06T00:00 UTC.
    let next = market.next_open(utc("2026-04-02T23:30:00"));
    assert_eq!(next, utc("2026-04-06T00:00:00"));
}

#[test]
fn asx_next_open_crosses_aedt_start() {
    let market = AsxMarket::default();
    // Fri 2026-10-02 12:00 AEST (02:00 UTC): next session is Monday 10:00,
    // which is AEDT after the Sunday shift = 2026-10-04T23:00 UTC.
    let next = market.next_open(utc("2026-10-02T02:00:00"));
    assert_eq!(next, utc("2026-10-04T23:00:00"));
}

#[test]
fn asx_next_open_returns_same_day_before_the_open() {
    let market = AsxMarket::default();
    // Python parity (`ASXMarket.next_open`): 09:00 local Friday is still
    // ahead of the 10:00 open, so today's open is the next one.
    // Fri 2026-04-10 09:00 AEST = 2026-04-09T23:00 UTC.
    let next = market.next_open(utc("2026-04-09T23:00:00"));
    assert_eq!(next, utc("2026-04-10T00:00:00"));
}

#[test]
fn asx_next_open_at_the_open_moves_to_the_next_day() {
    let market = AsxMarket::default();
    // At exactly 10:00 local the session has begun, so Python advances a day.
    // Mon 2026-04-06 10:00 AEST = 2026-04-06T00:00 UTC -> Tue open.
    let next = market.next_open(utc("2026-04-06T00:00:00"));
    assert_eq!(next, utc("2026-04-07T00:00:00"));
}
