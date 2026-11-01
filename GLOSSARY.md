# Delta

A personal store for AI-processed investment research, organised by Space: an
AI harness researches and writes into Delta, the user adds notes directly, and
Delta shows each idea's prices, reports, notes, indicator readings and trades
in one place. The product is clarity; readings are indicators, never decisions.

## Language

### Spaces

**Space**:
The home for one investment idea: its premise, instruments, notes, reports, indicator readings and trades.
_Avoid_: dossier, theme, target, thesis, idea, area, workspace

**Member**:
An instrument added to a Space; the instrument and its prices are shared, but what is written about it in the Space stays there.
_Avoid_: holding, constituent

**Horizon**:
How long the user gives a Space's idea to play out: under 6 months, 6-18 months, 1-3 years, or 3+ years.
_Avoid_: timeframe, holding period

**Risk appetite**:
How much risk the user accepts for one Space (low, medium or high); it shapes the candidates the harness proposes but never a JEV reading.
_Avoid_: risk tolerance, risk profile

**Tripwire**:
A condition the user names that would prove a Space's premise wrong; Delta watches the evidence for it and warns, but never acts on it.
_Avoid_: falsifier, kill criterion, invalidation criteria

**Conclusion**:
The short note written when a Space is archived, recording what the idea came to.
_Avoid_: post-mortem, summary

**Related Space**:
A link between two Spaces whose ideas overlap; Spaces never nest.
_Avoid_: sub-space, child space

**Benchmark**:
The one instrument a Space measures its market against; it sets the Space's market regime and the comparison for its trade outcomes.
_Avoid_: index (a benchmark need not be one)

**Premise**:
The question or hypothesis a Space exists to answer, e.g. "Is quantum computing worth investing in, and through which company?"
_Avoid_: thesis, hypothesis

**Harness**:
The external AI agent (e.g. Claude Code, Codex) that researches and writes into Delta through its validated interface.
_Avoid_: assistant, bot, agent (alone)

**Note**:
The user's own written thinking, pinned to a Space or to one of its members; read as the user's view, never cited as fact.
_Avoid_: comment, annotation, memo

**Opening note**:
The note written when a Space is created, holding the user's interview answers in their own words; the only note the harness writes.

**Mention**:
A link inside a note to an instrument, Space, report or trade, which can sit in any Space; the mentioned item lists the notes that mention it.
_Avoid_: tag, reference, backlink (a backlink is the reverse view of a mention)

**Alert**:
A notice that something in a Space needs the user's attention, such as an exit likely, a tripped tripwire, a reading change, a stale report, results due or a source failure; shown on Home when Delta is opened, until acted on or dismissed.
_Avoid_: notification, signal

### Indicators

**JEV reading**:
A recorded, unchangeable set of probabilities from the Jev decision model, with the evidence it was given: a market regime for a benchmark, or an outlook health or lean for a member within its Space.
_Avoid_: signal, recommendation, rating

**Market regime**:
The reading of whether a benchmark's market is in a bull, bear or range-bound phase; shared by every Space with that benchmark.

**Outlook health**:
The reading of whether the evidence on a member is improving or deteriorating against its Space's premise; a Space's health is the roll-up across its members.
_Avoid_: thesis health, score

**Lean**:
The reading of which way the evidence on balance tilts for a member: leans buy, leans hold, leans sell, or unclear.
_Avoid_: recommendation, call, signal

### Trades

**Trade**:
A user-recorded long position taken within a Space, mirroring one placed on the user's trading platform: entry price, target price, accepted loss and a rationale, with no size.
_Avoid_: order, transaction, position

**Accepted loss**:
The stop price set at entry; the trade's initial risk is the distance from entry to it.
_Avoid_: stop-loss amount, risk budget

**Trade outcome**:
The recorded result of a closed trade, kept beside the JEV readings at entry and exit to build a track record.
_Avoid_: P&L, score

**Exit likely**:
The flag on an open trade whose stop or target the stored prices have crossed, kept until the user records the exit from their actual fill.
_Avoid_: stopped out, triggered

**Track record**:
The summary of closed trade outcomes (win rate, average R, expectancy), broken down by lean and regime at entry, Space and risk appetite.
_Avoid_: performance, returns

**R multiple**:
A trade's return divided by its initial risk; a trade stopped out at its original stop scores -1R.

### Instruments and markets

**Instrument**:
A tradable security identified as market + symbol, e.g. `US:AAPL`.
_Avoid_: ticker, stock, symbol

**Market**:
A trading venue convention (e.g. US, ASX) that namespaces symbols and sets the reporting currency.
_Avoid_: exchange

**Macro series**:
A dated economic series not tied to any instrument, such as a policy rate, a bond yield or CPI.
_Avoid_: indicator, economic data

### Evidence

**Evidence**:
A single gathered, citable fact or observation, stored once and linked to any Space it bears on, addressable by a stable id.
_Avoid_: data point, document, article

**Linked evidence**:
Evidence attached to a Space by hand or by the harness because it bears on the idea rather than on one member; the only evidence that can be removed from a Space.
_Avoid_: attachment, bookmark

**Evidence pool**:
The unified set of all gathered evidence; the only facts a report or a JEV reading may rest on.

**Source record**:
A raw row written by a data source — bar, news item, filing, fundamental, event, macro value — flattened into evidence for reading.

**Event**:
A dated corporate occurrence for an instrument, such as a results announcement or an ex-dividend date, taken from a data source's calendar; it may be upcoming or past.
_Avoid_: news, catalyst (a catalyst is a report's expectation, not a calendar fact)

**Harness source**:
Evidence the harness found outside Delta's data sources and submitted, with its URL and when it was retrieved; once stored it is cited like any other evidence.
_Avoid_: web result, web evidence

### Synthesis

**Report**:
A cited synthesis of gathered evidence, written by the harness and belonging to one Space. Once accepted it never changes; a rewrite is a new version that supersedes it.
_Avoid_: analysis, summary

**Landscape report**:
The report on the terrain around a Space's premise (its players, size and drivers), first written when the Space is created.
_Avoid_: state-of-the-market report, market overview

**Deep dive**:
A report on one member of a Space.

**Scan**:
A report that sweeps a sub-sector or theme to surface candidate instruments.

**Answer**:
A report that replies to one question the user asked.

**Stale report**:
A report flagged as possibly out of date: it is past its shelf life, cites removed evidence, or a filing or earnings event has landed for an instrument it covers since it was written.

**Brief**:
A facts-only, no-model summary of current state for one instrument.
_Avoid_: report

**Citation**:
The link from a claim to the evidence or trade it rests on; machine-checked against the store.
_Avoid_: reference, source

**Citation contract**:
The rule that every paragraph of a report, outside its open questions, cites stored evidence or trades; a report with any missing or invalid citation is rejected whole.

**Bull / Bear**:
The paired arguments for and against; both are required in a Landscape report and a Deep dive.

### Pipeline

**Data source**:
A named entry in the config that supplies one or more kinds of source record for the markets and asset classes it covers.
_Avoid_: plugin (the code behind a source), feed, provider

**Ingest**:
Data sources fetching source records into the local store.

**News stance**:
Jev's bull, bear or neutral reading of one news item for one instrument; summarised over recent weeks as the news tone a JEV reading takes in.
_Avoid_: sentiment score

**Gather evidence**:
The action that runs ingest and news-stance classification together.
