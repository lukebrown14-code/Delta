# Trades are a second kind of citation

Reports may cite the user's trades as `[tr:<id>]` alongside evidence ids. A trade is a record of the user's actions, never market evidence, and the citation check treats the two kinds separately.

A report that mentions a past exit ("you were stopped out at 12.10") must, under the citation contract, show where that came from. The alternatives were to forbid the harness from mentioning trades, losing useful context, or to store trades as evidence, which would blur the line between what the market did and what the user did. See [Trade tracker](https://github.com/lukebrown14-code/Delta/issues/65).
