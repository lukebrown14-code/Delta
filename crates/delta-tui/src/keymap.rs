//! Key bindings and nav items, ported from the Python app so the keymap
//! cannot drift from what the Python keymap shows.
//!
//! Sources: `delta/tui/app.py::DeltaApp.BINDINGS` (Anywhere), `shell.py`
//! (`NAV_ITEMS`/`CHROME_ITEMS`), `widgets.py::KEY_DISPLAY`, and each
//! installed screen's own `BINDINGS` (`screens/help.py::_binding_groups`:
//! app first, then the screens in name order — `chat`, `config`, `data`,
//! `decisions`, `home`, `targets`, `theses`; Home declares none of its own,
//! so its group is empty and the help omits it).

/// One binding: the key, the action label and the tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub key: &'static str,
    pub label: &'static str,
    pub tooltip: &'static str,
    /// `show=False` bindings are functional but never listed in a keymap.
    pub show: bool,
}

impl Binding {
    pub const fn shown(key: &'static str, label: &'static str, tooltip: &'static str) -> Self {
        Self {
            key,
            label,
            tooltip,
            show: true,
        }
    }

    pub const fn hidden(key: &'static str, label: &'static str, tooltip: &'static str) -> Self {
        Self {
            key,
            label,
            tooltip,
            show: false,
        }
    }
}

/// Keys whose Textual name is not what a user would recognise on a keycap
/// (`widgets.py::KEY_DISPLAY`).
pub const KEY_DISPLAY: &[(&str, &str)] =
    &[("question_mark", "?"), ("escape", "esc"), ("slash", "/")];

/// The key as a user would recognise it on a keycap
/// (`widgets.py::binding_key`; `key_display` overrides do not occur).
pub fn binding_key(key: &str) -> &str {
    KEY_DISPLAY
        .iter()
        .find(|(name, _)| *name == key)
        .map_or(key, |(_, display)| display)
}

/// `("1", "home", "Home")` triples (`shell.py`).
pub type NavItem = (&'static str, &'static str, &'static str);

/// The bar panels (`shell.py::NAV_ITEMS`).
pub const NAV_ITEMS: &[NavItem] = &[
    ("1", "home", "Home"),
    ("2", "targets", "Watchlist"),
    ("3", "data", "Research"),
    ("4", "theses", "Theses"),
    ("5", "chat", "Ask"),
    ("6", "decisions", "Decisions"),
];

/// Chrome actions, rendered flush-right on the status bar
/// (`shell.py::CHROME_ITEMS`).
pub const CHROME_ITEMS: &[NavItem] = &[("c", "config", "Settings")];

/// Reachable by hotkey, `g` and the palette, but not shown on the bar
/// (`shell.py::OFF_BAR_ITEMS` — currently empty).
pub const OFF_BAR_ITEMS: &[NavItem] = &[];

/// Everything navigation may iterate (`shell.py::ALL_ITEMS`).
pub fn all_items() -> Vec<NavItem> {
    NAV_ITEMS
        .iter()
        .chain(CHROME_ITEMS)
        .chain(OFF_BAR_ITEMS)
        .copied()
        .collect()
}

/// The app's Anywhere bindings (`DeltaApp.BINDINGS`, in order).
pub const APP_BINDINGS: &[Binding] = &[
    Binding::shown("1", "Home", "Desk overview"),
    Binding::shown("2", "Watchlist", "Manage what is watched"),
    Binding::shown("3", "Research", "Read reports and evidence"),
    Binding::shown("4", "Theses", "Track claims and evidence"),
    Binding::shown("5", "Ask", "Grounded Q&A over evidence"),
    Binding::shown("6", "Decisions", "Record and review decision context"),
    Binding::shown("c", "Settings", "Providers, model, plugins and diagnostics"),
    Binding::shown("h", "Home", "The landing dashboard"),
    Binding::shown("m", "Model", "Pick the model for this screen"),
    Binding::shown("p", "Provider", "Connect or switch the AI provider"),
    Binding::shown("g", "Go", "Jump to a panel"),
    Binding::shown("question_mark", "Help", "Show the keymap"),
    Binding::shown("q", "Quit", "Leave Delta"),
    Binding::shown("f2", "Theme", "Switch light/dark palette"),
];

/// One help-tab group: a title plus the screen's own bindings.
pub struct BindingGroup {
    pub title: &'static str,
    pub bindings: &'static [Binding],
}

/// Chat (`delta/tui/screens/chat.py::Chat.BINDINGS`).
const CHAT_BINDINGS: &[Binding] = &[
    Binding::shown("i", "ask", ""),
    Binding::hidden("enter", "ask", ""),
    Binding::shown("escape", "back", ""),
    Binding::shown("t", "targets", ""),
    Binding::shown("space", "toggle target", ""),
    Binding::shown("a", "all / none", ""),
    Binding::shown("z", "zoom", ""),
    Binding::shown("x", "clear", ""),
    Binding::hidden("y", "confirm", ""),
    Binding::hidden("left", "citation", ""),
    Binding::hidden("right", "citation", ""),
    Binding::shown("o", "open citation", ""),
    Binding::shown("s", "save to thesis", ""),
];

/// Config (`delta/tui/screens/config.py::Config.BINDINGS`).
const CONFIG_BINDINGS: &[Binding] = &[
    Binding::shown("d", "diagnostics", ""),
    Binding::shown("r", "refresh", ""),
    Binding::shown("l", "plugins", ""),
    Binding::shown("s", "source", ""),
    Binding::shown("a", "market", ""),
    Binding::shown("e", "edit market", ""),
    Binding::shown("x", "remove market", ""),
    Binding::shown("escape", "back", ""),
];

/// Research (`delta/tui/screens/research.py::Research.BINDINGS`; `Data`
/// subclasses it without adding keys, so one group covers both).
const RESEARCH_BINDINGS: &[Binding] = &[
    Binding::shown("e", "evidence", ""),
    Binding::shown("r", "report", ""),
    Binding::shown("n", "generate report", ""),
    Binding::shown("u", "gather company", ""),
    Binding::shown("U", "gather all targets", ""),
    Binding::shown("slash", "search evidence", ""),
    Binding::shown("k", "kind", ""),
    Binding::shown("l", "load more", ""),
    Binding::shown("space", "fold prices", ""),
    Binding::shown("t", "company", ""),
    Binding::shown("o", "open link", ""),
    Binding::shown("v", "view in report", ""),
    Binding::shown("z", "zoom", ""),
    Binding::shown("escape", "back", ""),
];

/// Decisions (`delta/tui/screens/decisions.py::Decisions.BINDINGS`).
const DECISIONS_BINDINGS: &[Binding] = &[
    Binding::shown("n", "new", ""),
    Binding::shown("e", "edit", ""),
    Binding::shown("d", "delete", ""),
    Binding::shown("r", "review", ""),
    Binding::shown("o", "research", ""),
    Binding::shown("slash", "filter", ""),
    Binding::hidden("y", "confirm", ""),
    Binding::hidden("escape", "back", ""),
];

/// Watchlist (`delta/tui/screens/targets.py::Targets.BINDINGS`).
const TARGETS_BINDINGS: &[Binding] = &[
    Binding::shown("enter", "refresh metrics", ""),
    Binding::shown("r", "range", ""),
    Binding::shown("R", "range", ""),
    Binding::shown("i", "glossary", ""),
    Binding::shown("a", "add", ""),
    Binding::shown("d", "remove", ""),
    Binding::shown("slash", "filter", ""),
    Binding::shown("space", "fold", ""),
    Binding::shown("left", "member", ""),
    Binding::shown("right", "member", ""),
    Binding::shown("escape", "back", ""),
];

/// Theses (`delta/tui/screens/theses.py::Theses.BINDINGS`).
const THESES_BINDINGS: &[Binding] = &[
    Binding::shown("n", "new", ""),
    Binding::shown("d", "edit", ""),
    Binding::shown("f", "find", ""),
    Binding::shown("s", "summarise", ""),
    Binding::shown("t", "thesis", ""),
    Binding::shown("e", "evidence", ""),
    Binding::shown("a", "accept", ""),
    Binding::shown("x", "reject", ""),
    Binding::shown("u", "un-accept", ""),
    Binding::shown("slash", "filter", ""),
    Binding::hidden("escape", "back", ""),
    Binding::hidden("shift+down", "scroll note", ""),
    Binding::hidden("shift+up", "scroll note", ""),
];

/// The help modal's keymap groups (`help.py::_binding_groups`): Anywhere
/// first, then the installed screens in name order. Home declares no
/// bindings of its own, so its group is empty and omitted by the caller.
pub fn binding_groups() -> Vec<(&'static str, Vec<(String, String)>)> {
    let groups: &[(&str, &[Binding])] = &[
        ("Anywhere", APP_BINDINGS),
        ("Ask", CHAT_BINDINGS),
        ("Settings", CONFIG_BINDINGS),
        ("Research", RESEARCH_BINDINGS),
        ("Decisions", DECISIONS_BINDINGS),
        ("Watchlist", TARGETS_BINDINGS),
        ("Theses", THESES_BINDINGS),
    ];
    groups
        .iter()
        .map(|(title, bindings)| {
            (
                *title,
                bindings
                    .iter()
                    .filter(|b| b.show)
                    .map(|b| (binding_key(b.key).to_string(), b.label.to_string()))
                    .collect(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anywhere_bindings_match_python_delta_app() {
        let keys: Vec<&str> = APP_BINDINGS.iter().map(|b| b.key).collect();
        assert_eq!(
            keys,
            vec![
                "1",
                "2",
                "3",
                "4",
                "5",
                "6",
                "c",
                "h",
                "m",
                "p",
                "g",
                "question_mark",
                "q",
                "f2"
            ]
        );
        assert!(APP_BINDINGS.iter().all(|b| b.show));
    }

    #[test]
    fn all_items_is_nav_plus_chrome() {
        let names: Vec<&str> = all_items().iter().map(|(_, name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "home",
                "targets",
                "data",
                "theses",
                "chat",
                "decisions",
                "config"
            ]
        );
    }

    #[test]
    fn key_display_maps_the_keycap_names() {
        assert_eq!(binding_key("question_mark"), "?");
        assert_eq!(binding_key("escape"), "esc");
        assert_eq!(binding_key("slash"), "/");
        assert_eq!(binding_key("n"), "n");
    }

    #[test]
    fn help_groups_cover_the_installed_screens() {
        let titles: Vec<&str> = binding_groups().into_iter().map(|(t, _)| t).collect();
        assert_eq!(
            titles,
            vec![
                "Anywhere",
                "Ask",
                "Settings",
                "Research",
                "Decisions",
                "Watchlist",
                "Theses"
            ]
        );
    }
}
