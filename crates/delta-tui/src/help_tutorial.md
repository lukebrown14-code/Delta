# Welcome to Delta

Delta reads for you. It collects facts about the companies you care about,
keeps them in one place, and writes summaries where **every claim points back
to the fact it came from**.

It does not trade, it does not size positions, and it will not tell you what
to buy or sell. The point is clarity, not tips.

---

## 1. Tell it what you care about

Press 2 for Watchlist.

A watchlist entry is anything you want watched — one company, a whole
sector, or a theme. Press a to open the entry form, fill in the fields,
and press enter to save. Press esc to close the form.

Use / to filter, up/down to select, and enter to refresh metrics for
the selected target. The metrics inspector stays open beside the watchlist.
Press space on an asset-class header to expand or collapse its targets, and
press r to cycle the chart range. Press d to remove a selected target.

Prices stream from Yahoo while Watchlist is open. **DAY %** is Yahoo’s
daily percentage change: **▲** up, **▼** down, **─** unchanged. Quote age
and connection state are separate; delivery may vary by market. Missing
quotes show **—**. Streaming quotes do not replace gathered historical bars.

| Field | Example |
| --- | --- |
| name | `iron-ore` |
| kind | `industry` |
| market | `asx` |
| tickers | `BHP,RIO,FMG` |

## 2. Let it go collect

Open the command palette (ctrl+p) and run **Gather evidence**.

`ingest` pulls prices, news, filings and earnings dates into a local database,
and `extract` turns the news into structured facts. Gather runs both.

Press 3 for Research. Its three columns keep the selected company, its report,
and the supporting evidence visible together. Database counts and model spend
are under Settings → Diagnostics.

## 3. Read what it found

In Research, pick a company in the left column and press n to generate its
report. The centre column is the written summary, with a citation on every
claim. The right column lets you search, filter, and open the supporting
evidence. Press v on a source to jump to the first report claim that cites it.
If a sentence could not be traced back to something
collected in step 2, it gets dropped rather than guessed.

## 4. Ask it questions

Press 5 for Ask.

Tick the watchlist entries you want in scope, then type a question. Answers come only
from the evidence collected in step 2 — not from the model's own memory.

Web search is a toggle, it is **off by default**, and anything it returns is
used once and never saved.

## 5. Track an idea over time

Press 4 for Theses.

Write down something you believe — *"iron ore volumes hold up through 2027"* —
and Delta proposes evidence for and against it as new facts arrive.

You accept or reject each piece yourself. The model only ever *suggests*; it
never decides what counts, and the health badge is calculated from what you
accepted, not from an opinion.

## 6. Keep a decision journal

Press **6** for Decisions. Record your rationale, valuation or price context,
time horizon, review date, and what would invalidate the idea. Later reviews
are appended to the original entry, so you can see what you knew at the time.
This is a journal for your process, not a buy or sell recommendation.

---

## Worth knowing

- **Everything is cited.** You can always check the AI's working.
- **Nothing is a recommendation.** No buy, sell or position sizing, by design.
- **Your data stays local.** Evidence lives in a SQLite file in the project.
- Press m to change which model is used on the current screen.
- Press f2 to switch between the dark and light palettes.

Press ? or esc to close this help. Press k for the full keymap.
