//! Stable content IDs for rows without a natural key.
//!
//! Port of `delta/core/ids.py`. The hash contract is part of the stored data
//! format: it must stay byte-compatible with the Python implementation.

use sha2::{Digest, Sha256};

/// sha256 over the parts joined with NUL so `("ab","c")` and `("a","bc")` differ.
///
/// Parts are rendered with Python `str()` semantics for the types we feed it
/// (strings, integers, floats). Floats format like Python's `str()`, i.e.
/// shortest round-trip repr, no trailing `.0` stripped.
pub fn stable_id(parts: &[&str]) -> String {
    let joined = parts.join("\u{0}");
    hex(&Sha256::digest(joined.as_bytes()))
}

/// `market="US", symbol="AAPL"` -> `"US:AAPL"` — one source of truth.
pub fn make_instrument_id(market: &str, symbol: &str) -> String {
    format!("{market}:{symbol}")
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_matches_python() {
        // sha256("us\0AAPL\02026-01-01") computed with Python.
        assert_eq!(
            stable_id(&["us", "AAPL", "2026-01-01"]),
            "f2005afa98dfb3b24984883a69925acafc9c02b478527a46f59d1946f332eb3c"
        );
    }

    #[test]
    fn nul_join_is_unambiguous() {
        assert_ne!(stable_id(&["ab", "c"]), stable_id(&["a", "bc"]));
    }

    #[test]
    fn instrument_id_format() {
        assert_eq!(make_instrument_id("US", "AAPL"), "US:AAPL");
    }
}
