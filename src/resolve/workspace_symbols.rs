use super::{Definition, WorkspaceIndex};
use crate::symbols::Symbol;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchRank {
    Subsequence,
    Substring,
    Prefix,
    Exact,
}

/// Project-wide symbol search backing `workspace/symbol`.
///
/// `indexes` are searched in priority order (earliest wins ties), so the caller
/// passes workspace before base before builtins. Results are the document-symbol
/// set (everything except locals and parameters) whose name matches `query`,
/// ranked best-first and capped at `limit`.
pub fn workspace_symbols(
    indexes: &[&WorkspaceIndex],
    query: &str,
    limit: usize,
) -> Vec<Definition> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(MatchRank, usize, &str, &Symbol)> = Vec::new();
    for (tier, index) in indexes.iter().enumerate() {
        for (uri, symbols) in index.documents() {
            for symbol in symbols {
                if !symbol.kind.is_outline() || symbol.name.is_empty() {
                    continue;
                }
                if let Some(rank) = match_rank(&symbol.name, query) {
                    scored.push((rank, tier, uri, symbol));
                }
            }
        }
    }

    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.cmp(&b.1))
            .then(a.3.name.len().cmp(&b.3.name.len()))
            .then(a.3.name.cmp(&b.3.name))
    });
    scored.truncate(limit);

    scored
        .into_iter()
        .map(|(_, _, uri, symbol)| Definition {
            uri: uri.to_string(),
            symbol: symbol.clone(),
        })
        .collect()
}

/// `WitcherScript` identifiers are ASCII, so case folding is ASCII-only and allocation-free.
fn match_rank(name: &str, query: &str) -> Option<MatchRank> {
    let name = name.as_bytes();
    let query = query.as_bytes();
    if name.eq_ignore_ascii_case(query) {
        Some(MatchRank::Exact)
    } else if name.len() >= query.len() && name[..query.len()].eq_ignore_ascii_case(query) {
        Some(MatchRank::Prefix)
    } else if ascii_contains_ci(name, query) {
        Some(MatchRank::Substring)
    } else if ascii_subsequence_ci(name, query) {
        Some(MatchRank::Subsequence)
    } else {
        None
    }
}

fn ascii_contains_ci(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

fn ascii_subsequence_ci(haystack: &[u8], needle: &[u8]) -> bool {
    let mut matched = 0;
    for &byte in haystack {
        if matched == needle.len() {
            break;
        }
        if byte.eq_ignore_ascii_case(&needle[matched]) {
            matched += 1;
        }
    }
    matched == needle.len()
}
