//! The Dossier command grammar (#48): `<target> <SECTION> [arg]`, typed on
//! the `:` line and shared with the headless `delta dossier` command.
//!
//! Pure: the caller passes the focused target, the watched targets and the
//! default market; nothing here touches services or storage.

/// What a dossier is about: an instrument (`US:AAPL`) or a non-instrument
/// target named by its handle (`#cloud-capex`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Instrument(String),
    Handle(String),
}

impl Target {
    pub fn instrument(id: &str) -> Self {
        Target::Instrument(id.to_string())
    }

    pub fn handle(name: &str) -> Self {
        Target::Handle(name.to_string())
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Instrument(id) => write!(f, "{id}"),
            Target::Handle(name) => write!(f, "#{name}"),
        }
    }
}

/// A dossier section, by its typed code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Des,
    Evd,
    Val,
    Rpt,
    Ths,
    Note,
    Dec,
}

/// A section's fixed positional argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    /// `EVD FIL`: filings only.
    Filings,
    /// `NOTE +`: add a note.
    Add,
    /// `RPT new`: write a fresh report.
    New,
}

/// Each section's fixed arguments, by their displayed word.
const ARGS: [(Section, &str, Arg); 3] = [
    (Section::Evd, "FIL", Arg::Filings),
    (Section::Note, "+", Arg::Add),
    (Section::Rpt, "NEW", Arg::New),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub target: Target,
    pub section: Section,
    pub arg: Option<Arg>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Run(Command),
    /// A bare symbol matched several watched instruments: the user picks
    /// one, then `section` and `arg` run on it.
    Pick {
        candidates: Vec<Target>,
        section: Section,
        arg: Option<Arg>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// A bare section code with no dossier in focus.
    NoFocus(Section),
    /// Not watched and not held under the typed or default market.
    UnknownInstrument(String),
    /// `#handle` that no watched target answers to.
    UnknownHandle(String),
    /// An instrument-only section asked of a `#handle` target.
    NeedsInstrument(Section, String),
    /// The word after the target is not a section code.
    UnknownSection(String),
    /// A word the section does not take.
    UnexpectedWord(Section, String),
    /// Nothing typed.
    Empty,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::NoFocus(section) => write!(
                f,
                "No dossier is open. Name a target first, e.g. US:AAPL {}.",
                section.code()
            ),
            CommandError::UnknownInstrument(id) => write!(
                f,
                "Unknown instrument {id}. Type it as MARKET:SYMBOL, e.g. US:AAPL."
            ),
            CommandError::UnknownHandle(name) => {
                write!(f, "No watched target answers to #{name}.")
            }
            CommandError::NeedsInstrument(section, name) => {
                let codes: Vec<&str> = Section::ALL
                    .into_iter()
                    .filter(|s| !s.needs_instrument())
                    .map(Section::code)
                    .collect();
                let (last, rest) = codes.split_last().expect("sections without instruments");
                write!(
                    f,
                    "{} needs an instrument and #{name} is not one. Try {} or {last}.",
                    section.code(),
                    rest.join(", ")
                )
            }
            CommandError::UnknownSection(word) => write!(
                f,
                "{word} is not a section. Sections: DES, EVD, VAL, RPT, THS, NOTE, DEC."
            ),
            CommandError::UnexpectedWord(section, word) => {
                write!(f, "{} does not take {word}.", section.code())
            }
            CommandError::Empty => write!(f, "Type a target, a section, or both."),
        }
    }
}

impl std::error::Error for CommandError {}

/// What the grammar resolves against.
#[derive(Debug, Clone)]
pub struct Context {
    pub focus: Option<Target>,
    pub watched: Vec<Target>,
    /// Instrument ids (`US:MSFT`) Delta holds, watched or not.
    pub known_instruments: Vec<String>,
    pub default_market: String,
}

impl Section {
    /// Every section, in tab order.
    pub const ALL: [Section; 7] = [
        Section::Des,
        Section::Evd,
        Section::Val,
        Section::Rpt,
        Section::Ths,
        Section::Note,
        Section::Dec,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Section::Des => "DES",
            Section::Evd => "EVD",
            Section::Val => "VAL",
            Section::Rpt => "RPT",
            Section::Ths => "THS",
            Section::Note => "NOTE",
            Section::Dec => "DEC",
        }
    }

    /// Valuation and decisions only exist for an instrument.
    pub fn needs_instrument(self) -> bool {
        matches!(self, Section::Val | Section::Dec)
    }

    /// The argument a typed word names for this section, ignoring case.
    pub fn arg(self, word: &str) -> Option<Arg> {
        let word = word.to_ascii_uppercase();
        ARGS.iter()
            .find(|(s, w, _)| *s == self && *w == word)
            .map(|(_, _, arg)| *arg)
    }

    /// The words this section takes, as displayed.
    pub fn arg_words(self) -> impl Iterator<Item = &'static str> {
        ARGS.iter()
            .filter(move |(s, _, _)| *s == self)
            .map(|(_, w, _)| *w)
    }

    /// The section a typed word names, ignoring case: its code or one of the
    /// built-in long forms.
    pub fn from_word(word: &str) -> Option<Self> {
        match word.to_ascii_uppercase().as_str() {
            "DESC" => Some(Section::Des),
            "NOTES" => Some(Section::Note),
            "REPORT" => Some(Section::Rpt),
            code => Self::ALL.into_iter().find(|s| s.code() == code),
        }
    }
}

pub fn parse(input: &str, ctx: &Context) -> Result<Outcome, CommandError> {
    let mut words = input.split_whitespace();
    let first = words.next().ok_or(CommandError::Empty)?;
    let (mut candidates, section) = match Section::from_word(first) {
        Some(section) => (
            vec![ctx.focus.clone().ok_or(CommandError::NoFocus(section))?],
            section,
        ),
        None => {
            let candidates = resolve(first, ctx)?;
            let section = match words.next() {
                Some(word) => Section::from_word(word)
                    .ok_or_else(|| CommandError::UnknownSection(word.to_ascii_uppercase()))?,
                None => Section::Des,
            };
            (candidates, section)
        }
    };
    if let [Target::Handle(name)] = candidates.as_slice() {
        if section.needs_instrument() {
            return Err(CommandError::NeedsInstrument(section, name.clone()));
        }
    }
    let unexpected = |word: &str| CommandError::UnexpectedWord(section, word.to_ascii_uppercase());
    let arg = match words.next() {
        Some(word) => Some(section.arg(word).ok_or_else(|| unexpected(word))?),
        None => None,
    };
    if let Some(word) = words.next() {
        return Err(unexpected(word));
    }
    if candidates.len() > 1 {
        return Ok(Outcome::Pick {
            candidates,
            section,
            arg,
        });
    }
    Ok(Outcome::Run(Command {
        target: candidates.remove(0),
        section,
        arg,
    }))
}

/// Tab-completion candidates for the word being typed (the last one), as
/// they would be displayed.
pub fn complete(input: &str, ctx: &Context) -> Vec<String> {
    let (done, typed) = input.rsplit_once(' ').unwrap_or(("", input));
    let typed = typed.to_ascii_uppercase();
    let done: Vec<&str> = done.split_whitespace().collect();
    let sections = |handle: bool| {
        Section::ALL
            .into_iter()
            .filter(move |s| !(handle && s.needs_instrument()))
            .map(Section::code)
    };
    let words: Vec<String> = match done.as_slice() {
        [] => sections(false)
            .map(String::from)
            .chain(ctx.watched.iter().map(Target::to_string))
            .collect(),
        [only] => match Section::from_word(only) {
            Some(section) => section.arg_words().map(String::from).collect(),
            None => sections(only.starts_with('#')).map(String::from).collect(),
        },
        [_, section] => Section::from_word(section)
            .into_iter()
            .flat_map(Section::arg_words)
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    };
    words
        .into_iter()
        .filter(|w| {
            let w = w.to_ascii_uppercase();
            let symbol = w.split_once(':').map(|(_, s)| s);
            w.starts_with(&typed) || symbol.is_some_and(|s| s.starts_with(&typed))
        })
        .collect()
}

/// The targets a typed word may mean: `MARKET:SYMBOL` as given, or a bare
/// symbol matched against the watched instruments, then the default market.
fn resolve(word: &str, ctx: &Context) -> Result<Vec<Target>, CommandError> {
    if let Some(name) = word.strip_prefix('#') {
        let target = Target::Handle(name.to_ascii_lowercase());
        return match ctx.watched.contains(&target) {
            true => Ok(vec![target]),
            false => Err(CommandError::UnknownHandle(name.to_ascii_lowercase())),
        };
    }
    let word = word.to_ascii_uppercase();
    if word.contains(':') {
        return known(word, ctx).map(|t| vec![t]);
    }
    let suffix = format!(":{word}");
    let matches: Vec<Target> = ctx
        .watched
        .iter()
        .filter(|t| matches!(t, Target::Instrument(id) if id.ends_with(&suffix)))
        .cloned()
        .collect();
    if matches.is_empty() {
        return known(format!("{}:{word}", ctx.default_market), ctx)
            .map(|t| vec![t])
            .map_err(|_| CommandError::UnknownInstrument(word));
    }
    Ok(matches)
}

fn known(id: String, ctx: &Context) -> Result<Target, CommandError> {
    let target = Target::Instrument(id.clone());
    if ctx.known_instruments.contains(&id) || ctx.watched.contains(&target) {
        Ok(target)
    } else {
        Err(CommandError::UnknownInstrument(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        Context {
            focus: None,
            watched: vec![Target::instrument("US:AAPL"), Target::handle("cloud-capex")],
            known_instruments: vec!["US:AAPL".into(), "US:MSFT".into()],
            default_market: "US".into(),
        }
    }

    fn run(target: Target, section: Section) -> Outcome {
        Outcome::Run(Command {
            target,
            section,
            arg: None,
        })
    }

    #[test]
    fn instrument_and_section_runs_that_section() {
        assert_eq!(
            parse("US:AAPL DES", &ctx()),
            Ok(run(Target::instrument("US:AAPL"), Section::Des))
        );
    }

    #[test]
    fn input_is_case_insensitive() {
        assert_eq!(
            parse("us:aapl val", &ctx()),
            Ok(run(Target::instrument("US:AAPL"), Section::Val))
        );
    }

    #[test]
    fn bare_section_applies_to_the_focused_target() {
        let ctx = Context {
            focus: Some(Target::instrument("US:AAPL")),
            ..ctx()
        };
        assert_eq!(
            parse("evd", &ctx),
            Ok(run(Target::instrument("US:AAPL"), Section::Evd))
        );
    }

    #[test]
    fn bare_section_without_focus_is_an_error() {
        assert_eq!(
            parse("EVD", &ctx()),
            Err(CommandError::NoFocus(Section::Evd))
        );
    }

    #[test]
    fn bare_symbol_with_one_watched_match_opens_its_brief() {
        assert_eq!(
            parse("aapl", &ctx()),
            Ok(run(Target::instrument("US:AAPL"), Section::Des))
        );
    }

    #[test]
    fn bare_symbol_with_several_watched_matches_offers_a_pick() {
        let ctx = Context {
            watched: vec![
                Target::instrument("US:SHEL"),
                Target::instrument("LSE:SHEL"),
            ],
            ..ctx()
        };
        assert_eq!(
            parse("shel val", &ctx),
            Ok(Outcome::Pick {
                candidates: vec![
                    Target::instrument("US:SHEL"),
                    Target::instrument("LSE:SHEL")
                ],
                section: Section::Val,
                arg: None,
            })
        );
    }

    #[test]
    fn bare_symbol_not_watched_tries_the_default_market() {
        assert_eq!(
            parse("msft", &ctx()),
            Ok(run(Target::instrument("US:MSFT"), Section::Des))
        );
    }

    #[test]
    fn instrument_delta_does_not_hold_is_unknown() {
        assert_eq!(
            parse("zzzz", &ctx()),
            Err(CommandError::UnknownInstrument("ZZZZ".into()))
        );
        assert_eq!(
            parse("lse:zzzz evd", &ctx()),
            Err(CommandError::UnknownInstrument("LSE:ZZZZ".into()))
        );
    }

    #[test]
    fn handle_names_a_watched_non_instrument_target() {
        assert_eq!(
            parse("#Cloud-Capex rpt", &ctx()),
            Ok(run(Target::handle("cloud-capex"), Section::Rpt))
        );
        assert_eq!(
            parse("#nope", &ctx()),
            Err(CommandError::UnknownHandle("nope".into()))
        );
    }

    #[test]
    fn valuation_and_decisions_need_an_instrument() {
        let focused = Context {
            focus: Some(Target::handle("cloud-capex")),
            ..ctx()
        };
        assert_eq!(
            parse("#cloud-capex val", &ctx()),
            Err(CommandError::NeedsInstrument(
                Section::Val,
                "cloud-capex".into()
            ))
        );
        assert_eq!(
            parse("dec", &focused),
            Err(CommandError::NeedsInstrument(
                Section::Dec,
                "cloud-capex".into()
            ))
        );
    }

    #[test]
    fn long_form_aliases_name_their_sections() {
        let focused = Context {
            focus: Some(Target::instrument("US:AAPL")),
            ..ctx()
        };
        for (word, section) in [
            ("desc", Section::Des),
            ("Notes", Section::Note),
            ("REPORT", Section::Rpt),
        ] {
            assert_eq!(
                parse(word, &focused),
                Ok(run(Target::instrument("US:AAPL"), section)),
                "{word}"
            );
        }
    }

    #[test]
    fn sections_take_their_fixed_argument() {
        let aapl = |section, arg| {
            Ok(Outcome::Run(Command {
                target: Target::instrument("US:AAPL"),
                section,
                arg: Some(arg),
            }))
        };
        assert_eq!(
            parse("aapl evd fil", &ctx()),
            aapl(Section::Evd, Arg::Filings)
        );
        assert_eq!(
            parse("US:AAPL NOTE +", &ctx()),
            aapl(Section::Note, Arg::Add)
        );
        assert_eq!(parse("aapl rpt NEW", &ctx()), aapl(Section::Rpt, Arg::New));
    }

    #[test]
    fn stray_words_are_errors() {
        assert_eq!(
            parse("aapl foo", &ctx()),
            Err(CommandError::UnknownSection("FOO".into()))
        );
        assert_eq!(
            parse("aapl val fil", &ctx()),
            Err(CommandError::UnexpectedWord(Section::Val, "FIL".into()))
        );
        assert_eq!(
            parse("aapl evd fil now", &ctx()),
            Err(CommandError::UnexpectedWord(Section::Evd, "NOW".into()))
        );
        assert_eq!(parse("   ", &ctx()), Err(CommandError::Empty));
    }

    #[test]
    fn instrument_only_section_on_a_handle_says_what_to_try() {
        assert_eq!(
            CommandError::NeedsInstrument(Section::Val, "cloud-capex".into()).to_string(),
            "VAL needs an instrument and #cloud-capex is not one. Try DES, EVD, RPT, THS or NOTE."
        );
    }

    #[test]
    fn bare_section_without_focus_says_how_to_name_a_target() {
        assert_eq!(
            CommandError::NoFocus(Section::Evd).to_string(),
            "No dossier is open. Name a target first, e.g. US:AAPL EVD."
        );
    }

    #[test]
    fn completes_a_section_code_from_its_prefix() {
        assert_eq!(complete("va", &ctx()), vec!["VAL"]);
    }

    #[test]
    fn completes_watched_targets_by_handle_or_symbol() {
        assert_eq!(complete("#cl", &ctx()), vec!["#cloud-capex"]);
        assert_eq!(complete("aa", &ctx()), vec!["US:AAPL"]);
        assert_eq!(complete("us:a", &ctx()), vec!["US:AAPL"]);
    }

    #[test]
    fn completes_what_fits_where_the_word_sits() {
        assert_eq!(
            complete("#cloud-capex ", &ctx()),
            vec!["DES", "EVD", "RPT", "THS", "NOTE"]
        );
        assert_eq!(complete("aapl evd f", &ctx()), vec!["FIL"]);
        assert_eq!(complete("aapl note ", &ctx()), vec!["+"]);
        assert_eq!(complete("aapl val ", &ctx()), Vec::<String>::new());
    }
}
