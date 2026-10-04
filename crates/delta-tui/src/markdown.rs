//! Markdown prose rendering shared by reports and Ask.
use crate::research::wrap;
use crate::screen::{color, Style};

pub struct MarkdownLine {
    pub text: String,
    pub style: Style,
}

/// Render headings, lists, quotes, links and fenced code into display lines.
pub fn render(markdown: &str, width: usize) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();
    let mut code = false;
    for raw in markdown.lines() {
        if raw.trim_start().starts_with("```") {
            code = !code;
            continue;
        }
        let (text, style) = if code {
            (raw.to_string(), Style::fg(color::FG).bg(color::PANEL))
        } else if raw.starts_with('#') {
            (
                inline(raw.trim_start_matches('#').trim_start()),
                Style::fg(color::BLUE).bold(),
            )
        } else if let Some(item) = raw.strip_prefix("- ").or_else(|| raw.strip_prefix("* ")) {
            (format!("• {}", inline(item)), Style::fg(color::FG))
        } else if let Some(quote) = raw.strip_prefix("> ") {
            (format!("│ {}", inline(quote)), Style::fg(color::MUTED))
        } else {
            (inline(raw), Style::fg(color::FG))
        };
        let wrapped = if text.is_empty() {
            vec![String::new()]
        } else if code {
            vec![crate::research::truncate(&text, width)]
        } else {
            wrap(&text, width)
        };
        for text in wrapped {
            lines.push(MarkdownLine { text, style });
        }
    }
    lines
}

fn inline(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while !rest.is_empty() {
        if rest.starts_with('[') {
            if let Some(end) = rest.find("](") {
                if let Some(close) = rest[end + 2..].find(')') {
                    out.push_str(&rest[1..end]);
                    rest = &rest[end + 3 + close..];
                    continue;
                }
            }
        }
        if rest.starts_with("**") || rest.starts_with("__") {
            rest = &rest[2..];
            continue;
        }
        let ch = rest.chars().next().unwrap();
        if ch != '`' {
            out.push(ch);
        }
        rest = &rest[ch.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_render_links_headings_lists_and_code_without_raw_markup() {
        let lines = render(
            "## Findings\n- **Growth** [source](evidence:news:1)\n```rust\n  x += 1;\n```",
            40,
        );
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["Findings", "• Growth source", "  x += 1;"]
        );
        assert!(lines[0].style.bold);
        assert_eq!(lines[2].style.bg, Some(color::PANEL));
    }
}
