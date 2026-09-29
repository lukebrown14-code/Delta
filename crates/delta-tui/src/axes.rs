//! Pure helpers for chart axes: Y tick placement and X/Y label formatting.
//! Port of `delta/tui/axes.py`. No rendering, no I/O — chart widgets call
//! these on the UI thread and tests run offline.

use chrono::{DateTime, Datelike, Duration, NaiveDateTime, Timelike};

/// Heckbert's nice step multipliers; 2.5 keeps steps like 0.25 and 2,500 available.
const NICE_STEPS: [f64; 5] = [1.0, 2.0, 2.5, 5.0, 10.0];

/// Label format switches: past 36h a clock time is noise, past ~18 months the
/// day number is too small to read on a chart that wide.
const HOURS_36: Duration = Duration::hours(36);
const MONTHS_18: Duration = Duration::days(548);

/// Smallest multiple of `step` >= `value`; the round() absorbs FP dust so a
/// mathematically exact grid point does not jump a whole step.
fn ceil_to(value: f64, step: f64) -> f64 {
    (value / step).round_to_precision(9).ceil() * step
}

trait RoundTo {
    fn round_to_precision(self, digits: i32) -> f64;
}
impl RoundTo for f64 {
    fn round_to_precision(self, digits: i32) -> f64 {
        let factor = 10f64.powi(digits);
        (self * factor).round() / factor
    }
}

/// Tick values for a Y axis: 1/2/2.5/5 x 10^n steps covering `[low, high]`.
///
/// Returns exactly `count` ticks, first <= low, last >= high, ascending. Ticks
/// sit on one step grid so labels read as round numbers; the grid is anchored
/// at the top (last tick >= high), so the step must leave enough slack that
/// rounding the top up does not push the first tick past `low`. A flat series
/// has no span to step over, so every tick is the flat value.
pub fn nice_ticks(low: f64, high: f64, count: usize) -> Vec<f64> {
    assert!(count >= 2, "nice_ticks needs at least two ticks");
    if (low - high).abs() == 0.0 {
        return vec![low; count];
    }
    let (low, high) = if low > high { (high, low) } else { (low, high) }; // defensive; callers pass min()/max()
    let raw_step = (high - low) / (count as f64 - 1.0);
    let magnitude = 10f64.powf(raw_step.log10().floor());
    for base in [magnitude, magnitude * 10.0] {
        for multiplier in NICE_STEPS {
            let step = multiplier * base;
            if step < raw_step {
                continue;
            }
            let first = ceil_to(high, step) - (count as f64 - 1.0) * step;
            if first <= low {
                return (0..count)
                    .map(|i| (first + i as f64 * step).round_to_precision(10))
                    .collect();
            }
        }
    }
    // No grid point lands both ends inside the range: split linearly so the
    // coverage contract still holds.
    let step = (high - low) / (count as f64 - 1.0);
    (0..count)
        .map(|i| (low + i as f64 * step).round_to_precision(10))
        .collect()
}

/// The `[first tick, last tick]` bounds a plot is scaled to (K2).
///
/// Scaling the chart to the outer `nice_ticks` bounds — rather than the raw
/// data min/max — means every tick lands exactly on a dot row rather than
/// between rows or off the edge. Returns `(plot_low, plot_high)`.
pub fn plot_scale(low: f64, high: f64, count: usize) -> (f64, f64) {
    let ticks = nice_ticks(low, high, count);
    (ticks[0], ticks[ticks.len() - 1])
}

/// Python `f"{value:,.2f}"`: thousands-separated, two decimals.
pub fn format_fixed(value: f64) -> String {
    let s = format!("{value:.2}");
    let (sign, digits) = if let Some(rest) = s.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", s.as_str())
    };
    let (int_part, frac_part) = digits.split_once('.').unwrap_or((digits, ""));
    let mut grouped = String::new();
    for (i, ch) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}{grouped}.{frac_part}")
}

/// Format one axis label.
///
/// `kind="price"` gets thousands separators (1,842.50); a yield never does,
/// because a separator reads like a thousands-scaled rate. Currency rides
/// after a space only when given, so the default label stays column-compact.
pub fn format_price(value: f64, kind: &str, currency: &str) -> String {
    let body = if kind == "price" {
        format_fixed(value)
    } else {
        format!("{value:.2}")
    };
    if currency.is_empty() {
        body
    } else {
        format!("{body} {currency}")
    }
}

/// The default Y tick label: thousands-separated, two decimals.
pub fn default_y_format(value: f64) -> String {
    format_fixed(value)
}

fn parse_iso(s: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.naive_utc())
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok())
}

/// Python `strftime("%d %b")` / `"%b %y"` for the tick labels.
fn strftime(point: NaiveDateTime, fmt: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    match fmt {
        "%H:%M" => format!("{:02}:{:02}", point.hour(), point.minute()),
        "%d %b" => format!("{:02} {}", point.day(), MONTHS[point.month0() as usize]),
        "%b %y" => format!(
            "{} {:02}",
            MONTHS[point.month0() as usize],
            point.year() % 100
        ),
        _ => point.to_string(),
    }
}

/// At most three X axis ticks for a chart `columns` braille cells wide.
///
/// `times` are ISO timestamp strings; unparseable entries are skipped rather
/// than aborting the chart. Ends anchor at columns 0 and columns-1 so the
/// labels line up with the chart edges; a middle tick is added only when the
/// span is wide enough that its label cannot collide with the ends (label
/// width estimated as `len(label) + 2` cells).
pub fn x_ticks(times: &[&str], columns: usize) -> Vec<(usize, String)> {
    if times.is_empty() || times.len() < 2 || columns < 8 {
        return Vec::new();
    }
    let parsed: Vec<NaiveDateTime> = times.iter().filter_map(|t| parse_iso(t)).collect();
    if parsed.len() < 2 {
        return Vec::new();
    }
    let span = parsed[parsed.len() - 1] - parsed[0];
    let fmt = if span < HOURS_36 {
        "%H:%M"
    } else if span < MONTHS_18 {
        "%d %b"
    } else {
        "%b %y"
    };
    let labels: Vec<String> = parsed.iter().map(|p| strftime(*p, fmt)).collect();
    let last_col = columns - 1;
    let mut ticks = vec![
        (0usize, labels[0].clone()),
        (last_col, labels[labels.len() - 1].clone()),
    ];
    // Nearest sample to the span midpoint.
    let middle = (0..parsed.len())
        .min_by_key(|&i| {
            let offset = (parsed[i] - parsed[0]).num_milliseconds().abs()
                - span.num_milliseconds().abs() / 2;
            offset.abs()
        })
        .unwrap();
    let mid_col =
        crate::braille::py_round(middle as f64 * last_col as f64 / (parsed.len() - 1) as f64)
            as usize;
    if 0 < mid_col && mid_col < last_col {
        // A middle label needs clearance from both anchored end labels.
        let width = labels.iter().map(String::len).max().unwrap_or(0) + 2;
        if mid_col >= width && last_col - mid_col >= width {
            ticks.insert(1, (mid_col, labels[middle].clone()));
        }
    }
    ticks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_ticks_matches_python() {
        assert_eq!(nice_ticks(0.0, 100.0, 3), vec![0.0, 50.0, 100.0]);
        assert_eq!(nice_ticks(5.0, 5.0, 3), vec![5.0, 5.0, 5.0]);
        assert_eq!(nice_ticks(100.0, 0.0, 3), vec![0.0, 50.0, 100.0]);
        assert_eq!(
            nice_ticks(10.25, 11.75, 5),
            vec![10.0, 10.5, 11.0, 11.5, 12.0]
        );
        assert_eq!(nice_ticks(0.102, 0.118, 3), vec![0.1, 0.11, 0.12]);
    }

    #[test]
    fn plot_scale_matches_python() {
        assert_eq!(plot_scale(10.25, 11.75, 5), (10.0, 12.0));
    }

    #[test]
    fn format_price_matches_python() {
        assert_eq!(format_price(1842.5, "price", ""), "1,842.50");
        assert_eq!(format_price(0.0425, "yield", ""), "0.04");
        assert_eq!(format_price(1842.5, "price", "AUD"), "1,842.50 AUD");
        assert_eq!(format_price(-1234.5, "price", ""), "-1,234.50");
    }

    #[test]
    fn x_ticks_match_python() {
        let times = [
            "2026-01-02T21:00:00",
            "2026-01-10T21:00:00",
            "2026-01-20T21:00:00",
        ];
        assert_eq!(
            x_ticks(&times, 40),
            vec![
                (0, "02 Jan".to_string()),
                (20, "10 Jan".to_string()),
                (39, "20 Jan".to_string())
            ]
        );
        assert!(x_ticks(&times, 4).is_empty());
        assert!(x_ticks(&[], 40).is_empty());
    }
}
