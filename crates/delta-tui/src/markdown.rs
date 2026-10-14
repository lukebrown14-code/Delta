//! `DeltaMarkdown`: pulldown-cmark parsing plus Textual's Markdown widget
//! layout rules (port target: `textual/widgets/_markdown.py` as themed by
//! `delta/tui/theme.py`; Tier B — prose).
//!
//! The renderer produces one [`Block`] per markdown block with Textual's
//! margins (headers `2 0 1 0`, paragraphs `0 0 1 0`, rules `padding-top 1`
//! plus `margin-bottom 1`), Textual's inline styles (`.strong` bold,
//! `.em` italic, `.code_inline` on a 10% warning tint) and Rich word wrap
//! ([`crate::wrap`]). Heading colours are the `markdown-h*` tokens
//! (`#5b8def` on both Delta themes); h1 centres whole-line, h2 underlines,
//! h3 is plain bold.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::theme::Theme;
use crate::wrap::wrap_text;

/// One styled span of a rendered line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub fg: &'static str,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub bg: Option<&'static str>,
}

impl Run {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            fg: Theme::FOREGROUND.hex,
            bold: false,
            italic: false,
            underline: false,
            bg: None,
        }
    }

    /// Screen-model style runs (fg hex + bold) for the golden painter; the
    /// underline/italic/bg detail the cell model cannot carry is dropped,
    /// exactly as the exporter's harness compares (char, fg, bg, bold).
    pub fn screen_runs(runs: &[Run]) -> Vec<(String, crate::screen::Style)> {
        runs.iter()
            .map(|run| {
                let mut style = crate::screen::Style::fg(run.fg);
                if run.bold {
                    style = style.bold();
                }
                (run.text.clone(), style)
            })
            .collect()
    }

    /// Ratatui style for the component path.
    pub fn rstyle(&self) -> Style {
        let mut style = Style::default().fg(crate::theme::hex_color(self.fg));
        if let Some(bg) = self.bg {
            style = style.bg(crate::theme::hex_color(bg));
        }
        let mut modifiers = Modifier::empty();
        if self.bold {
            modifiers |= Modifier::BOLD;
        }
        if self.italic {
            modifiers |= Modifier::ITALIC;
        }
        if self.underline {
            modifiers |= Modifier::UNDERLINED;
        }
        style.add_modifier(modifiers)
    }
}

/// One laid-out markdown block: blank rows above and below, then content
/// lines at the requested content width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub margin_top: usize,
    pub lines: Vec<Vec<Run>>,
    pub margin_bottom: usize,
}

/// A numbered/bulleted list under construction.
#[derive(Debug, Default)]
struct ListState {
    ordered: bool,
    index: usize,
}

/// Render `source` into laid-out blocks at `width` content cells.
pub fn render(source: &str, width: usize) -> Vec<Block> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(source, options);

    let mut blocks: Vec<Block> = Vec::new();
    // Inline segment assembly: (byte range, styled run) over the
    // paragraph/heading text; a style stack merges nested strong/em/code.
    let mut text = String::new();
    let mut segments: Vec<(usize, usize, Run)> = Vec::new();
    let mut style_stack: Vec<Run> = Vec::new();
    // Block-level context.
    let mut heading: Option<HeadingLevel> = None;
    let mut in_code = false;
    let mut code = String::new();
    let mut list: ListState = ListState::default();
    let mut list_items: Vec<Block> = Vec::new();

    /// The run template for text emitted now: every active style merged.
    fn merged(stack: &[Run]) -> Run {
        let mut run = Run::plain("");
        for style in stack {
            run.bold |= style.bold;
            run.italic |= style.italic;
            run.underline |= style.underline;
            run.fg = style.fg;
        }
        run
    }

    fn flush_inline(
        text: &mut String,
        segments: &mut Vec<(usize, usize, Run)>,
        blocks: &mut Vec<Block>,
        width: usize,
        heading: Option<HeadingLevel>,
    ) {
        let plain = std::mem::take(text);
        let segs = std::mem::take(segments);
        let (top, bottom) = if heading.is_some() { (2, 1) } else { (0, 1) };
        let lines = match heading {
            Some(HeadingLevel::H1) => {
                // content-align: center middle — the whole line one run,
                // padded to the full content width.
                wrap_text(plain.trim(), width)
                    .into_iter()
                    .map(|line| {
                        let cells = crate::wrap::cell_width(&line);
                        let pad = width.saturating_sub(cells) / 2;
                        let right = width.saturating_sub(cells + pad);
                        vec![Run {
                            text: format!("{}{}{}", " ".repeat(pad), line, " ".repeat(right)),
                            fg: Theme::TEXT_PRIMARY.hex,
                            bold: true,
                            italic: false,
                            underline: false,
                            bg: None,
                        }]
                    })
                    .collect()
            }
            _ => {
                let mut lines = wrap_lines(&plain, &segs, width);
                if let Some(level) = heading {
                    // The delta theme pins markdown-h1/2/3-color to
                    // `$text-primary`; Textual's styles: h2 underline,
                    // h3+ bold.
                    for line in &mut lines {
                        for run in line {
                            run.fg = Theme::TEXT_PRIMARY.hex;
                            run.underline = level == HeadingLevel::H2;
                            run.bold = matches!(
                                level,
                                HeadingLevel::H3
                                    | HeadingLevel::H4
                                    | HeadingLevel::H5
                                    | HeadingLevel::H6
                            );
                        }
                    }
                }
                lines
            }
        };
        blocks.push(Block {
            margin_top: top,
            lines,
            margin_bottom: bottom,
        });
    }

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => heading = Some(level),
            Event::End(TagEnd::Heading(_)) => {
                flush_inline(&mut text, &mut segments, &mut blocks, width, heading.take());
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                flush_inline(&mut text, &mut segments, &mut blocks, width, None);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                // BlockQuote prose is Tier B; its text flows into the block
                // stream (the border/tint chrome lands with the screens).
            }
            Event::End(TagEnd::BlockQuote(_)) => {}
            Event::Rule => {
                blocks.push(Block {
                    margin_top: 0,
                    // height 1 + padding-top 1: a blank row, then the border.
                    lines: vec![
                        vec![Run::plain("")],
                        vec![Run {
                            text: "─".repeat(width),
                            fg: Theme::SECONDARY.hex,
                            bold: false,
                            italic: false,
                            underline: false,
                            bg: None,
                        }],
                    ],
                    margin_bottom: 1,
                });
            }
            Event::Start(Tag::CodeBlock(_kind)) => {
                in_code = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                let code = std::mem::take(&mut code);
                // MarkdownFence: margin 1 0, Label padding 1 2, black 10%
                // over the surface background, rgb(210,210,210) code text.
                let mut lines = vec![vec![Run::plain("")]];
                for source_line in wrap_text(code.trim_end(), width.saturating_sub(4)) {
                    lines.push(vec![Run {
                        text: format!("  {source_line}  "),
                        fg: "#d2d2d2",
                        bold: false,
                        italic: false,
                        underline: false,
                        bg: Some("#0c0c0c"),
                    }]);
                }
                lines.push(vec![Run::plain("")]);
                lines.insert(0, vec![Run::plain("")]);
                blocks.push(Block {
                    margin_top: 0,
                    lines,
                    margin_bottom: 1,
                });
            }
            Event::Text(t) => {
                if in_code {
                    code.push_str(&t);
                } else {
                    let run = merged(&style_stack);
                    push_inline(&mut text, &mut segments, &t, run);
                }
            }
            Event::Code(t) => {
                // .code_inline: $warning 10% background, $text-warning 95%,
                // resolved over the surface the dialog provides.
                let run = Run {
                    text: t.to_string(),
                    fg: "#e9960b",
                    bold: false,
                    italic: false,
                    underline: false,
                    bg: Some("#21170c"),
                };
                push_inline(&mut text, &mut segments, &t, run);
            }
            Event::Start(Tag::Strong) => {
                style_stack.push(Run {
                    bold: true,
                    ..Run::plain("")
                });
            }
            Event::End(TagEnd::Strong) => {
                style_stack.pop();
            }
            Event::Start(Tag::Emphasis) => {
                style_stack.push(Run {
                    italic: true,
                    ..Run::plain("")
                });
            }
            Event::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }
            Event::Start(Tag::Strikethrough) => {
                // The cell model has no strike attribute; the text keeps
                // the paragraph style (logged as a Tier B finding).
                style_stack.push(Run::plain(""));
            }
            Event::End(TagEnd::Strikethrough) => {
                style_stack.pop();
            }
            Event::Start(Tag::Link { .. }) => {
                // Textual 8 styles links with a click action only; the text
                // keeps the paragraph style.
                style_stack.push(Run::plain(""));
            }
            Event::End(TagEnd::Link) => {
                style_stack.pop();
            }
            Event::SoftBreak => {
                let run = merged(&style_stack);
                push_inline(&mut text, &mut segments, " ", run);
            }
            Event::HardBreak => push_inline(&mut text, &mut segments, "\n", Run::plain("\n")),
            Event::Start(Tag::List(start)) => {
                list = ListState {
                    ordered: start.is_some(),
                    index: start.unwrap_or(1) as usize,
                };
                list_items = Vec::new();
            }
            Event::End(TagEnd::List(_)) => {
                let items = std::mem::take(&mut list_items);
                let ordered = list.ordered;
                let lines = layout_list(&items, width, ordered);
                blocks.push(Block {
                    margin_top: 0,
                    lines,
                    margin_bottom: 1,
                });
            }
            Event::Start(Tag::Item) => {
                segments.clear();
                text.clear();
            }
            Event::End(TagEnd::Item) => {
                let plain = std::mem::take(&mut text);
                let segs = std::mem::take(&mut segments);
                let lines = wrap_lines(&plain, &segs, width);
                list_items.push(Block {
                    margin_top: 0,
                    lines,
                    margin_bottom: 0,
                });
                list.index += 1;
            }
            _ => {}
        }
    }
    blocks
}

/// Append `content` to the inline buffer, registering a styled segment.
fn push_inline(
    text: &mut String,
    segments: &mut Vec<(usize, usize, Run)>,
    content: &str,
    run: Run,
) {
    let start = text.len();
    text.push_str(content);
    if start < text.len() {
        segments.push((start, text.len(), run));
    }
}

/// Wrap one inline buffer into styled lines: wrap the plain text, then cut
/// the styled segments at the break offsets. `\n` in the buffer is a hard
/// break (markdown hardbreak).
fn wrap_lines(plain: &str, segments: &[(usize, usize, Run)], width: usize) -> Vec<Vec<Run>> {
    use crate::wrap::compute_wrap_offsets;

    fn line_runs(
        plain: &str,
        segments: &[(usize, usize, Run)],
        start: usize,
        end: usize,
    ) -> Vec<Run> {
        let mut runs: Vec<Run> = Vec::new();
        let mut cursor = start;
        for (seg_start, seg_end, run) in segments {
            if *seg_end <= cursor || *seg_start >= end {
                continue;
            }
            let from = (*seg_start).max(cursor);
            if from > cursor {
                runs.push(Run {
                    text: plain[cursor..from].to_string(),
                    ..Run::plain("")
                });
            }
            let to = (*seg_end).min(end);
            runs.push(Run {
                text: plain[from..to].to_string(),
                ..run.clone()
            });
            cursor = to;
        }
        if cursor < end {
            runs.push(Run {
                text: plain[cursor..end].to_string(),
                ..Run::plain("")
            });
        }
        runs
    }

    let mut lines: Vec<Vec<Run>> = Vec::new();
    let mut offset = 0usize;
    for part in plain.split('\n') {
        let part_start = offset;
        let mut breaks = compute_wrap_offsets(part, width);
        breaks.push(part.len());
        let mut previous = 0usize;
        for offset in &breaks {
            let mut runs = line_runs(plain, segments, part_start + previous, part_start + offset);
            // The painter rstrips: the trailing break space keeps no style,
            // whichever run it rode in on.
            while runs.len() > 1 && runs.last().is_some_and(|r| r.text.is_empty()) {
                runs.pop();
            }
            if let Some(last) = runs.last_mut() {
                let trimmed = last.text.trim_end_matches(' ').len();
                last.text.truncate(trimmed);
            }
            while runs.len() > 1 && runs.last().is_some_and(|r| r.text.is_empty()) {
                runs.pop();
            }
            lines.push(runs);
            previous = *offset;
        }
        // Advance past this part plus its separating '\n'.
        offset = part_start + part.len() + 1;
    }
    if lines.is_empty() {
        lines.push(vec![Run::plain("")]);
    }
    lines
}

/// List layout: a bullet column (the symbol plus Textual's one-space
/// margin-right) and the item content beside it; ordered bullets are
/// right-justified to the widest number+suffix, as `MarkdownOrderedList`
/// does.
fn layout_list(items: &[Block], width: usize, ordered: bool) -> Vec<Vec<Run>> {
    let count = items.len();
    let numbered = |index: usize| format!("{index}. ");
    let symbol_width = if ordered {
        (1..=count).map(|i| numbered(i).len()).max().unwrap_or(0) + 1
    } else {
        "• ".len()
    };
    let content_width = width.saturating_sub(symbol_width);
    let mut lines: Vec<Vec<Run>> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let bullet_text = if ordered {
            format!("{:>width$}", numbered(index + 1), width = symbol_width - 1)
        } else {
            "• ".to_string()
        };
        for (position, line) in item.lines.iter().enumerate() {
            let mut runs = Vec::new();
            if position == 0 {
                runs.push(Run {
                    text: bullet_text.clone(),
                    fg: Theme::TEXT_PRIMARY.hex,
                    bold: false,
                    italic: false,
                    underline: false,
                    bg: None,
                });
            } else {
                runs.push(Run {
                    text: " ".repeat(symbol_width),
                    ..Run::plain("")
                });
            }
            for run in line {
                let mut indented = run.clone();
                if indented.text.len() > content_width {
                    // Keep the run inside the content column; full
                    // re-wrapping of styled runs is Tier B.
                    indented.text.truncate(content_width);
                }
                runs.push(indented);
            }
            lines.push(runs);
        }
    }
    lines
}

/// Stack laid-out blocks into one line list with CSS margin collapse: the
/// gap between two blocks is `max(previous.margin_bottom, next.margin_top)`
/// (Textual's layout collapses adjacent margins), not their sum.
pub fn stack_lines(blocks: &[Block]) -> Vec<Vec<Run>> {
    let mut lines: Vec<Vec<Run>> = Vec::new();
    let mut pending_bottom = 0usize;
    let mut first = true;
    for block in blocks {
        let pending_top = if first {
            block.margin_top
        } else {
            pending_bottom.max(block.margin_top)
        };
        for _ in 0..pending_top {
            lines.push(Vec::new());
        }
        for line in &block.lines {
            lines.push(line.clone());
        }
        pending_bottom = block.margin_bottom;
        first = false;
    }
    for _ in 0..pending_bottom {
        lines.push(Vec::new());
    }
    lines
}

/// Render markdown lines straight into a ratatui frame area (the component
/// path for future screens; the golden painter uses [`Run::screen_runs`]).
pub fn draw(source: &str, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let blocks = render(source, area.width as usize);
    let mut lines: Vec<Line> = Vec::new();
    for runs in stack_lines(&blocks) {
        lines.push(Line::from(
            runs.iter()
                .map(|run| Span::styled(run.text.clone(), run.rstyle()))
                .collect::<Vec<_>>(),
        ));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_text(block: &Block) -> Vec<String> {
        block
            .lines
            .iter()
            .map(|runs| runs.iter().map(|r| r.text.clone()).collect::<String>())
            .collect()
    }

    #[test]
    fn paragraphs_get_textual_margins_and_wrap_points() {
        let source = "It does not trade, it does not size positions, and it will not \
                      tell you what to buy or sell. The point is clarity, not tips.";
        let blocks = render(source, 62);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].margin_top, 0);
        assert_eq!(blocks[0].margin_bottom, 1);
        let lines = flat_text(&blocks[0]);
        assert_eq!(
            lines,
            vec![
                "It does not trade, it does not size positions, and it will not",
                "tell you what to buy or sell. The point is clarity, not tips.",
            ]
        );
    }

    #[test]
    fn headings_follow_the_delta_markdown_tokens() {
        let blocks = render(
            "# Welcome to Delta\n\n## 1. Tell it what you care about\n\n### level three",
            62,
        );
        assert_eq!(blocks.len(), 3);
        // h1: whole line centred, bold, text-primary, margins 2/1.
        assert_eq!(blocks[0].margin_top, 2);
        assert_eq!(blocks[0].margin_bottom, 1);
        let h1 = &blocks[0].lines[0];
        assert!(h1[0].text.starts_with(' ') && h1[0].text.trim() == "Welcome to Delta");
        assert!(h1[0].bold);
        assert_eq!(h1[0].fg, Theme::TEXT_PRIMARY.hex);
        // h2: underline, not bold, left-aligned.
        let h2 = &blocks[1].lines[0];
        assert_eq!(h2[0].text, "1. Tell it what you care about");
        assert!(!h2[0].bold);
        assert!(h2[0].underline);
        // h3: bold.
        assert!(blocks[2].lines[0][0].bold);
    }

    #[test]
    fn strong_spans_survive_the_wrap() {
        let blocks = render(
            "where **every claim points back to the fact it came from**.",
            62,
        );
        let runs = &blocks[0].lines[0];
        let plain: String = runs.iter().map(|r| r.text.clone()).collect();
        assert!(plain.starts_with("where every claim points"));
        // The bold run carries exactly the strong text.
        let bold: String = runs
            .iter()
            .filter(|r| r.bold)
            .map(|r| r.text.clone())
            .collect();
        assert_eq!(bold, "every claim points back to the fact it came from");
    }

    #[test]
    fn horizontal_rule_is_a_secondary_line_with_padding() {
        let blocks = render("above\n\n---\n\nbelow", 30);
        let hr = blocks
            .iter()
            .find(|b| b.lines.len() == 2)
            .expect("rule block");
        assert_eq!(hr.lines[1][0].text, "─".repeat(30));
        assert_eq!(hr.lines[1][0].fg, Theme::SECONDARY.hex);
        assert_eq!(hr.margin_bottom, 1);
    }

    #[test]
    fn lists_render_bullets_in_the_text_primary() {
        let blocks = render("- one\n- two", 20);
        let list = &blocks[0];
        assert_eq!(list.margin_bottom, 1);
        assert_eq!(flat_text(list).len(), 2);
        assert_eq!(list.lines[0][0].text, "• ");
        assert_eq!(list.lines[0][0].fg, Theme::TEXT_PRIMARY.hex);
    }

    #[test]
    fn inline_code_takes_the_warning_tint() {
        let blocks = render("press `a` to add", 30);
        let runs = &blocks[0].lines[0];
        let code = runs.iter().find(|r| r.bg.is_some()).expect("code span");
        assert_eq!(code.text, "a");
    }

    #[test]
    fn render_fits_a_test_backend() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 24)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    "# Welcome to Delta\n\nDelta reads for you. It collects facts.",
                    frame,
                    frame.area(),
                )
            })
            .unwrap();
    }
}

#[cfg(test)]
mod debug_tmp3 {
    use super::*;
    #[test]
    fn debug_line_runs() {
        let blocks = render("Delta reads for you. It collects facts about the companies you\ncare about, keeps them in one place, and writes summaries where **every claim points back\nto the fact it came from**.", 62);
        for (i, block) in blocks.iter().enumerate() {
            for (j, runs) in block.lines.iter().enumerate() {
                println!(
                    "block {i} line {j}: {:?}",
                    runs.iter()
                        .map(|r| (r.text.clone(), r.bold))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}
