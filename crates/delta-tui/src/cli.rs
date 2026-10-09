//! Headless CLI entry points: `gather`, `report`, `review-due` over the
//! configured database, no TUI (`docs/rewrite/tasks/r3-cli.md`). Thin wiring
//! over the services layer — the same code paths the interactive app uses;
//! results go to stdout, diagnostics to stderr.

use std::io;

/// One line of usage, printed by `--help` (stdout) and misuse (stderr).
pub const USAGE: &str =
    "usage: delta [gather [--target X] | report TARGET | review-due [--as-of DATE]]\n\
                         Run without a command to open the terminal app.";

/// Parse `args` (argv minus the program name) and run the subcommand.
/// Returns the process exit code: 0 success, 1 runtime failure, 2 misuse.
pub async fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            println!("{USAGE}");
            0
        }
        Some("gather") => match GatherArgs::parse(&args[1..]) {
            Ok(args) => gather(args).await,
            Err(message) => usage_error(&message),
        },
        Some("report") => match ReportArgs::parse(&args[1..]) {
            Ok(args) => report(args).await,
            Err(message) => usage_error(&message),
        },
        Some("review-due") => match ReviewArgs::parse(&args[1..]) {
            Ok(args) => review_due(args).await,
            Err(message) => usage_error(&message),
        },
        Some(other) => usage_error(&format!("unknown command {other:?}")),
        None => usage_error("a command is required when arguments are given"),
    }
}

fn usage_error(message: &str) -> i32 {
    eprintln!("delta: {message}\n{USAGE}");
    2
}

fn fail(error: &io::Error) -> i32 {
    eprintln!("delta: {error}");
    1
}

pub struct GatherArgs {
    pub target: Option<String>,
}

impl GatherArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut target = None;
        let mut rest = args;
        while let Some(first) = rest.first() {
            if first == "--target" {
                let value = rest.get(1).ok_or("--target needs a value")?;
                if target.is_some() {
                    return Err("--target given twice".to_string());
                }
                target = Some(value.clone());
                rest = &rest[2..];
            } else {
                return Err(format!("unexpected argument {first:?}"));
            }
        }
        Ok(Self { target })
    }
}

pub struct ReportArgs {
    pub target: String,
}

impl ReportArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        match args {
            [target] => Ok(Self {
                target: target.clone(),
            }),
            [] => Err("report needs a target, e.g. US:AAPL".to_string()),
            _ => Err("report takes exactly one target".to_string()),
        }
    }
}

pub struct ReviewArgs {
    pub as_of: Option<String>,
}

impl ReviewArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        match args {
            [] => Ok(Self { as_of: None }),
            [flag, date] if flag == "--as-of" => Ok(Self {
                as_of: Some(date.clone()),
            }),
            _ => Err("usage: review-due [--as-of YYYY-MM-DD]".to_string()),
        }
    }
}

/// The context every headless command opens: `config.toml` + `.env` + the
/// configured universe + the configured database — the same files the TUI
/// opens.
struct Headless {
    cfg: delta_core::config::AppConfig,
    universe: Vec<delta_core::models::Instrument>,
    db: delta_core::db::Db,
}

/// The config file every headless command reads (like the TUI, from the
/// working directory).
const CONFIG_PATH: &str = "config.toml";

fn open_headless() -> Result<Headless, io::Error> {
    let path = std::path::Path::new(CONFIG_PATH);
    let (_, cfg) =
        delta_core::config::load_config(path).map_err(|e| io::Error::other(e.to_string()))?;
    let universe =
        delta_services::configured_universe(path).map_err(|e| io::Error::other(e.to_string()))?;
    let db = delta_core::db::Db::open(std::path::Path::new(&cfg.db_path))
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(Headless { cfg, universe, db })
}

async fn gather(args: GatherArgs) -> i32 {
    let mut headless = match open_headless() {
        Ok(headless) => headless,
        Err(e) => return fail(&e),
    };
    let universe = match args.target.as_deref() {
        None => headless.universe,
        Some(name) => {
            let filtered: Vec<_> = headless
                .universe
                .iter()
                .filter(|inst| inst.watchlists.iter().any(|list| list == name))
                .cloned()
                .collect();
            if filtered.is_empty() {
                return fail(&io::Error::other(format!(
                    "unknown target {name:?}; not in [targets]"
                )));
            }
            filtered
        }
    };
    match delta_services::gather_configured(&mut headless.db, &headless.cfg, &universe, |m| {
        println!("{m}")
    })
    .await
    {
        Ok(result) => {
            let rows: usize = result.ingested.counts.values().sum();
            println!(
                "{} rows stored; {} events extracted; {} stances classified",
                rows, result.extracted.events, result.sentiment
            );
            for warning in &result.warnings {
                eprintln!("warning: {warning}");
            }
            0
        }
        Err(e) => fail(&io::Error::other(e.to_string())),
    }
}

async fn report(args: ReportArgs) -> i32 {
    let mut headless = match open_headless() {
        Ok(headless) => headless,
        Err(e) => return fail(&e),
    };
    // Gather first (the card's contract), then build the cited report from
    // the fresh evidence pool. Stage logs go to stderr so stdout carries
    // only the report path (`p=$(delta report X)`).
    if let Err(e) = delta_services::gather_configured(
        &mut headless.db,
        &headless.cfg,
        &headless.universe,
        |message| eprintln!("{message}"),
    )
    .await
    {
        return fail(&io::Error::other(e.to_string()));
    }
    match delta_services::generate_report_configured(&mut headless.db, &headless.cfg, &args.target)
        .await
    {
        Ok((_, path)) => {
            println!("{}", path.display());
            0
        }
        Err(e) => fail(&io::Error::other(e.to_string())),
    }
}

async fn review_due(args: ReviewArgs) -> i32 {
    let as_of = match args.as_of.as_deref() {
        None => chrono::Utc::now().date_naive(),
        Some(raw) => match chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
            Ok(date) => date,
            Err(e) => {
                return fail(&io::Error::other(format!(
                    "bad --as-of {raw:?}: {e}; expected YYYY-MM-DD"
                )));
            }
        },
    };
    let db = match open_headless() {
        Ok(headless) => headless.db,
        Err(e) => return fail(&e),
    };
    let due = match delta_services::due_reviews(&db, Some(as_of)) {
        Ok(due) => due,
        Err(e) => return fail(&io::Error::other(e.to_string())),
    };
    let overdue = due.iter().any(|decision| decision.review_date < as_of);
    for decision in &due {
        println!(
            "{}\t{}\t{}\t{}",
            decision.id, decision.review_date, decision.instrument_id, decision.rationale
        );
    }
    if overdue {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gather_parses_target_flag() {
        let args = GatherArgs::parse(&["--target".to_string(), "apple".to_string()]).unwrap();
        assert_eq!(args.target.as_deref(), Some("apple"));
        assert!(GatherArgs::parse(&["--target".to_string()]).is_err());
        assert!(GatherArgs::parse(&["extra".to_string()]).is_err());
    }

    #[test]
    fn report_takes_exactly_one_target() {
        assert_eq!(
            ReportArgs::parse(&["US:AAPL".to_string()]).unwrap().target,
            "US:AAPL"
        );
        assert!(ReportArgs::parse(&[]).is_err());
        assert!(ReportArgs::parse(&["a".to_string(), "b".to_string()]).is_err());
    }

    #[test]
    fn review_due_parses_as_of_date() {
        assert!(ReviewArgs::parse(&[]).unwrap().as_of.is_none());
        assert_eq!(
            ReviewArgs::parse(&["--as-of".to_string(), "2026-10-09".to_string()])
                .unwrap()
                .as_of
                .as_deref(),
            Some("2026-10-09")
        );
        assert!(ReviewArgs::parse(&["--as-of".to_string()]).is_err());
    }
}
