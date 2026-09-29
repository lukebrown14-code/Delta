//! Delta theme: resolved `delta-dark` tokens (port of `delta/tui/theme.py`).
//!
//! The plan requires widgets to take styles only from `Theme` — no inline
//! colours. Values are the literal hex definitions from the Python theme
//! (dark-only for the rewrite; the light variant is a post-cutover question).

use ratatui::style::Color;

/// One resolved theme token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub name: &'static str,
    pub hex: &'static str,
}

impl Token {
    pub fn color(self) -> Color {
        hex_color(self.hex)
    }
}

/// Parse `#rrggbb` into a ratatui RGB colour.
pub fn hex_color(hex: &str) -> Color {
    {
        // ratatui's Color::from_u32 little-endian trick; keep it explicit.
        let bytes = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&bytes[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&bytes[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&bytes[4..6], 16).unwrap_or(0);
        Color::Rgb(r, g, b)
    }
}

/// Resolved `delta-dark`. Ink tokens (`primary`, `success`, ...) are the brand
/// hexes exactly; the `text-` siblings are what you read on black.
pub struct Theme;

impl Theme {
    pub const PRIMARY: Token = Token {
        name: "primary",
        hex: "#264b96",
    };
    pub const SECONDARY: Token = Token {
        name: "secondary",
        hex: "#264b96",
    };
    pub const SUCCESS: Token = Token {
        name: "success",
        hex: "#15803d",
    };
    pub const WARNING: Token = Token {
        name: "warning",
        hex: "#d97706",
    };
    pub const ERROR: Token = Token {
        name: "error",
        hex: "#b91c1c",
    };
    pub const FOREGROUND: Token = Token {
        name: "foreground",
        hex: "#d4d4d4",
    };
    pub const BACKGROUND: Token = Token {
        name: "background",
        hex: "#000000",
    };
    pub const SURFACE: Token = Token {
        name: "surface",
        hex: "#0d0d0d",
    };
    pub const PANEL: Token = Token {
        name: "panel",
        hex: "#1a1a1a",
    };

    /// What you read: brighter siblings of the ink colours, same hue.
    pub const TEXT_PRIMARY: Token = Token {
        name: "text-primary",
        hex: "#5b8def",
    };
    pub const TEXT_SECONDARY: Token = Token {
        name: "text-secondary",
        hex: "#5b8def",
    };
    pub const TEXT_ERROR: Token = Token {
        name: "text-error",
        hex: "#f87171",
    };
    pub const TEXT_SUCCESS: Token = Token {
        name: "text-success",
        hex: "#22c55e",
    };
    pub const TEXT_WARNING: Token = Token {
        name: "text-warning",
        hex: "#f59e0b",
    };
    pub const TEXT_MUTED: Token = Token {
        name: "text-muted",
        hex: "#8a8a8a",
    };
    pub const TEXT_DISABLED: Token = Token {
        name: "text-disabled",
        hex: "#5c5c5c",
    };

    /// Lines: separators want a quiet grey, focus wants the readable blue.
    pub const BORDER: Token = Token {
        name: "border",
        hex: "#5b8def",
    };
    pub const BORDER_BLURRED: Token = Token {
        name: "border-blurred",
        hex: "#333333",
    };

    /// Chrome.
    pub const FOOTER_KEY: Token = Token {
        name: "footer-key-foreground",
        hex: "#5b8def",
    };
    pub const SCROLLBAR: Token = Token {
        name: "scrollbar",
        hex: "#3a3a3a",
    };

    /// Evidence kinds — five hues that are not the semantic four.
    pub const KIND_NEWS: Token = Token {
        name: "kind-news",
        hex: "#5ccfe6",
    };
    pub const KIND_FILING: Token = Token {
        name: "kind-filing",
        hex: "#ffd580",
    };
    pub const KIND_EVENT: Token = Token {
        name: "kind-event",
        hex: "#d7a1ff",
    };
    pub const KIND_FUNDAMENTAL: Token = Token {
        name: "kind-fundamental",
        hex: "#7ee0c0",
    };
    pub const KIND_PRICE: Token = Token {
        name: "kind-price",
        hex: "#a8b2c8",
    };

    /// A token by name, falling back to `fallback` (matches `token_color`).
    pub fn token_color(token: &str, fallback: &str) -> Color {
        match token {
            "primary" | "secondary" | "accent" => Self::PRIMARY.color(),
            "success" => Self::SUCCESS.color(),
            "warning" => Self::WARNING.color(),
            "error" => Self::ERROR.color(),
            "foreground" => Self::FOREGROUND.color(),
            "background" => Self::BACKGROUND.color(),
            "surface" => Self::SURFACE.color(),
            "panel" => Self::PANEL.color(),
            "text-primary" | "text-secondary" | "text-accent" => Self::TEXT_PRIMARY.color(),
            "text-success" => Self::TEXT_SUCCESS.color(),
            "text-error" => Self::TEXT_ERROR.color(),
            "text-warning" => Self::TEXT_WARNING.color(),
            "text-muted" => Self::TEXT_MUTED.color(),
            "text-disabled" => Self::TEXT_DISABLED.color(),
            "border" => Self::BORDER.color(),
            "border-blurred" => Self::BORDER_BLURRED.color(),
            "footer-key-foreground" => Self::FOOTER_KEY.color(),
            "kind-news" => Self::KIND_NEWS.color(),
            "kind-filing" => Self::KIND_FILING.color(),
            "kind-event" => Self::KIND_EVENT.color(),
            "kind-fundamental" => Self::KIND_FUNDAMENTAL.color(),
            "kind-price" => Self::KIND_PRICE.color(),
            _ => hex_color(fallback),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(hex_color("#5b8def"), Color::Rgb(0x5b, 0x8d, 0xef));
        assert_eq!(hex_color("#000000"), Color::Rgb(0, 0, 0));
    }

    #[test]
    fn token_lookup_with_fallback() {
        assert_eq!(
            Theme::token_color("text-muted", "#d4d4d4"),
            Color::Rgb(0x8a, 0x8a, 0x8a)
        );
        assert_eq!(
            Theme::token_color("nope", "#d4d4d4"),
            Color::Rgb(0xd4, 0xd4, 0xd4)
        );
    }
}
