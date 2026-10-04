//! Persistent shell navigation and cached data freshness/spend.
use crate::screen::{color, Screen, Style};
use chrono::{NaiveDateTime, Utc};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FooterState {
    pub latest_bar: Option<NaiveDateTime>,
    pub spend: f64,
    pub provider: String,
    pub unavailable: bool,
}

impl FooterState {
    pub fn load(db: &delta_core::db::Db, provider: String) -> Result<Self, String> {
        let health = delta_services::data_health(db).map_err(|error| error.to_string())?;
        let spend = delta_services::llm_costs(db, None)
            .map_err(|error| error.to_string())?
            .iter()
            .map(|row| row.cost_usd)
            .sum();
        Ok(Self {
            latest_bar: health.latest_bar.into_values().max(),
            spend,
            provider,
            unavailable: false,
        })
    }

    pub fn freshness(&self, now: NaiveDateTime) -> (String, &'static str) {
        if self.unavailable {
            return ("?".into(), color::AMBER);
        }
        let Some(latest) = self.latest_bar else {
            return ("none".into(), color::RED);
        };
        let seconds = (now - latest).num_seconds();
        if seconds < 300 {
            ("live".into(), color::GREEN)
        } else if seconds < 3600 {
            (format!("{}m", seconds / 60), color::GREEN)
        } else if seconds < 86400 {
            (
                format!("{}h", seconds / 3600),
                if seconds > 43200 {
                    color::AMBER
                } else {
                    color::GREEN
                },
            )
        } else {
            (
                format!("{}d", seconds / 86400),
                if seconds < 7 * 86400 {
                    color::AMBER
                } else {
                    color::RED
                },
            )
        }
    }

    pub fn paint(&self, screen: &mut Screen, active: &str, context: &str) {
        self.paint_at(screen, active, context, Utc::now().naive_utc());
    }

    pub fn paint_at(&self, screen: &mut Screen, active: &str, context: &str, now: NaiveDateTime) {
        let y = screen.h.saturating_sub(1);
        let panel = Style::DEFAULT.bg(color::PANEL);
        let muted = Style::fg(color::MUTED).bg(color::PANEL);
        let plain = Style::fg(color::FG).bg(color::PANEL);
        screen.fill(0, y, screen.w, y + 1, Style::DEFAULT.bg(color::BLACK));
        screen.fill(1, y, screen.w.saturating_sub(1), y + 1, panel);
        let compact = screen.w < 123;
        let minimal = screen.w < 87;
        let mut x = 1;
        for (key, name) in [
            ("1", "Home"),
            ("2", "Watchlist"),
            ("3", "Research"),
            ("4", "Theses"),
            ("5", "Ask"),
            ("6", "Decisions"),
        ] {
            let selected = name == active;
            let text = if selected || !compact {
                format!("{key} {name}")
            } else {
                key.into()
            };
            let selected_bg = if name == "Settings" {
                "#494949"
            } else {
                color::BLUE_BG
            };
            let style = if selected {
                Style::fg(color::WHITE).bg(selected_bg).bold()
            } else {
                muted
            };
            screen.fill(
                x,
                y,
                x + text.width() + 2,
                y + 1,
                Style::DEFAULT.bg(if selected { selected_bg } else { color::PANEL }),
            );
            screen.text(x + 1, y, key, style.bold());
            if text.len() > key.len() {
                screen.text(x + 1 + key.len(), y, &text[key.len()..], style);
            }
            x += text.width() + 2;
        }
        let (age, mut dot_color) = self.freshness(now);
        if active == "Settings" {
            dot_color = "#a6a6a6";
        }
        let data = format!("data {age}");
        let spend = format!("${:.2}", self.spend);
        let provider = if self.provider.is_empty() {
            "—"
        } else {
            &self.provider
        };
        let mut parts = vec![
            ("●".to_string(), Style::fg(dot_color).bg(color::PANEL)),
            ("  ".into(), panel),
            (data, plain),
            ("  ".into(), panel),
        ];
        if !minimal {
            parts.extend([(provider.to_string(), plain), ("  ".into(), panel)]);
        }
        parts.extend([(spend, plain), ("  ".into(), panel)]);
        let settings = active == "Settings";
        let cfg_style = if settings {
            Style::fg(color::WHITE).bg("#494949").bold()
        } else {
            muted
        };
        // Settings is a NavKey with a cell of padding on both sides.
        parts.extend([
            ("c".into(), cfg_style.bold()),
            (" Settings".into(), cfg_style),
            (
                " ".into(),
                if settings {
                    Style::DEFAULT.bg("#494949")
                } else {
                    panel
                },
            ),
        ]);
        if !minimal {
            parts.extend([(" ".into(), panel), ("? help · g go ".into(), muted)]);
        }
        let length: usize = parts.iter().map(|(text, _)| text.width()).sum();
        let right = screen.w.saturating_sub(1 + length).max(x);
        if right > x + 1 {
            screen.text(
                x,
                y,
                &crate::research::truncate(context, right - x - 1),
                muted,
            );
        }
        let mut position = right;
        for (text, style) in parts {
            if settings && text == "c" {
                screen.put(
                    position.saturating_sub(1),
                    y,
                    ' ',
                    Style::DEFAULT.bg("#494949"),
                );
            }
            position = screen.text(position, y, &text, style);
        }
        screen.put(
            screen.w.saturating_sub(2),
            y,
            ' ',
            if settings && minimal {
                Style::DEFAULT.bg("#494949")
            } else {
                panel
            },
        );
        screen.put(
            screen.w.saturating_sub(1),
            y,
            ' ',
            Style::DEFAULT.bg(color::BLACK),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freshness_matches_python_boundaries() {
        let now = Utc::now().naive_utc();
        for (seconds, label, color) in [
            (0, "live", color::GREEN),
            (300, "5m", color::GREEN),
            (3600, "1h", color::GREEN),
            (43200, "12h", color::GREEN),
            (43201, "12h", color::AMBER),
            (86400, "1d", color::AMBER),
            (604800, "7d", color::RED),
        ] {
            let state = FooterState {
                latest_bar: Some(now - chrono::Duration::seconds(seconds)),
                ..Default::default()
            };
            assert_eq!(state.freshness(now), (label.to_string(), color));
        }
        assert_eq!(
            FooterState::default().freshness(now),
            ("none".into(), color::RED)
        );
    }
}
