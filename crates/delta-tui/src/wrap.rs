//! Word wrap and cell widths with Rich/Textual parity
//! (`docs/RUST_REWRITE_PLAN.md` "Text wrapping and Unicode width").
//!
//! Port of Rich's `_wrap.divide_line` — the wrap Textual's `Content` uses
//! for every widget line (`content.py` imports it), and therefore the wrap
//! the golden screens pin. Words are `\s*\S+\s*` chunks; a word fits when
//! its *rstripped* width fits the remaining cells (the trailing break
//! space may overflow the line), else the line breaks at the word.
//! Overlong words fold by grapheme unit. Cell widths come from
//! `unicode-width`, matching Rich's `cell_len` for the prose Delta
//! renders; the wrap-parity tests pin outputs captured from Python.

use unicode_width::UnicodeWidthChar;

/// Rich's `get_character_cell_size` for one character.
///
/// `unicode-width` agrees with Rich's table for everything Delta renders;
/// the wrap-parity tests (captured from Rich/Textual) hold the line.
pub fn char_width(ch: char) -> usize {
    match ch {
        // Control characters have no cell width.
        '\0'..='\u{1f}' | '\u{7f}' => 0,
        _ => ch.width().unwrap_or(0),
    }
}

/// Rich's `cell_len`: the number of terminal cells a string occupies.
pub fn cell_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// Break offsets for `text` at `width` cells: the port of
/// `rich/_wrap.py::divide_line` (fold=True; tabs carry no width, as in
/// Textual's `cell_len`).
pub fn compute_wrap_offsets(text: &str, width: usize) -> Vec<usize> {
    let mut breaks = Vec::new();
    let mut cell_offset = 0usize;
    for (start, chunk) in words(text) {
        let word_length = cell_width(chunk.trim_end());
        let remaining = width.saturating_sub(cell_offset);
        if remaining >= word_length {
            // The word fits; the chunk (with its trailing space) accumulates
            // and may legally overflow the line by that space.
            cell_offset += cell_width(chunk);
        } else if word_length > width {
            // The word does not fit on any line: it starts on a fresh line
            // and folds by grapheme unit, one unit minimum.
            if start > 0 {
                breaks.push(start);
            }
            let mut line_size = 0usize;
            for (index, unit) in grapheme_units(chunk) {
                let unit_width = cell_width(unit);
                if line_size + unit_width > width {
                    // The boundary sits at the piece's end (the overflow
                    // point); Rich pushes the chunk's own start once via
                    // `if start`, then each accumulated boundary.
                    if start + index > 0 {
                        breaks.push(start + index);
                    }
                    line_size = 0;
                }
                line_size += unit_width;
            }
            cell_offset = line_size;
        } else if cell_offset > 0 && start > 0 {
            // The word fits on the next line.
            breaks.push(start);
            cell_offset = cell_width(chunk);
        }
    }
    breaks
}

/// Wrap `text` to `width`-cell lines (the painter rstrips trailing break
/// spaces). Empty text yields one empty line, matching the exporter's
/// render of an empty paragraph.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut previous = 0usize;
    let mut offsets = compute_wrap_offsets(text, width);
    offsets.push(text.len());
    for offset in offsets {
        lines.push(text[previous..offset].to_string());
        previous = offset;
    }
    lines
}

/// The `\s*\S+\s*` word chunking: `(start, chunk)` spans into `text`
/// (Rich's `words`; whitespace-only text yields nothing).
fn words(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut position = 0usize;
    let char_len = |at: usize| text[at..].chars().next().map(char::len_utf8).unwrap_or(1);
    let is_space =
        |at: usize| bytes[at].is_ascii_whitespace() || text[at..].starts_with('\u{00a0}');
    while position < text.len() {
        // Skip any run of whitespace `\s*`, but only when a word follows
        // (a trailing whitespace run belongs to no word, as in Rich).
        let mut at = position;
        while at < text.len() && is_space(at) {
            at += char_len(at);
        }
        if at >= text.len() {
            break;
        }
        let start = at;
        // `\S+`
        while at < text.len() && !is_space(at) {
            at += char_len(at);
        }
        // `\s*`
        while at < text.len() && is_space(at) {
            at += char_len(at);
        }
        out.push((start, &text[start..at]));
        position = at;
    }
    out
}

/// Fold units: grapheme-ish clusters so an emoji or flag never splits.
/// Covers the sequences Delta's prose renders (VS16, skin-tone modifiers,
/// ZWJ chains, regional-indicator pairs, combining marks); a full UAX-29
/// implementation is Tier B overkill.
fn grapheme_units(chunk: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut units: Vec<(usize, &str)> = Vec::new();
    let mut start = 0usize;
    let mut previous_width1 = false;
    let mut previous_ri = false;
    let mut previous_zwj = false;
    for (index, ch) in chunk.char_indices() {
        let ri = ('\u{1f1e6}'..='\u{1f1ff}').contains(&ch);
        let extend = is_extend(ch);
        if index > start {
            let join = extend
                || previous_zwj
                || (previous_ri && ri)
                || (previous_width1 && matches!(ch, '\u{fe0f}'));
            if !join {
                units.push((start, &chunk[start..index]));
                start = index;
            }
        }
        previous_width1 = char_width(ch) == 1;
        previous_ri = ri;
        previous_zwj = ch == '\u{200d}';
    }
    if start < chunk.len() {
        units.push((start, &chunk[start..]));
    }
    units.into_iter()
}

/// Combining marks and variation selectors that extend the previous unit.
fn is_extend(ch: char) -> bool {
    matches!(ch as u32,
        0x0300..=0x036f      // combining diacritics
        | 0x1ab0..=0x1aff
        | 0x1dc0..=0x1dff
        | 0x20d0..=0x20f0
        | 0xfe00..=0xfe0f    // variation selectors
        | 0x1f3fb..=0x1f3ff  // emoji skin-tone modifiers
        | 0x200d             // ZWJ (handled above too)
        | 0xe0100..=0xe01ef
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(text, width, expected lines)` captured from Rich's `divide_line`
    /// (Textual 8.2.8's `Content.wrap` path — the wrap the goldens pin),
    /// including the tutorial paragraphs the help golden renders at width
    /// 62, a folded long word, emoji, CJK and whitespace-only chunks.
    fn captured_cases() -> Vec<(&'static str, usize, Vec<&'static str>)> {
        vec![
            (
                "Delta reads for you. It collects facts about the companies you care about, keeps them in one place, and writes summaries where every claim points back to the fact it came from.",
                62,
                vec![
                    "Delta reads for you. It collects facts about the companies you ",
                    "care about, keeps them in one place, and writes summaries ",
                    "where every claim points back to the fact it came from.",
                ],
            ),
            (
                "It does not trade, it does not size positions, and it will not tell you what to buy or sell. The point is clarity, not tips.",
                62,
                vec![
                    "It does not trade, it does not size positions, and it will not ",
                    "tell you what to buy or sell. The point is clarity, not tips.",
                ],
            ),
            (
                "A watchlist entry is anything you want watched — one company, a whole sector, or a theme. Press a to open the entry form, fill in the fields, and press enter to save. Press esc to close the form.",
                62,
                vec![
                    "A watchlist entry is anything you want watched — one company, ",
                    "a whole sector, or a theme. Press a to open the entry form, ",
                    "fill in the fields, and press enter to save. Press esc to ",
                    "close the form.",
                ],
            ),
            (
                "Prices stream from Yahoo while Watchlist is open. DAY % is Yahoo’s daily percentage change: ▲ up, ▼ down, ─ unchanged. Quote age and connection state are separate; delivery may vary by market. Missing quotes show —. Streaming quotes do not replace gathered historical bars.",
                62,
                vec![
                    "Prices stream from Yahoo while Watchlist is open. DAY % is ",
                    "Yahoo’s daily percentage change: ▲ up, ▼ down, ─ unchanged. ",
                    "Quote age and connection state are separate; delivery may vary ",
                    "by market. Missing quotes show —. Streaming quotes do not ",
                    "replace gathered historical bars.",
                ],
            ),
            (
                "Use / to filter, up/down to select, and enter to refresh metrics for the selected target. The metrics inspector stays open beside the watchlist. Press space on an asset-class header to expand or collapse its targets, and press r to cycle the chart range. Press d to remove a selected target.",
                62,
                vec![
                    "Use / to filter, up/down to select, and enter to refresh ",
                    "metrics for the selected target. The metrics inspector stays ",
                    "open beside the watchlist. Press space on an asset-class ",
                    "header to expand or collapse its targets, and press r to cycle ",
                    "the chart range. Press d to remove a selected target.",
                ],
            ),
            (
                "supercalifragilisticexpialidocious and a_much_longer_token_than_width_without_spaces_0123456789 tail",
                20,
                vec![
                    "supercalifragilistic",
                    "expialidocious and ",
                    "a_much_longer_token_",
                    "than_width_without_s",
                    "paces_0123456789 ",
                    "tail",
                ],
            ),
            (
                "Emoji: 👍🏽 raised_hands 🙌 and flags 🇦🇺🇯🇵 mix",
                12,
                vec!["Emoji: 👍🏽 ", "raised_hands ", "🙌 and flags ", "🇦🇺🇯🇵 mix"],
            ),
            (
                "CJK: 中文測試漢字寬度與換行規則 mixed with latin words",
                14,
                vec!["CJK: ", "中文測試漢字寬", "度與換行規則 ", "mixed with ", "latin words"],
            ),
            (
                "一個沒有空格的非常長的中文單字超出寬度時應逐字折行",
                8,
                vec!["一個沒有", "空格的非", "常長的中", "文單字超", "出寬度時", "應逐字折", "行"],
            ),
            (
                "trailing spaces keep   the   chunk   boundaries   honest",
                15,
                vec![
                    "trailing spaces ",
                    "keep   the   ",
                    "chunk   ",
                    "boundaries   ",
                    "honest",
                ],
            ),
            (
                "indent▲arrow—em-dash·middle-dot",
                10,
                vec!["indent▲arr", "ow—em-dash", "·middle-do", "t"],
            ),
            ("", 40, vec![""]),
            ("one", 1, vec!["o", "n", "e"]),
            ("ab cd", 3, vec!["ab ", "cd"]),
        ]
    }

    #[test]
    fn wrap_parity_with_captured_rich_outputs() {
        for (text, width, want) in captured_cases() {
            let got = wrap_text(text, width);
            assert_eq!(got, want, "wrap mismatch at width {width} for {text:?}");
        }
    }

    #[test]
    fn cell_widths_match_rich_semantics() {
        assert_eq!(cell_width("abc"), 3);
        assert_eq!(cell_width("中文"), 4);
        assert_eq!(cell_width("—"), 1);
        assert_eq!(cell_width("👍"), 2);
        assert_eq!(cell_width("▲"), 1);
        assert_eq!(cell_width(""), 0);
    }

    #[test]
    fn every_line_fits_the_width_after_rstrip() {
        for (text, width, _) in captured_cases() {
            for line in wrap_text(text, width) {
                assert!(
                    cell_width(line.trim_end()) <= width,
                    "line {line:?} exceeds {width}"
                );
            }
        }
    }
}
