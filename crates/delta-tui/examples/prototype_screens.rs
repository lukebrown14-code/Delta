//! PROTOTYPE — throwaway. Wayfinder ticket "Screen prototype: Space screen,
//! Home and Trades" (#67, map #56). Answers "what should these screens look
//! like?" with structurally different variants over stub data. No database,
//! no services, no tests; never fold this file into the real app as is.
//!
//! Run:  cargo run -p delta-tui --example prototype_screens
//! Dump: cargo run -p delta-tui --example prototype_screens -- --dump
//!
//! Keys: 1 Space · 2 Home · 3 Trades · ←/→ cycle variant · ↑/↓ select
//!       Enter open · g go to Space · n new note · c close trade · q quit

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use delta_tui::braille::BrailleGraph;
use delta_tui::theme::{Theme, Token};
use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap};
use ratatui::{Frame, Terminal};

// ---------------------------------------------------------------- stub data

const TODAY: &str = "30 Oct";
const PREMISE: &str = "Is quantum computing worth investing in, and through which company?";

struct Member {
    sym: &'static str,
    name: &'static str,
    reason: &'static str,
    last: f64,
    m1: f64,
    prices: Vec<f64>,
    health: &'static str,
    lean: &'static str,
    lean_p: [u8; 3],
    flag: Option<&'static str>,
    attribution: Vec<(&'static str, &'static str, f64)>,
}

struct Report {
    kind: &'static str,
    subject: &'static str,
    ver: u32,
    age: u32,
    by: &'static str,
    stale: Option<&'static str>,
    withdrawn: bool,
}

#[derive(Clone)]
struct Note {
    date: String,
    pin: String,
    text: String,
}

#[derive(Clone)]
struct Trade {
    inst: &'static str,
    space: &'static str,
    entry_date: &'static str,
    entry: f64,
    stop: f64,
    target: f64,
    last: f64,
    lean: &'static str,
    regime: &'static str,
    days: u32,
    bench: f64,
    exit: Option<(String, f64, &'static str)>,
    void: bool,
    flag: Option<&'static str>,
}

impl Trade {
    fn price(&self) -> f64 {
        self.exit.as_ref().map(|e| e.1).unwrap_or(self.last)
    }
    fn r(&self) -> f64 {
        (self.price() - self.entry) / (self.entry - self.stop)
    }
    fn ret(&self) -> f64 {
        (self.price() / self.entry - 1.0) * 100.0
    }
    fn closed(&self) -> bool {
        self.exit.is_some()
    }
}

struct SpaceRow {
    name: &'static str,
    slug: &'static str,
    premise: &'static str,
    members: u8,
    bench: &'static str,
    regime: &'static str,
    regime_p: u8,
    health: &'static str,
    weakest: &'static str,
    leans: [u8; 4],
    open_trades: u8,
    latest: &'static str,
    flags: u8,
    archived: bool,
    bench_series: Vec<f64>,
}

struct Attn {
    space: &'static str,
    icon: &'static str,
    tone: Token,
    text: &'static str,
    action: &'static str,
}

struct FeedItem {
    date: &'static str,
    icon: &'static str,
    tone: Token,
    text: &'static str,
    detail: &'static str,
}

fn series(seed: u32, n: usize, start: f64, end: f64) -> Vec<f64> {
    let amp = (start + end) / 2.0 * 0.05;
    let s = seed as f64;
    let mut v: Vec<f64> = (0..n)
        .map(|i| {
            let i = i as f64;
            let t = i / (n as f64 - 1.0);
            start
                + (end - start) * t
                + amp * ((i * 0.31 + s).sin() * 0.6 + (i * 0.07 + s * 2.0).sin())
        })
        .collect();
    if let Some(last) = v.last_mut() {
        *last = end;
    }
    v
}

fn members() -> Vec<Member> {
    vec![
        Member {
            sym: "US:IONQ",
            name: "IonQ",
            reason: "Trapped-ion leader; largest pure-play revenue",
            last: 38.42,
            m1: 12.4,
            prices: series(1, 120, 24.0, 38.42),
            health: "Improving",
            lean: "Leans buy",
            lean_p: [68, 24, 8],
            flag: None,
            attribution: vec![
                ("supports", "10-Q: bookings +41% y/y", 0.91),
                ("supports", "Q3 beat, guidance raised", 0.84),
                ("opposes", "Form 4: insider sales", 0.72),
            ],
        },
        Member {
            sym: "US:RGTI",
            name: "Rigetti Computing",
            reason: "Superconducting; cheapest exposure, highest risk",
            last: 11.87,
            m1: -8.1,
            prices: series(2, 120, 9.0, 11.87),
            health: "Deteriorating",
            lean: "Leans sell",
            lean_p: [14, 25, 61],
            flag: Some("EXIT LIKELY"),
            attribution: vec![
                ("opposes", "10-Q: cash runway 6 quarters", 0.88),
                ("opposes", "Dilutive raise announced", 0.81),
                ("supports", "DARPA stage B selection", 0.66),
            ],
        },
        Member {
            sym: "US:QBTS",
            name: "D-Wave Quantum",
            reason: "Annealing; real commercial bookings",
            last: 7.95,
            m1: 3.2,
            prices: series(3, 120, 5.0, 7.95),
            health: "Steady",
            lean: "Unclear",
            lean_p: [38, 41, 21],
            flag: None,
            attribution: vec![
                ("supports", "Advantage2 general availability", 0.74),
                ("opposes", "Revenue concentration: 2 clients", 0.69),
            ],
        },
        Member {
            sym: "US:IBM",
            name: "IBM",
            reason: "Diversified hedge; quantum roadmap to 2029",
            last: 241.10,
            m1: 1.9,
            prices: series(4, 120, 205.0, 241.10),
            health: "Steady",
            lean: "Leans hold",
            lean_p: [22, 57, 21],
            flag: None,
            attribution: vec![("supports", "Heron r3 roadmap on schedule", 0.71)],
        },
    ]
}

fn reports() -> Vec<Report> {
    vec![
        Report { kind: "Landscape", subject: "Quantum computing", ver: 2, age: 12, by: "Claude Code", stale: None, withdrawn: false },
        Report { kind: "Answer", subject: "How exposed is IONQ to government contracts?", ver: 1, age: 4, by: "Codex", stale: None, withdrawn: false },
        Report { kind: "Deep dive", subject: "US:RGTI", ver: 1, age: 20, by: "Claude Code", stale: Some("new 10-Q since"), withdrawn: false },
        Report { kind: "Deep dive", subject: "US:IONQ", ver: 1, age: 34, by: "Claude Code", stale: Some("past shelf life"), withdrawn: false },
        Report { kind: "Scan", subject: "Quantum picks & shovels", ver: 1, age: 41, by: "Claude Code", stale: None, withdrawn: false },
        Report { kind: "Answer", subject: "Is QBTS revenue recurring?", ver: 1, age: 15, by: "Codex", stale: None, withdrawn: true },
    ]
}

fn notes() -> Vec<Note> {
    let n = |d: &str, p: &str, t: &str| Note { date: d.into(), pin: p.into(), text: t.into() };
    vec![
        n("28 Oct", "Space", "Main risk is timeline: revenue at scale may be 5+ years out. Keep size small."),
        n("21 Oct", "QBTS", "Lesson: took profit at target and didn't chase. Good."),
        n("14 Oct", "IONQ", "Watch gross margin, not bookings."),
        n("2 Oct", "Space", "Opening note (interview): horizon 3-5 yrs; wrong if no error-corrected logical qubits by 2028; candidates IONQ, IBM."),
    ]
}

fn trades() -> Vec<Trade> {
    let t = |inst, space, entry_date, entry, stop, target, last, lean, regime, days, bench, exit: Option<(&str, f64, &'static str)>| Trade {
        inst,
        space,
        entry_date,
        entry,
        stop,
        target,
        last,
        lean,
        regime,
        days,
        bench,
        exit: exit.map(|(d, p, r)| (d.to_string(), p, r)),
        void: false,
        flag: None,
    };
    let mut v = vec![
        t("US:IONQ", "Quantum computing", "3 Oct", 31.20, 27.50, 45.00, 38.42, "Leans buy", "Range-bound", 27, 4.1, None),
        t("US:RGTI", "Quantum computing", "10 Oct", 13.40, 12.10, 17.00, 11.87, "Leans hold", "Bull", 20, 2.2, None),
        t("ASX:PLS", "Lithium recovery", "17 Oct", 2.85, 2.55, 3.60, 3.02, "Leans buy", "Range-bound", 13, 0.8, None),
        t("US:QBTS", "Quantum computing", "6 Sep", 5.10, 4.40, 7.40, 7.95, "Leans buy", "Range-bound", 46, 6.2, Some(("21 Oct", 7.40, "target hit"))),
        t("ASX:MIN", "Lithium recovery", "2 Sep", 41.00, 37.00, 49.00, 0.0, "Leans hold", "Bear", 19, -1.8, Some(("21 Sep", 36.90, "stop hit"))),
        t("ASX:GMG", "Rate cuts & REITs", "1 Jul", 30.10, 28.00, 34.00, 0.0, "Unclear", "Range-bound", 63, 3.4, Some(("2 Sep", 33.20, "premise changed"))),
        t("ASX:IGO", "Lithium recovery", "1 Jun", 6.20, 5.60, 7.50, 0.0, "Leans sell", "Bear", 90, -2.6, Some(("30 Aug", 5.80, "time"))),
        t("US:IONQ", "Quantum computing", "12 May", 9.80, 8.30, 14.70, 0.0, "Leans buy", "Bull", 71, 7.9, Some(("22 Jul", 14.70, "target hit"))),
        t("ASX:PLS", "Lithium recovery", "4 May", 3.40, 3.08, 4.20, 0.0, "Leans buy", "Bear", 12, -3.0, Some(("16 May", 3.05, "stop hit"))),
        t("ASX:SCG", "Rate cuts & REITs", "20 Apr", 3.55, 3.30, 3.92, 0.0, "Leans buy", "Range-bound", 55, 2.1, Some(("14 Jun", 3.92, "target hit"))),
        t("US:IBM", "Quantum computing", "2 Apr", 205.0, 190.0, 240.0, 0.0, "Leans hold", "Bull", 84, 5.5, Some(("25 Jun", 228.0, "time"))),
        t("ASX:GMG", "Rate cuts & REITs", "10 Mar", 27.40, 26.20, 31.00, 0.0, "Leans buy", "Bear", 30, -4.4, Some(("9 Apr", 26.10, "stop hit"))),
    ];
    v[1].flag = Some("EXIT LIKELY");
    let mut voided = v[9].clone();
    voided.entry = 35.5;
    voided.void = true;
    v.push(voided);
    v
}

fn spaces() -> Vec<SpaceRow> {
    vec![
        SpaceRow { name: "Quantum computing", slug: "quantum", premise: PREMISE, members: 4, bench: "NASDAQ Comp", regime: "Bull", regime_p: 62, health: "Improving", weakest: "RGTI", leans: [1, 1, 1, 1], open_trades: 2, latest: "Landscape 12d", flags: 3, archived: false, bench_series: series(7, 60, 17000.0, 18900.0) },
        SpaceRow { name: "Lithium recovery", slug: "lithium", premise: "Has lithium bottomed, and which ASX miner recovers first?", members: 3, bench: "ASX 200", regime: "Range-bound", regime_p: 48, health: "Deteriorating", weakest: "IGO", leans: [1, 0, 1, 1], open_trades: 1, latest: "Deep dive PLS 3d", flags: 2, archived: false, bench_series: series(8, 60, 8200.0, 8350.0) },
        SpaceRow { name: "Rate cuts & REITs", slug: "reits", premise: "Do RBA cuts re-rate quality REITs within a year?", members: 3, bench: "ASX 200", regime: "Range-bound", regime_p: 48, health: "Steady", weakest: "CHC", leans: [1, 2, 0, 0], open_trades: 0, latest: "Landscape 70d", flags: 1, archived: false, bench_series: series(8, 60, 8200.0, 8350.0) },
        SpaceRow { name: "Uranium restart", slug: "uranium", premise: "Do reactor restarts lift uranium developers?", members: 2, bench: "ASX 200", regime: "-", regime_p: 0, health: "-", weakest: "-", leans: [0, 0, 0, 0], open_trades: 0, latest: "Concluded 14 Feb", flags: 0, archived: true, bench_series: vec![] },
    ]
}

fn attention() -> Vec<Attn> {
    vec![
        Attn { space: "Quantum computing", icon: "!", tone: Theme::WARNING, text: "RGTI stop 12.10 likely hit 27 Oct (close 11.87)", action: "record exit" },
        Attn { space: "Quantum computing", icon: "◆", tone: Theme::TEXT_WARNING, text: "Deep dive RGTI stale: new 10-Q filed 24 Oct", action: "open report" },
        Attn { space: "Quantum computing", icon: "◆", tone: Theme::TEXT_WARNING, text: "Deep dive IONQ stale: past 30-day shelf life", action: "open report" },
        Attn { space: "Lithium recovery", icon: "●", tone: Theme::TEXT_ERROR, text: "IGO lean moved Leans hold → Leans sell (61%)", action: "open reading" },
        Attn { space: "Lithium recovery", icon: "◆", tone: Theme::TEXT_SUCCESS, text: "New Deep dive PLS by Claude Code", action: "open report" },
        Attn { space: "Rate cuts & REITs", icon: "x", tone: Theme::TEXT_ERROR, text: "Source failure: yfinance_calendar for ASX:CHC", action: "open sources" },
        Attn { space: "Quantum computing", icon: "●", tone: Theme::TEXT_SUCCESS, text: "Regime NASDAQ Range-bound → Bull (62%)", action: "open reading" },
    ]
}

fn upcoming() -> Vec<FeedItem> {
    vec![
        FeedItem { date: "4 Nov", icon: "◷", tone: Theme::KIND_EVENT, text: "IONQ Q3 results (after close) · in 5 days", detail: "calendar · yfinance_calendar · confirmed" },
        FeedItem { date: "7 Nov", icon: "◷", tone: Theme::KIND_EVENT, text: "IBM ex-dividend 1.68", detail: "calendar · yfinance_calendar" },
        FeedItem { date: "12 Nov", icon: "◷", tone: Theme::KIND_EVENT, text: "RGTI Q3 results · in 13 days", detail: "calendar · yfinance_calendar · estimated" },
        FeedItem { date: "Dec", icon: "◇", tone: Theme::TEXT_SECONDARY, text: "NQI reauthorisation vote expected (per Landscape v2 Catalysts)", detail: "from report · cites [ev:3d10] · not a calendar fact" },
    ]
}

fn feed() -> Vec<FeedItem> {
    vec![
        FeedItem { date: "30 Oct", icon: "●", tone: Theme::TEXT_ERROR, text: "RGTI lean Leans hold → Leans sell (61%)", detail: "opposes: 10-Q cash runway 0.88 · dilutive raise 0.81 · jev-1.14 v3" },
        FeedItem { date: "29 Oct", icon: "!", tone: Theme::WARNING, text: "RGTI stop 12.10 crossed (close 11.87): Exit likely", detail: "c record exit, prefilled 27 Oct @ 12.10, correct to your actual fill" },
        FeedItem { date: "28 Oct", icon: "✎", tone: Theme::TEXT_SECONDARY, text: "Note: Main risk is timeline: revenue at scale may be 5+ years out.", detail: "pinned to Space · your view, never cited" },
        FeedItem { date: "26 Oct", icon: "◆", tone: Theme::TEXT_SUCCESS, text: "Answer: How exposed is IONQ to government contracts? (Codex)", detail: "3 sections · 11 citations · request: \"how much of IONQ revenue is govt?\"" },
        FeedItem { date: "24 Oct", icon: "+", tone: Theme::KIND_FILING, text: "RGTI 10-Q filed (SEC EDGAR): Deep dive RGTI now stale", detail: "evidence ev:51b0 · automatic member evidence" },
        FeedItem { date: "18 Oct", icon: "◆", tone: Theme::TEXT_SUCCESS, text: "Landscape v2 by Claude Code (supersedes v1)", detail: "10 sections · 34 citations · 6 harness sources linked" },
        FeedItem { date: "10 Oct", icon: "⇄", tone: Theme::PRIMARY, text: "Bought RGTI @ 13.40 · stop 12.10 · target 17.00", detail: "rationale: cheapest exposure if DARPA stage B lands · lean at entry Leans hold" },
        FeedItem { date: "9 Oct", icon: "●", tone: Theme::TEXT_SUCCESS, text: "Regime NASDAQ Range-bound → Bull (62%)", detail: "trend above 200d 3 months · volatility normal · macro: US10Y falling" },
        FeedItem { date: "3 Oct", icon: "⇄", tone: Theme::PRIMARY, text: "Bought IONQ @ 31.20 · stop 27.50 · target 45.00", detail: "rationale: bookings inflecting · lean at entry Leans buy" },
        FeedItem { date: "2 Oct", icon: "◆", tone: Theme::TEXT_SECONDARY, text: "Space created by Claude Code from interview · 4 members", detail: "first ingest: 2y daily bars, news, filings, fundamentals · benchmark NASDAQ Comp" },
    ]
}

// ---------------------------------------------------------------- app state

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Space,
    Home,
    Trades,
}

const SPACE_VARIANTS: [&str; 3] = ["Overview", "Focus", "Timeline"];
const HOME_VARIANTS: [&str; 2] = ["Spaces table", "Inbox"];
const TRADES_VARIANTS: [&str; 2] = ["Ledger", "Record first"];
const REASONS: [&str; 5] = ["target hit", "stop hit", "premise changed", "time", "other"];

enum Modal {
    None,
    Go { filter: String, sel: usize },
    Note { text: String, pin: usize },
    Close { trade: usize, field: usize, date: String, price: String, reason: usize, lesson: String },
}

struct App {
    screen: Screen,
    variant: [usize; 3],
    sel: usize,
    modal: Modal,
    members: Vec<Member>,
    notes: Vec<Note>,
    trades: Vec<Trade>,
    space_name: String,
    status: String,
    quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            screen: Screen::Space,
            variant: [0; 3],
            sel: 0,
            modal: Modal::None,
            members: members(),
            notes: notes(),
            trades: trades(),
            space_name: "Quantum computing".into(),
            status: String::new(),
            quit: false,
        }
    }

    fn vidx(&self) -> usize {
        self.screen as usize
    }

    fn variants(&self) -> &'static [&'static str] {
        match self.screen {
            Screen::Space => &SPACE_VARIANTS,
            Screen::Home => &HOME_VARIANTS,
            Screen::Trades => &TRADES_VARIANTS,
        }
    }

    fn open_trades(&self) -> Vec<usize> {
        (0..self.trades.len()).filter(|&i| !self.trades[i].closed() && !self.trades[i].void).collect()
    }

    fn list_len(&self) -> usize {
        match (self.screen, self.variant[self.vidx()]) {
            (Screen::Space, 0) => self.members.len(),
            (Screen::Space, 1) => self.members.len() + 1,
            (Screen::Space, _) => feed().len(),
            (Screen::Home, 0) => spaces().len(),
            (Screen::Home, _) => attention().len(),
            (Screen::Trades, _) => self.open_trades().len(),
        }
    }

    fn selected_member(&self) -> Option<&Member> {
        match (self.screen, self.variant[0]) {
            (Screen::Space, 0) => self.members.get(self.sel),
            (Screen::Space, 1) if self.sel > 0 => self.members.get(self.sel - 1),
            _ => None,
        }
    }

    fn key(&mut self, code: KeyCode) {
        match &mut self.modal {
            Modal::None => {}
            Modal::Go { filter, sel } => {
                let names: Vec<&str> = spaces()
                    .iter()
                    .map(|s| s.name)
                    .filter(|n| n.to_lowercase().contains(&filter.to_lowercase()))
                    .collect();
                match code {
                    KeyCode::Esc => self.modal = Modal::None,
                    KeyCode::Up => *sel = sel.saturating_sub(1),
                    KeyCode::Down => *sel = (*sel + 1).min(names.len().saturating_sub(1)),
                    KeyCode::Backspace => {
                        filter.pop();
                        *sel = 0;
                    }
                    KeyCode::Char(c) => {
                        filter.push(c);
                        *sel = 0;
                    }
                    KeyCode::Enter => {
                        if let Some(name) = names.get(*sel) {
                            self.space_name = name.to_string();
                            self.status = if *name == "Quantum computing" {
                                String::new()
                            } else {
                                "prototype: only 'Quantum computing' has stub data; showing it under this name".into()
                            };
                            self.screen = Screen::Space;
                            self.sel = 0;
                        }
                        self.modal = Modal::None;
                    }
                    _ => {}
                }
                return;
            }
            Modal::Note { text, pin } => {
                match code {
                    KeyCode::Esc => self.modal = Modal::None,
                    KeyCode::Tab => *pin = (*pin + 1) % (self.members.len() + 1),
                    KeyCode::Backspace => {
                        text.pop();
                    }
                    KeyCode::Char(c) => text.push(c),
                    KeyCode::Enter if !text.trim().is_empty() => {
                        let pin_label = if *pin == 0 {
                            "Space".to_string()
                        } else {
                            short(self.members[*pin - 1].sym).to_string()
                        };
                        self.notes.insert(0, Note { date: TODAY.into(), pin: pin_label, text: text.clone() });
                        self.status = "note saved (your view, never cited)".into();
                        self.modal = Modal::None;
                    }
                    _ => {}
                }
                return;
            }
            Modal::Close { trade, field, date, price, reason, lesson } => {
                match code {
                    KeyCode::Esc => self.modal = Modal::None,
                    KeyCode::Up => *field = field.saturating_sub(1),
                    KeyCode::Down | KeyCode::Tab => *field = (*field + 1).min(3),
                    KeyCode::Left if *field == 2 => *reason = (*reason + REASONS.len() - 1) % REASONS.len(),
                    KeyCode::Right if *field == 2 => *reason = (*reason + 1) % REASONS.len(),
                    KeyCode::Backspace => {
                        match field {
                            0 => date.pop(),
                            1 => price.pop(),
                            3 => lesson.pop(),
                            _ => None,
                        };
                    }
                    KeyCode::Char(c) => match field {
                        0 => date.push(c),
                        1 => price.push(c),
                        3 => lesson.push(c),
                        _ => {}
                    },
                    KeyCode::Enter => {
                        let t = &mut self.trades[*trade];
                        let px = price.parse().unwrap_or(t.last);
                        t.exit = Some((date.clone(), px, REASONS[*reason]));
                        t.flag = None;
                        if !lesson.trim().is_empty() {
                            self.notes.insert(0, Note { date: TODAY.into(), pin: short(t.inst).into(), text: format!("Lesson: {lesson}") });
                        }
                        self.status = format!("closed {} at {px:.2}: {:+.2}R, JEV snapshot taken at exit", short(t.inst), t.r());
                        self.sel = 0;
                        self.modal = Modal::None;
                    }
                    _ => {}
                }
                return;
            }
        }
        let len = self.list_len();
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('1') => self.switch(Screen::Space),
            KeyCode::Char('2') => self.switch(Screen::Home),
            KeyCode::Char('3') => self.switch(Screen::Trades),
            KeyCode::Left | KeyCode::Right => {
                let n = self.variants().len();
                let v = &mut self.variant[self.screen as usize];
                *v = if code == KeyCode::Right { (*v + 1) % n } else { (*v + n - 1) % n };
                self.sel = 0;
            }
            KeyCode::Up => self.sel = self.sel.saturating_sub(1),
            KeyCode::Down => self.sel = (self.sel + 1).min(len.saturating_sub(1)),
            KeyCode::Char('v') if self.screen == Screen::Space => {
                self.variant[0] = (self.variant[0] + 1) % SPACE_VARIANTS.len();
            }
            KeyCode::Char('g') => self.modal = Modal::Go { filter: String::new(), sel: 0 },
            KeyCode::Char('n') if self.screen == Screen::Space => {
                let pin = match (self.variant[0], self.sel) {
                    (0, s) => s + 1,
                    (1, s) => s,
                    _ => 0,
                };
                self.modal = Modal::Note { text: String::new(), pin };
            }
            KeyCode::Char('c') if self.screen == Screen::Trades => {
                if let Some(&i) = self.open_trades().get(self.sel) {
                    let t = &self.trades[i];
                    let (date, price, reason) = if t.flag.is_some() {
                        ("27 Oct".to_string(), format!("{:.2}", t.stop), 1)
                    } else {
                        (TODAY.to_string(), format!("{:.2}", t.last), 4)
                    };
                    self.modal = Modal::Close { trade: i, field: 1, date, price, reason, lesson: String::new() };
                }
            }
            KeyCode::Enter if self.screen == Screen::Home => {
                let name = if self.variant[1] == 0 { spaces()[self.sel].name } else { attention()[self.sel].space };
                self.space_name = name.into();
                self.switch(Screen::Space);
            }
            _ => {}
        }
    }

    fn switch(&mut self, s: Screen) {
        self.screen = s;
        self.sel = 0;
    }
}

// ---------------------------------------------------------------- helpers

fn fg(t: Token) -> Style {
    Style::default().fg(t.color())
}

fn muted() -> Style {
    fg(Theme::TEXT_MUTED)
}

fn bold(t: Token) -> Style {
    fg(t).add_modifier(Modifier::BOLD)
}

fn short(sym: &str) -> &str {
    sym.split(':').nth(1).unwrap_or(sym)
}

fn panel(title: &str) -> Block<'static> {
    Block::bordered()
        .title(Span::styled(format!(" {title} "), bold(Theme::TEXT_SECONDARY)))
        .border_style(fg(Theme::BORDER_BLURRED))
}

fn lean_style(lean: &str) -> Style {
    if lean.contains("buy") {
        fg(Theme::TEXT_SUCCESS)
    } else if lean.contains("sell") {
        fg(Theme::TEXT_ERROR)
    } else if lean == "Unclear" {
        muted()
    } else {
        fg(Theme::TEXT_SECONDARY)
    }
}

fn lean_glyph(lean: &str) -> &'static str {
    if lean.contains("buy") {
        "▲"
    } else if lean.contains("sell") {
        "▼"
    } else if lean == "Unclear" {
        "?"
    } else {
        "■"
    }
}

fn health_style(h: &str) -> Style {
    match h {
        "Improving" => fg(Theme::TEXT_SUCCESS),
        "Deteriorating" => fg(Theme::TEXT_ERROR),
        _ => fg(Theme::TEXT_SECONDARY),
    }
}

fn regime_style(r: &str) -> Style {
    match r {
        "Bull" => fg(Theme::TEXT_SUCCESS),
        "Bear" => fg(Theme::TEXT_ERROR),
        _ => fg(Theme::TEXT_SECONDARY),
    }
}

fn signed(x: f64) -> Style {
    if x >= 0.0 {
        fg(Theme::TEXT_SUCCESS)
    } else {
        fg(Theme::TEXT_ERROR)
    }
}

fn spark(data: &[f64], w: usize, h: usize) -> Vec<Line<'static>> {
    let up = data.last().copied().unwrap_or(0.0) >= data.first().copied().unwrap_or(0.0);
    let style = if up { fg(Theme::TEXT_SUCCESS) } else { fg(Theme::TEXT_ERROR) };
    BrailleGraph::new(data.to_vec())
        .rows(w, h)
        .into_iter()
        .map(|r| Line::from(Span::styled(r, style)))
        .collect()
}

fn tag() -> Span<'static> {
    Span::styled(" INDICATOR · NOT ADVICE ", Style::default().fg(Color::Black).bg(Theme::WARNING.color()).add_modifier(Modifier::BOLD))
}

fn chip(text: &str, t: Token) -> Span<'static> {
    Span::styled(format!(" {text} "), Style::default().fg(Color::Black).bg(t.color()).add_modifier(Modifier::BOLD))
}

fn sel_style(on: bool) -> Style {
    if on {
        Style::default().bg(Theme::PANEL.color()).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

fn regime_series() -> Vec<f64> {
    (0..30).map(|i| 0.35 + 0.27 * (i as f64 / 29.0) + 0.03 * (i as f64 * 0.9).sin()).collect()
}

// ---------------------------------------------------------------- shared pieces

fn space_header(f: &mut Frame, area: Rect, app: &App) {
    let lines = vec![
        Line::from(vec![
            Span::styled(app.space_name.clone(), bold(Theme::TEXT_PRIMARY)),
            Span::styled("  quantum  ", muted()),
            chip("OPEN", Theme::SUCCESS),
            Span::raw(" "),
            chip("INVESTED", Theme::WARNING),
            Span::styled("   related: Semiconductor capex", muted()),
        ]),
        Line::from(vec![Span::styled("Premise  ", muted()), Span::styled(PREMISE, fg(Theme::TEXT_PRIMARY))]),
        Line::from(vec![
            Span::styled("Benchmark ", muted()),
            Span::raw("NASDAQ Comp  "),
            Span::styled("Regime ", muted()),
            Span::styled("▲ Bull 62%", bold(Theme::TEXT_SUCCESS)),
            Span::styled(" (Range-bound until 9 Oct)  ", muted()),
            Span::styled("Health ", muted()),
            Span::styled("Improving", health_style("Improving")),
            Span::styled(", weakest RGTI ▼  ", muted()),
            tag(),
        ]),
    ];
    f.render_widget(Paragraph::new(lines).block(Block::new().borders(Borders::BOTTOM).border_style(fg(Theme::BORDER_BLURRED))), area);
}

fn readings_lines(m: Option<&Member>) -> Vec<Line<'static>> {
    let mut l = vec![
        Line::from(Span::styled("Regime · NASDAQ Comp", muted())),
        {
            let mut spans = vec![Span::styled("▲ Bull 62%  ", bold(Theme::TEXT_SUCCESS))];
            spans.extend(spark(&regime_series(), 14, 1).remove(0).spans);
            spans.push(Span::styled("  conf 0.41", muted()));
            Line::from(spans)
        },
        Line::from(Span::styled("  Range-bound → Bull on 9 Oct", muted())),
        Line::raw(""),
        Line::from(Span::styled("Health · Space roll-up", muted())),
        Line::from(vec![Span::styled("Improving", health_style("Improving")), Span::styled("  weakest RGTI deteriorating", muted())]),
        Line::raw(""),
    ];
    if let Some(m) = m {
        l.push(Line::from(Span::styled(format!("Lean · {} in this Space", short(m.sym)), muted())));
        l.push(Line::from(vec![
            Span::styled(format!("{} {}  ", lean_glyph(m.lean), m.lean), lean_style(m.lean).add_modifier(Modifier::BOLD)),
            Span::styled(format!("buy {} · hold {} · sell {}", m.lean_p[0], m.lean_p[1], m.lean_p[2]), muted()),
        ]));
        for (side, text, p) in &m.attribution {
            let style = if *side == "supports" { fg(Theme::TEXT_SUCCESS) } else { fg(Theme::TEXT_ERROR) };
            l.push(Line::from(vec![Span::styled(format!("  {side:<9}"), style), Span::raw(format!("{text:<30}")), Span::styled(format!("{p:.2}"), muted())]));
        }
        l.push(Line::raw(""));
    }
    l.push(Line::from(vec![tag(), Span::styled("  jev-1.14 · v3 · 30 Oct 09:12", muted())]));
    l
}

fn report_lines(width: usize) -> Vec<Line<'static>> {
    reports()
        .iter()
        .filter(|r| !r.withdrawn)
        .map(|r| {
            let mut spans = vec![
                Span::styled(format!("{:<10}", r.kind), bold(Theme::TEXT_SECONDARY)),
                Span::raw(format!("{:<w$}", truncate(&r.subject.replace("US:", ""), width.saturating_sub(30)), w = width.saturating_sub(30))),
                Span::styled(format!(" v{} {:>3}d ", r.ver, r.age), muted()),
            ];
            if r.stale.is_some() {
                spans.push(chip("STALE", Theme::WARNING));
            }
            Line::from(spans)
        })
        .chain(std::iter::once(Line::from(Span::styled("1 withdrawn · h history", muted()))))
        .collect()
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>())
    }
}

fn note_lines(app: &App, pin: Option<&str>) -> Vec<Line<'static>> {
    let mut l = Vec::new();
    for n in app.notes.iter().filter(|n| pin.is_none_or(|p| n.pin == p)) {
        l.push(Line::from(vec![Span::styled(format!("{} · {} ", n.date, n.pin), muted()), Span::raw(n.text.clone())]));
    }
    l.push(Line::from(Span::styled("n new note", muted())));
    l
}

fn space_trade_lines(app: &App, inst: Option<&str>) -> Vec<Line<'static>> {
    let mut l = Vec::new();
    for t in app.trades.iter().filter(|t| t.space == "Quantum computing" && !t.void && inst.is_none_or(|i| t.inst == i)) {
        let mut spans = vec![
            Span::styled(format!("{:<5}", short(t.inst)), bold(Theme::TEXT_PRIMARY)),
            Span::styled(format!("{:>7.1}% ", t.ret()), signed(t.ret())),
            Span::styled(format!("{:+.2}R ", t.r()), signed(t.r())),
        ];
        match (&t.exit, t.flag) {
            (Some((d, _, why)), _) => spans.push(Span::styled(format!("closed {d} · {why}"), muted())),
            (None, Some(flag)) => spans.push(chip(flag, Theme::WARNING)),
            (None, None) => spans.push(Span::styled(format!("open {}d", t.days), fg(Theme::TEXT_SECONDARY))),
        }
        l.push(Line::from(spans));
    }
    if l.is_empty() {
        l.push(Line::from(Span::styled("no trades", muted())));
    }
    l
}

// ---------------------------------------------------------------- Space variants

fn space_cockpit(f: &mut Frame, area: Rect, app: &App) {
    let [head, top, bottom] = Layout::vertical([Constraint::Length(4), Constraint::Percentage(55), Constraint::Min(0)]).areas(area);
    space_header(f, head, app);
    let [left, right] = Layout::horizontal([Constraint::Percentage(62), Constraint::Min(0)]).areas(top);
    let rows: Vec<Row> = app
        .members
        .iter()
        .enumerate()
        .map(|(i, m)| {
            Row::new(vec![
                Cell::from(Line::from(vec![Span::styled(format!("{:<5} ", short(m.sym)), bold(Theme::TEXT_PRIMARY)), Span::styled(m.name, muted())])),
                Cell::from(format!("{:>8.2}", m.last)),
                Cell::from(Span::styled(format!("{:>+6.1}%", m.m1), signed(m.m1))),
                Cell::from(spark(&m.prices, 14, 1).remove(0)),
                Cell::from(Span::styled(m.health, health_style(m.health))),
                Cell::from(Span::styled(format!("{} {}", lean_glyph(m.lean), m.lean), lean_style(m.lean))),
                Cell::from(m.flag.map(|f| chip(f, Theme::WARNING)).unwrap_or_else(|| Span::raw(""))),
            ])
            .style(sel_style(i == app.sel))
        })
        .collect();
    let table = Table::new(
        rows,
        [Constraint::Length(26), Constraint::Length(8), Constraint::Length(7), Constraint::Length(14), Constraint::Length(13), Constraint::Length(12), Constraint::Min(0)],
    )
    .header(Row::new(["Member", "Last", "1M", "3M", "Health", "Lean", ""]).style(muted()))
    .block(panel("Members"));
    f.render_widget(table, left);
    f.render_widget(Paragraph::new(readings_lines(app.selected_member())).block(panel("Readings")), right);
    let [rep, notes, trades] = Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(30), Constraint::Min(0)]).areas(bottom);
    f.render_widget(Paragraph::new(report_lines(rep.width as usize - 2)).block(panel("Reports")), rep);
    f.render_widget(Paragraph::new(note_lines(app, None)).wrap(Wrap { trim: true }).block(panel("Notes")), notes);
    f.render_widget(Paragraph::new(space_trade_lines(app, None)).block(panel("Trades")), trades);
}

fn space_master_detail(f: &mut Frame, area: Rect, app: &App) {
    let [list, detail] = Layout::horizontal([Constraint::Length(30), Constraint::Min(0)]).areas(area);
    let mut items = vec![Line::from(Span::styled(format!("◆ {}", app.space_name), bold(Theme::TEXT_PRIMARY))).style(sel_style(app.sel == 0))];
    items.push(Line::from(Span::styled("  Members", muted())));
    for (i, m) in app.members.iter().enumerate() {
        let mut spans = vec![
            Span::styled(format!("  {} ", lean_glyph(m.lean)), lean_style(m.lean)),
            Span::styled(format!("{:<5}", short(m.sym)), fg(Theme::TEXT_PRIMARY)),
            Span::styled(format!("{:>8.2} ", m.last), muted()),
            Span::styled(format!("{:+.1}%", m.m1), signed(m.m1)),
        ];
        if m.flag.is_some() {
            spans.push(Span::styled(" !", bold(Theme::WARNING)));
        }
        items.push(Line::from(spans).style(sel_style(app.sel == i + 1)));
    }
    items.push(Line::raw(""));
    items.push(Line::from(Span::styled("  Related", muted())));
    items.push(Line::from(Span::raw("  Semiconductor capex")));
    items.push(Line::raw(""));
    items.push(Line::from(Span::styled("  ↑↓ select · n note", muted())));
    f.render_widget(Paragraph::new(items).block(panel("Space")), list);

    match app.selected_member() {
        None => {
            let [top, body] = Layout::vertical([Constraint::Length(7), Constraint::Min(0)]).areas(detail);
            let head = vec![
                Line::from(vec![Span::styled(app.space_name.clone(), bold(Theme::TEXT_PRIMARY)), Span::raw("  "), chip("OPEN", Theme::SUCCESS), Span::raw(" "), chip("INVESTED", Theme::WARNING)]),
                Line::from(vec![Span::styled("Premise  ", muted()), Span::raw(PREMISE)]),
                Line::from(Span::styled("         2 Oct: \"Is quantum computing investable?\" (edited 18 Oct)", muted())),
                Line::from(vec![
                    Span::styled("Regime ", muted()),
                    Span::styled("▲ Bull 62%", bold(Theme::TEXT_SUCCESS)),
                    Span::styled("  Health ", muted()),
                    Span::styled("Improving", health_style("Improving")),
                    Span::styled(", weakest RGTI  ", muted()),
                    tag(),
                ]),
                Line::from(vec![Span::styled("Benchmark NASDAQ Comp · 4 members · 2 open trades · 5 reports · 4 notes", muted())]),
            ];
            f.render_widget(Paragraph::new(head).block(Block::new().borders(Borders::BOTTOM).border_style(fg(Theme::BORDER_BLURRED))), top);
            let [doc, side] = Layout::horizontal([Constraint::Percentage(64), Constraint::Min(0)]).areas(body);
            let ev = |s: &'static str| Span::styled(s, muted());
            let h = |s: &'static str| Line::from(Span::styled(s, bold(Theme::PRIMARY)));
            let doc_lines = vec![
                Line::from(vec![Span::styled("Landscape report v2", bold(Theme::TEXT_PRIMARY)), Span::styled(" · Claude Code · 18 Oct · fresh", muted())]),
                Line::raw(""),
                h("Summary"),
                Line::from(vec![Span::raw("Quantum is moving from lab to early revenue; IONQ leads on bookings while small-cap cash runways are the weak point. "), ev("[ev:8f2c] [ev:51b0]")]),
                h("Landscape"),
                Line::from(vec![Span::raw("~US$1.4B 2026 revenue across listed players; government is the largest buyer. "), ev("[ev:3d10]")]),
                h("Drivers"),
                Line::from(vec![Span::raw("• US National Quantum Initiative reauthorisation "), ev("[ev:3d10]")]),
                Line::from(vec![Span::raw("• Error-correction milestones (logical qubits) "), ev("[ev:77a2]")]),
                h("Trends"),
                Line::from(vec![Span::raw("• IONQ bookings +41% y/y, Q3 2026 "), ev("[ev:8f2c]")]),
                Line::from(vec![Span::raw("• Dilutive raises across small caps, 2025-26 "), ev("[ev:2e77]")]),
                h("Players"),
                Line::from(Span::raw("IONQ, RGTI, QBTS, IBM (members) · Quantinuum, PsiQuantum (unlisted)")),
                Line::from(Span::styled("Bull · Bear · Catalysts · Risks · Unknowns   (Enter expands)", muted())),
            ];
            f.render_widget(Paragraph::new(doc_lines).wrap(Wrap { trim: true }).block(panel("Latest report")), doc);
            let [r, n] = Layout::vertical([Constraint::Length(9), Constraint::Min(0)]).areas(side);
            f.render_widget(Paragraph::new(report_lines(side.width as usize - 2)).block(panel("Reports")), r);
            f.render_widget(Paragraph::new(note_lines(app, Some("Space"))).wrap(Wrap { trim: true }).block(panel("Notes on Space")), n);
        }
        Some(m) => {
            let [top, chart, body] = Layout::vertical([Constraint::Length(3), Constraint::Length(10), Constraint::Min(0)]).areas(detail);
            let head = vec![
                Line::from(vec![
                    Span::styled(format!("{}  {}", m.sym, m.name), bold(Theme::TEXT_PRIMARY)),
                    Span::raw(format!("   {:.2} ", m.last)),
                    Span::styled(format!("{:+.1}% 1M", m.m1), signed(m.m1)),
                ]),
                Line::from(vec![Span::styled("Why it's here  ", muted()), Span::raw(m.reason)]),
            ];
            f.render_widget(Paragraph::new(head), top);
            let mut chart_lines = spark(&m.prices, chart.width as usize - 14, 8);
            if let Some(t) = app.trades.iter().find(|t| t.inst == m.sym && !t.closed()) {
                chart_lines.insert(0, Line::from(Span::styled(format!("entry {:.2} · stop {:.2} · target {:.2}", t.entry, t.stop, t.target), muted())));
            }
            f.render_widget(Paragraph::new(chart_lines).block(panel("6M price")), chart);
            let [read, rest] = Layout::horizontal([Constraint::Percentage(50), Constraint::Min(0)]).areas(body);
            f.render_widget(Paragraph::new(readings_lines(Some(m))).block(panel("Readings")), read);
            let mut lines = vec![Line::from(Span::styled("Deep dive", muted()))];
            match reports().iter().find(|r| r.kind == "Deep dive" && r.subject == m.sym) {
                Some(r) => lines.push(Line::from(vec![
                    Span::raw(format!("v{} · {} · {}d ", r.ver, r.by, r.age)),
                    r.stale.map(|s| chip(&format!("STALE: {s}"), Theme::WARNING)).unwrap_or_else(|| Span::raw("")),
                ])),
                None => lines.push(Line::from(Span::styled("none yet: ask the harness for one", muted()))),
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled("Trades", muted())));
            lines.extend(space_trade_lines(app, Some(m.sym)));
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled("Notes", muted())));
            lines.extend(note_lines(app, Some(short(m.sym))));
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).block(panel(short(m.sym))), rest);
        }
    }
}

fn space_timeline(f: &mut Frame, area: Rect, app: &App) {
    let [head, body] = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
    let lines = vec![
        Line::from(vec![
            Span::styled(app.space_name.clone(), bold(Theme::TEXT_PRIMARY)),
            Span::raw("  "),
            chip("OPEN", Theme::SUCCESS),
            Span::raw(" "),
            chip("INVESTED", Theme::WARNING),
            Span::styled(format!("   {PREMISE}"), fg(Theme::TEXT_SECONDARY)),
        ]),
        Line::from(Span::styled("Everything that happened in this Space, newest first. ↑↓ select · Enter open · n note", muted())),
    ];
    f.render_widget(Paragraph::new(lines).block(Block::new().borders(Borders::BOTTOM).border_style(fg(Theme::BORDER_BLURRED))), head);
    let [left, right] = Layout::horizontal([Constraint::Percentage(68), Constraint::Min(0)]).areas(body);
    let mut feed_lines = vec![Line::from(Span::styled("Upcoming", bold(Theme::TEXT_SECONDARY)))];
    for it in upcoming() {
        feed_lines.push(Line::from(vec![
            Span::styled(format!("{:<7}", it.date), fg(Theme::KIND_EVENT)),
            Span::styled(format!("{} ", it.icon), bold(it.tone)),
            Span::raw(it.text),
        ]));
    }
    feed_lines.push(Line::from(Span::styled(format!("── today {TODAY} ─────────────────────────────────────────────"), muted())));
    let mut prev = "";
    for (i, it) in feed().iter().enumerate() {
        let date = if it.date == prev { "" } else { it.date };
        prev = it.date;
        feed_lines.push(
            Line::from(vec![
                Span::styled(format!("{date:<7}"), muted()),
                Span::styled(format!("{} ", it.icon), bold(it.tone)),
                Span::raw(it.text),
            ])
            .style(sel_style(i == app.sel)),
        );
        if i == app.sel {
            feed_lines.push(Line::from(Span::styled(format!("         {}", it.detail), fg(Theme::TEXT_SECONDARY))).style(sel_style(true)));
        }
    }
    f.render_widget(Paragraph::new(feed_lines).block(panel("Timeline")), left);
    let mut now = vec![
        Line::from(vec![Span::styled("Regime ", muted()), Span::styled("▲ Bull 62%", bold(Theme::TEXT_SUCCESS))]),
        Line::from(vec![Span::styled("Health ", muted()), Span::styled("Improving", health_style("Improving")), Span::styled(" · weakest RGTI", muted())]),
        Line::from(tag()),
        Line::raw(""),
    ];
    for m in &app.members {
        now.push(Line::from(vec![
            Span::styled(format!("{} ", lean_glyph(m.lean)), lean_style(m.lean)),
            Span::styled(format!("{:<5}", short(m.sym)), fg(Theme::TEXT_PRIMARY)),
            Span::raw(format!("{:>8.2} ", m.last)),
        ]));
        now.push(spark(&m.prices, 18, 1).remove(0));
    }
    now.push(Line::raw(""));
    now.push(Line::from(Span::styled("Open trades", muted())));
    now.extend(space_trade_lines(app, None).into_iter().take(2));
    f.render_widget(Paragraph::new(now).block(panel("Now")), right);
}

// ---------------------------------------------------------------- Home variants

fn home_table(f: &mut Frame, area: Rect, app: &App) {
    let [head, attn, table] = Layout::vertical([Constraint::Length(2), Constraint::Length(9), Constraint::Min(0)]).areas(area);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Delta", bold(Theme::PRIMARY)),
            Span::styled(format!("  {TODAY} 2026 · since you were away (2 days): 7 changes · harness last wrote 3h ago (Claude Code)"), muted()),
        ])),
        head,
    );
    let lines: Vec<Line> = attention()
        .iter()
        .map(|a| {
            Line::from(vec![
                Span::styled(format!("{} ", a.icon), bold(a.tone)),
                Span::styled(format!("{:<19}", a.space), muted()),
                Span::raw(a.text),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(panel("Needs attention")), attn);
    let rows: Vec<Row> = spaces()
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let dim = if s.archived { muted() } else { Style::default() };
            Row::new(vec![
                Cell::from(Span::styled(s.name, if s.archived { muted() } else { bold(Theme::TEXT_PRIMARY) })),
                Cell::from(Span::styled(truncate(s.premise, 34), fg(Theme::TEXT_SECONDARY))),
                Cell::from(format!("{}", s.members)),
                Cell::from(Line::from(vec![Span::styled(format!("{} ", s.regime), regime_style(s.regime)), Span::styled(if s.regime_p > 0 { format!("{}%", s.regime_p) } else { String::new() }, muted())])),
                Cell::from(Line::from(vec![Span::styled(s.health, health_style(s.health)), Span::styled(if s.archived { String::new() } else { format!(" {}", s.weakest) }, muted())])),
                Cell::from(Line::from(vec![
                    Span::styled(format!("▲{} ", s.leans[0]), fg(Theme::TEXT_SUCCESS)),
                    Span::styled(format!("■{} ", s.leans[1]), fg(Theme::TEXT_SECONDARY)),
                    Span::styled(format!("▼{} ", s.leans[2]), fg(Theme::TEXT_ERROR)),
                    Span::styled(format!("?{}", s.leans[3]), muted()),
                ])),
                Cell::from(if s.open_trades > 0 { format!("{} open", s.open_trades) } else { String::new() }),
                Cell::from(Span::styled(s.latest, muted())),
                Cell::from(if s.flags > 0 { chip(&format!("{}", s.flags), Theme::WARNING) } else { Span::raw(if s.archived { "archived" } else { "" }) }),
            ])
            .style(sel_style(i == app.sel).patch(dim))
        })
        .collect();
    let t = Table::new(
        rows,
        [
            Constraint::Length(19),
            Constraint::Length(35),
            Constraint::Length(3),
            Constraint::Length(16),
            Constraint::Length(19),
            Constraint::Length(12),
            Constraint::Length(7),
            Constraint::Length(17),
            Constraint::Min(0),
        ],
    )
    .header(Row::new(["Space", "Premise", "#", "Regime", "Health", "Leans", "Trades", "Latest report", "!"]).style(muted()))
    .block(panel("Spaces · Enter open · g go"));
    f.render_widget(t, table);
}

fn home_inbox(f: &mut Frame, area: Rect, app: &App) {
    let [inbox, side] = Layout::horizontal([Constraint::Percentage(66), Constraint::Min(0)]).areas(area);
    let mut lines = vec![Line::from(Span::styled("Since you were away (2 days) · newest first · Enter acts", muted())), Line::raw("")];
    let all = attention();
    let mut order: Vec<usize> = (0..all.len()).collect();
    order.sort_by_key(|&i| spaces().iter().position(|s| s.name == all[i].space));
    let mut prev = "";
    for i in order {
        let a = &all[i];
        if a.space != prev {
            if !prev.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(Span::styled(a.space, bold(Theme::TEXT_PRIMARY))));
            prev = a.space;
        }
        lines.push(
            Line::from(vec![
                Span::styled(format!("  {} ", a.icon), bold(a.tone)),
                Span::raw(format!("{:<50}", a.text)),
                Span::styled(format!("→ {}", a.action), fg(Theme::FOOTER_KEY)),
            ])
            .style(sel_style(i == app.sel)),
        );
    }
    f.render_widget(Paragraph::new(lines).block(panel("Inbox")), inbox);
    let mut s = Vec::new();
    for sp in spaces().iter().filter(|s| !s.archived) {
        s.push(Line::from(Span::styled(sp.name, bold(Theme::TEXT_PRIMARY))));
        s.push(Line::from(vec![
            Span::styled(format!("{} {}% ", sp.regime, sp.regime_p), regime_style(sp.regime)),
            Span::styled(sp.health, health_style(sp.health)),
            Span::styled(format!(" · {} open", sp.open_trades), muted()),
        ]));
        s.push(Line::from(vec![Span::styled(format!("{:<12}", sp.bench), muted())]));
        s.extend(spark(&sp.bench_series, 26, 1));
        s.push(Line::raw(""));
    }
    s.push(Line::from(Span::styled("Archived: Uranium restart", muted())));
    s.push(Line::raw(""));
    s.push(Line::from(tag()));
    f.render_widget(Paragraph::new(s).block(panel("Spaces")), side);
}

// ---------------------------------------------------------------- Trades variants

struct Stats {
    n: usize,
    win: f64,
    avg_win: f64,
    avg_loss: f64,
    exp: f64,
}

fn stats<'a>(ts: impl Iterator<Item = &'a Trade>) -> Stats {
    let rs: Vec<f64> = ts.filter(|t| t.closed() && !t.void).map(Trade::r).collect();
    let n = rs.len();
    let wins: Vec<f64> = rs.iter().copied().filter(|r| *r > 0.0).collect();
    let losses: Vec<f64> = rs.iter().copied().filter(|r| *r <= 0.0).collect();
    let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    Stats {
        n,
        win: if n == 0 { 0.0 } else { wins.len() as f64 / n as f64 * 100.0 },
        avg_win: mean(&wins),
        avg_loss: mean(&losses),
        exp: mean(&rs),
    }
}

fn breakdown(app: &App, title: &str, keys: &[&'static str], key: fn(&Trade) -> &'static str) -> Table<'static> {
    let rows: Vec<Row> = keys
        .iter()
        .map(|k| {
            let s = stats(app.trades.iter().filter(|t| key(t) == *k));
            let cells = if s.n < 5 {
                vec![Cell::from(Span::styled(*k, fg(Theme::TEXT_PRIMARY))), Cell::from(format!("{}", s.n)), Cell::from(Span::styled("too few trades", muted())), Cell::from("")]
            } else {
                vec![
                    Cell::from(Span::styled(*k, fg(Theme::TEXT_PRIMARY))),
                    Cell::from(format!("{}", s.n)),
                    Cell::from(format!("{:.0}% win", s.win)),
                    Cell::from(Span::styled(format!("{:+.2}R", s.exp), signed(s.exp))),
                ]
            };
            Row::new(cells)
        })
        .collect();
    Table::new(rows, [Constraint::Length(19), Constraint::Length(3), Constraint::Length(15), Constraint::Min(0)])
        .header(Row::new(["", "n", "", "exp."]).style(muted()))
        .block(panel(title))
}

fn trade_row(t: &Trade, open: bool, selected: bool) -> Row<'static> {
    let mut cells = vec![
        Cell::from(Span::styled(short(t.inst).to_string(), bold(if t.void { Theme::TEXT_MUTED } else { Theme::TEXT_PRIMARY }))),
        Cell::from(Span::styled(truncate(t.space, 11), muted())),
        Cell::from(format!("{} @ {:.2}", t.entry_date, t.entry)),
    ];
    if open {
        cells.push(Cell::from(format!("{:.2}", t.last)));
        cells.push(Cell::from(format!("{:.2} / {:.2}", t.stop, t.target)));
    } else {
        let (d, p, why) = t.exit.clone().unwrap_or_default();
        cells.push(Cell::from(format!("{d} @ {p:.2}")));
        cells.push(Cell::from(Span::styled(if t.void { "VOID (typo)" } else { why }, muted())));
    }
    cells.push(Cell::from(Span::styled(format!("{:+.1}%", t.ret()), signed(t.ret()))));
    cells.push(Cell::from(Span::styled(format!("{:+.2}R", t.r()), signed(t.r()))));
    cells.push(Cell::from(if open { format!("{}d", t.days) } else { format!("{:+.1}%", t.ret() - t.bench) }));
    cells.push(Cell::from(Span::styled(t.lean, lean_style(t.lean))));
    cells.push(Cell::from(match (t.void, t.flag) {
        (true, _) => Span::styled("void", muted()),
        (false, Some(flag)) => chip(flag, Theme::WARNING),
        _ => Span::raw(""),
    }));
    Row::new(cells).style(sel_style(selected).patch(if t.void { muted().add_modifier(Modifier::CROSSED_OUT) } else { Style::default() }))
}

const TRADE_WIDTHS: [Constraint; 10] = [
    Constraint::Length(6),
    Constraint::Length(12),
    Constraint::Length(15),
    Constraint::Length(15),
    Constraint::Length(16),
    Constraint::Length(7),
    Constraint::Length(7),
    Constraint::Length(7),
    Constraint::Length(11),
    Constraint::Min(0),
];

fn trades_ledger(f: &mut Frame, area: Rect, app: &App) {
    let open = app.open_trades();
    let [top, bottom] = Layout::vertical([Constraint::Length(open.len() as u16 + 3), Constraint::Min(0)]).areas(area);
    let rows: Vec<Row> = open.iter().enumerate().map(|(i, &t)| trade_row(&app.trades[t], true, i == app.sel)).collect();
    f.render_widget(
        Table::new(rows, TRADE_WIDTHS)
            .header(Row::new(["", "Space", "Entry", "Last", "Stop / target", "Ret", "R", "Held", "Lean@entry", ""]).style(muted()))
            .block(panel("Open · ↑↓ select · c close")),
        top,
    );
    let [closed, record] = Layout::horizontal([Constraint::Percentage(72), Constraint::Min(0)]).areas(bottom);
    let rows: Vec<Row> = app.trades.iter().filter(|t| t.closed()).map(|t| trade_row(t, false, false)).collect();
    f.render_widget(
        Table::new(rows, TRADE_WIDTHS)
            .header(Row::new(["", "Space", "Entry", "Exit", "Reason", "Ret", "R", "vs bench", "Lean@entry", ""]).style(muted()))
            .block(panel("Closed")),
        closed,
    );
    let s = stats(app.trades.iter());
    let mut lines = vec![
        Line::from(vec![Span::styled(format!("{} closed  ", s.n), bold(Theme::TEXT_PRIMARY)), Span::raw(format!("{:.0}% win  ", s.win)), Span::styled(format!("exp {:+.2}R", s.exp), signed(s.exp))]),
        Line::from(Span::styled(format!("avg win {:+.2}R · avg loss {:+.2}R", s.avg_win, s.avg_loss), muted())),
        Line::raw(""),
        Line::from(Span::styled("By lean at entry", muted())),
    ];
    for k in ["Leans buy", "Leans hold", "Leans sell", "Unclear"] {
        let b = stats(app.trades.iter().filter(|t| t.lean == k));
        lines.push(Line::from(vec![
            Span::styled(format!("{k:<11}"), lean_style(k)),
            if b.n < 5 {
                Span::styled(format!("n={} too few trades", b.n), muted())
            } else {
                Span::styled(format!("n={} {:.0}% win {:+.2}R", b.n, b.win, b.exp), signed(b.exp))
            },
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("By regime at entry", muted())));
    for k in ["Bull", "Range-bound", "Bear"] {
        let b = stats(app.trades.iter().filter(|t| t.regime == k));
        lines.push(Line::from(vec![
            Span::styled(format!("{k:<11}"), regime_style(k)),
            if b.n < 5 { Span::styled(format!("n={} too few trades", b.n), muted()) } else { Span::raw(format!("n={} {:.0}% win {:+.2}R", b.n, b.win, b.exp)) },
        ]));
    }
    f.render_widget(Paragraph::new(lines).block(panel("Track record")), record);
}

fn trades_record_first(f: &mut Frame, area: Rect, app: &App) {
    let [tiles, mats, list] = Layout::vertical([Constraint::Length(5), Constraint::Length(8), Constraint::Min(0)]).areas(area);
    let s = stats(app.trades.iter());
    let tile_areas: [Rect; 4] = Layout::horizontal([Constraint::Ratio(1, 4); 4]).areas(tiles);
    let tile = |label: &str, value: String, style: Style| Paragraph::new(vec![Line::from(Span::styled(value, style.add_modifier(Modifier::BOLD))), Line::from(Span::styled(label.to_string(), muted()))]).block(Block::bordered().border_style(fg(Theme::BORDER_BLURRED)));
    f.render_widget(tile("closed trades (1 void excluded)", format!("{}", s.n), fg(Theme::TEXT_PRIMARY)), tile_areas[0]);
    f.render_widget(tile("win rate", format!("{:.0}%", s.win), fg(Theme::TEXT_PRIMARY)), tile_areas[1]);
    f.render_widget(tile("expectancy per trade", format!("{:+.2}R", s.exp), signed(s.exp)), tile_areas[2]);
    f.render_widget(tile("avg win / avg loss", format!("{:+.2}R / {:+.2}R", s.avg_win, s.avg_loss), fg(Theme::TEXT_PRIMARY)), tile_areas[3]);
    let [a, b, c] = Layout::horizontal([Constraint::Ratio(1, 3); 3]).areas(mats);
    f.render_widget(breakdown(app, "By lean at entry", &["Leans buy", "Leans hold", "Leans sell", "Unclear"], |t| t.lean), a);
    f.render_widget(breakdown(app, "By regime at entry", &["Bull", "Range-bound", "Bear"], |t| t.regime), b);
    f.render_widget(breakdown(app, "By Space", &["Quantum computing", "Lithium recovery", "Rate cuts & REITs"], |t| t.space), c);
    let open = app.open_trades();
    let mut rows: Vec<Row> = open.iter().enumerate().map(|(i, &t)| trade_row(&app.trades[t], true, i == app.sel)).collect();
    rows.extend(app.trades.iter().filter(|t| t.closed() || t.void).map(|t| trade_row(t, false, false)));
    f.render_widget(
        Table::new(rows, TRADE_WIDTHS)
            .header(Row::new(["", "Space", "Entry", "Last / exit", "Stop/tgt · why", "Ret", "R", "Held/vs", "Lean@entry", ""]).style(muted()))
            .block(panel("All trades · open first · c close")),
        list,
    );
}

// ---------------------------------------------------------------- modals

fn draw_modal(f: &mut Frame, app: &App) {
    let area = f.area();
    match &app.modal {
        Modal::None => {}
        Modal::Go { filter, sel } => {
            let r = centered(area, 52, 12);
            f.render_widget(Clear, r);
            let mut lines = vec![Line::from(vec![Span::styled("> ", fg(Theme::PRIMARY)), Span::raw(format!("{filter}_"))]), Line::raw("")];
            for (i, s) in spaces().iter().filter(|s| s.name.to_lowercase().contains(&filter.to_lowercase())).enumerate() {
                lines.push(
                    Line::from(vec![
                        Span::styled(format!("{:<20}", s.name), if s.archived { muted() } else { fg(Theme::TEXT_PRIMARY) }),
                        Span::styled(format!("{:<9}", s.slug), muted()),
                        Span::styled(if s.archived { "archived".to_string() } else { format!("{} · {}", s.regime, s.health) }, muted()),
                    ])
                    .style(sel_style(i == *sel)),
                );
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled("type to filter · Enter open · Esc", muted())));
            f.render_widget(Paragraph::new(lines).block(panel("Go to Space").border_style(fg(Theme::BORDER))), r);
        }
        Modal::Note { text, pin } => {
            let r = centered(area, 76, 9);
            f.render_widget(Clear, r);
            let mut pins = vec![Span::styled("Pinned to  ", muted())];
            let labels: Vec<String> = std::iter::once("Space".to_string()).chain(app.members.iter().map(|m| short(m.sym).to_string())).collect();
            for (i, l) in labels.iter().enumerate() {
                pins.push(if i == *pin { chip(l, Theme::PRIMARY) } else { Span::styled(format!(" {l} "), muted()) });
            }
            let lines = vec![
                Line::from(pins),
                Line::raw(""),
                Line::from(vec![Span::styled("> ", fg(Theme::PRIMARY)), Span::raw(format!("{text}_"))]),
                Line::raw(""),
                Line::from(Span::styled("Tab pin · Enter save · Esc cancel · notes are your view, never cited as fact", muted())),
            ];
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(panel(&format!("New note · {}", app.space_name)).border_style(fg(Theme::BORDER))), r);
        }
        Modal::Close { trade, field, date, price, reason, lesson } => {
            let t = &app.trades[*trade];
            let r = centered(area, 90, 16);
            f.render_widget(Clear, r);
            let mut preview = t.clone();
            preview.exit = Some((date.clone(), price.parse().unwrap_or(t.last), REASONS[*reason]));
            let fieldl = |i: usize, label: &str, value: Line<'static>| {
                let mut spans = vec![Span::styled(format!("{} {label:<12}", if i == *field { "›" } else { " " }), if i == *field { bold(Theme::PRIMARY) } else { muted() })];
                spans.extend(value.spans);
                Line::from(spans)
            };
            let mut reasons = Vec::new();
            for (i, why) in REASONS.iter().enumerate() {
                reasons.push(if i == *reason { chip(why, Theme::PRIMARY) } else { Span::styled(format!(" {why} "), muted()) });
            }
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(format!("{} ", short(t.inst)), bold(Theme::TEXT_PRIMARY)),
                    Span::styled(format!("{} · bought {} @ {:.2} · stop {:.2} · target {:.2}", t.space, t.entry_date, t.entry, t.stop, t.target), muted()),
                ]),
            ];
            if t.flag.is_some() {
                lines.push(Line::from(Span::styled("Stop likely hit 27 Oct. Prefilled with the stop; correct to your actual fill.", fg(Theme::TEXT_WARNING))));
            }
            lines.push(Line::raw(""));
            lines.push(fieldl(0, "Exit date", Line::raw(format!("{date}{}", if *field == 0 { "_" } else { "" }))));
            lines.push(fieldl(1, "Exit price", Line::raw(format!("{price}{}", if *field == 1 { "_" } else { "" }))));
            lines.push(fieldl(2, "Reason", Line::from(reasons)));
            lines.push(fieldl(3, "Lesson", Line::raw(format!("{lesson}{}", if *field == 3 { "_" } else { "  (optional, saved as a note on the member)" }))));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("Outcome  ", muted()),
                Span::styled(format!("{:+.1}%  {:+.2}R", preview.ret(), preview.r()), signed(preview.r()).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  vs benchmark {:+.1}%  · held {}d · JEV snapshot at exit", preview.ret() - t.bench, t.days), muted()),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled("↑↓ field · ←→ reason · Enter record exit · Esc cancel", muted())));
            f.render_widget(Paragraph::new(lines).block(panel("Record exit").border_style(fg(Theme::BORDER))), r);
        }
    }
}

// ---------------------------------------------------------------- frame

fn draw(f: &mut Frame, app: &App) {
    let [body, status, bar] = Layout::vertical([Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)]).areas(f.area());
    match (app.screen, app.variant[app.vidx()]) {
        (Screen::Space, 0) => space_cockpit(f, body, app),
        (Screen::Space, 1) => space_master_detail(f, body, app),
        (Screen::Space, _) => space_timeline(f, body, app),
        (Screen::Home, 0) => home_table(f, body, app),
        (Screen::Home, _) => home_inbox(f, body, app),
        (Screen::Trades, 0) => trades_ledger(f, body, app),
        (Screen::Trades, _) => trades_record_first(f, body, app),
    }
    f.render_widget(Paragraph::new(Span::styled(app.status.clone(), fg(Theme::TEXT_WARNING))), status);
    let screen = ["SPACE", "HOME", "TRADES"][app.vidx()];
    let v = app.variant[app.vidx()];
    let letter = (b'A' + v as u8) as char;
    let key = Style::default().fg(Color::Rgb(255, 220, 255)).add_modifier(Modifier::BOLD);
    let line = Line::from(vec![
        Span::styled(" PROTOTYPE ", Style::default().fg(Color::Black).bg(Color::Rgb(230, 120, 255)).add_modifier(Modifier::BOLD)),
        Span::styled("  ◀ ", key),
        Span::styled(format!("{screen} · {letter} {} ({}/{})", app.variants()[v], v + 1, app.variants().len()), key),
        Span::styled(" ▶   ", key),
        Span::raw("1 Space  2 Home  3 Trades  ↑↓ select  Enter open  g go  n note  c close  q quit"),
    ]);
    f.render_widget(Paragraph::new(line).style(Style::default().bg(Color::Rgb(70, 20, 90)).fg(Color::White)), bar);
    draw_modal(f, app);
}

fn dump() -> io::Result<()> {
    let shots: Vec<(Screen, usize, &str)> = vec![
        (Screen::Space, 0, ""),
        (Screen::Space, 1, ""),
        (Screen::Space, 1, "member"),
        (Screen::Space, 2, ""),
        (Screen::Home, 0, ""),
        (Screen::Home, 1, ""),
        (Screen::Trades, 0, ""),
        (Screen::Trades, 1, ""),
        (Screen::Space, 0, "note"),
        (Screen::Trades, 0, "close"),
        (Screen::Home, 0, "go"),
    ];
    for (screen, v, extra) in shots {
        let mut app = App::new();
        app.screen = screen;
        app.variant[screen as usize] = v;
        match extra {
            "member" => app.sel = 2,
            "note" => app.key(KeyCode::Char('n')),
            "close" => {
                app.sel = 1;
                app.key(KeyCode::Char('c'));
            }
            "go" => app.key(KeyCode::Char('g')),
            _ => {}
        }
        let mut term = Terminal::new(TestBackend::new(140, 42))?;
        term.draw(|f| draw(f, &app))?;
        let buf = term.backend().buffer().clone();
        println!("=== {:?} variant {} {}", ["Space", "Home", "Trades"][screen as usize], v, extra);
        for y in 0..buf.area.height {
            let line: String = (0..buf.area.width).map(|x| buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" ")).collect();
            println!("{}", line.trim_end());
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    if std::env::args().any(|a| a == "--dump") {
        return dump();
    }
    let mut terminal = ratatui::init();
    let mut app = App::new();
    let result = (|| -> io::Result<()> {
        while !app.quit {
            terminal.draw(|f| draw(f, &app))?;
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    app.key(k.code);
                }
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}
