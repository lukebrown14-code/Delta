//! Watchlist symbol lookup through the Yahoo data plugin.

use delta_plugins::yahoo::{SearchResult, YahooClient};
use delta_plugins::PluginError;

/// Resolve a typed company or ticker query. The TUI owns debounce and stale
/// request cancellation; this service keeps provider access out of the view.
pub async fn search_targets(
    client: &YahooClient,
    query: &str,
) -> Result<Vec<SearchResult>, PluginError> {
    if query.trim().chars().count() < 2 {
        return Ok(Vec::new());
    }
    client.search(query, 8).await
}

/// Keep local matches first and append distinct provider listings.
pub fn merge_target_search(
    local: Vec<SearchResult>,
    remote: Vec<SearchResult>,
) -> Vec<SearchResult> {
    let mut merged = local;
    for hit in remote {
        if !merged.iter().any(|item| {
            item.market.eq_ignore_ascii_case(&hit.market)
                && item.symbol.eq_ignore_ascii_case(&hit.symbol)
        }) {
            merged.push(hit);
        }
    }
    merged.truncate(8);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hit(symbol: &str, market: &str) -> SearchResult {
        SearchResult {
            symbol: symbol.into(),
            name: symbol.into(),
            market: market.into(),
            currency: "USD".into(),
            exchange: String::new(),
            asset_class: "equity".into(),
        }
    }

    #[test]
    fn offline_matches_stay_first_and_remote_duplicates_are_removed() {
        let merged = merge_target_search(
            vec![hit("AAPL", "us")],
            vec![hit("aapl", "US"), hit("APLE", "us")],
        );
        assert_eq!(
            merged
                .iter()
                .map(|hit| hit.symbol.as_str())
                .collect::<Vec<_>>(),
            vec!["AAPL", "APLE"]
        );
    }
}
