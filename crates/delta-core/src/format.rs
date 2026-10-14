//! Python-`format()`-compatible number rendering shared by the watchlist
//! metrics and the brief (`{:,.2f}` thousands grouping).

/// `{:,.Nf}`: `decimals` places with comma-grouped thousands. Groups only the
/// integer part; the sign moves in front of the grouping.
pub fn grouped(value: f64, decimals: usize) -> String {
    let fixed = format!("{value:.decimals$}");
    let (int_part, frac) = fixed.split_once('.').unwrap_or((fixed.as_str(), ""));
    let negative = int_part.starts_with('-');
    let digits = int_part.trim_start_matches('-');
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + decimals + 2);
    if negative {
        out.push('-');
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if !frac.is_empty() {
        out.push('.');
        out.push_str(frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::grouped;

    #[test]
    fn matches_python_grouping() {
        assert_eq!(grouped(265_600_000_000.0, 2), "265,600,000,000.00");
        assert_eq!(grouped(12_345.678, 2), "12,345.68");
        assert_eq!(grouped(265.6, 2), "265.60");
        assert_eq!(grouped(-1234.5, 2), "-1,234.50");
        assert_eq!(grouped(3400.0, 0), "3,400");
        assert_eq!(grouped(7.46, 2), "7.46");
    }
}
