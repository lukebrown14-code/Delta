# A report with any bad citation is rejected whole

V1 silently dropped claims whose citations failed and kept the rest, rejecting a draft only if nothing substantive survived. V2 rejects the whole submission if any paragraph or bullet outside Unknowns lacks a valid citation, and returns the failing paragraphs.

V1's model couldn't be asked to retry, so dropping bad claims was the only way to save a draft. The harness can fix and resubmit. Whole rejection means an accepted report contains exactly what was written, never a version quietly trimmed by Delta. See [Report types](https://github.com/lukebrown14-code/Delta/issues/62).
