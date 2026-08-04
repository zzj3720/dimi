//! Fuzzy matching utilities — port of `@dimi-agent/pi-tui` `src/fuzzy.ts`.
//!
//! Matches if all query characters appear in order (not necessarily
//! consecutive). Lower score = better match. Used by the choice picker and
//! model selector search.

/// Result of a fuzzy match: whether it matched and its score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuzzyMatch {
    pub matches: bool,
    pub score: f64,
}

/// Word-boundary check — mirrors the TS regex `[\s\-_./:]`.
fn is_word_boundary_char(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '_' | '.' | '/' | ':')
}

/// Match a single normalized query against `text_lower`.
fn match_query(query: &str, text_lower: &str) -> FuzzyMatch {
    if query.is_empty() {
        return FuzzyMatch {
            matches: true,
            score: 0.0,
        };
    }
    if query.chars().count() > text_lower.chars().count() {
        return FuzzyMatch {
            matches: false,
            score: 0.0,
        };
    }

    let query_chars: Vec<char> = query.chars().collect();
    let text_chars: Vec<char> = text_lower.chars().collect();
    let mut query_index = 0usize;
    let mut score = 0.0f64;
    let mut last_match_index: Option<usize> = None;
    let mut consecutive_matches = 0usize;

    for (i, ch) in text_chars.iter().enumerate() {
        if query_index >= query_chars.len() {
            break;
        }
        if *ch == query_chars[query_index] {
            let is_word_boundary = i == 0 || is_word_boundary_char(text_chars[i - 1]);

            // Reward consecutive matches.
            if last_match_index == Some(i.wrapping_sub(1)) {
                consecutive_matches += 1;
                score -= (consecutive_matches * 5) as f64;
            } else {
                consecutive_matches = 0;
                // Penalize gaps.
                if let Some(last) = last_match_index {
                    score += ((i - last - 1) * 2) as f64;
                }
            }

            // Reward word boundary matches.
            if is_word_boundary {
                score -= 10.0;
            }

            // Slight penalty for later matches.
            score += i as f64 * 0.1;

            last_match_index = Some(i);
            query_index += 1;
        }
    }

    if query_index < query_chars.len() {
        return FuzzyMatch {
            matches: false,
            score: 0.0,
        };
    }

    if query == text_lower {
        score -= 100.0;
    }

    FuzzyMatch {
        matches: true,
        score,
    }
}

/// Full fuzzy match — primary match, plus the alphanumeric swap fallback.
pub fn fuzzy_match(query: &str, text: &str) -> FuzzyMatch {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();

    let primary = match_query(&query_lower, &text_lower);
    if primary.matches {
        return primary;
    }

    // TS: /^(?<letters>[a-z]+)(?<digits>[0-9]+)$/ and the reverse.
    let swapped = swap_letter_digit(&query_lower);
    let Some(swapped) = swapped else {
        return primary;
    };

    let swapped_match = match_query(&swapped, &text_lower);
    if !swapped_match.matches {
        return primary;
    }

    FuzzyMatch {
        matches: true,
        score: swapped_match.score + 5.0,
    }
}

/// For a query like `abc123`, produce `123abc` (and vice versa).
///
/// Mirrors the TS `^(?<letters>[a-z]+)(?<digits>[0-9]+)$` and the reverse.
fn swap_letter_digit(query: &str) -> Option<String> {
    let letters_then_digits = regex::Regex::new(r"^[a-z]+[0-9]+$").ok()?.is_match(query);
    let digits_then_letters = regex::Regex::new(r"^[0-9]+[a-z]+$").ok()?.is_match(query);
    let letters: String = query.chars().filter(|c| c.is_ascii_lowercase()).collect();
    let digits: String = query.chars().filter(|c| c.is_ascii_digit()).collect();
    if letters_then_digits {
        Some(format!("{digits}{letters}"))
    } else if digits_then_letters {
        Some(format!("{letters}{digits}"))
    } else {
        None
    }
}

/// Filter and sort items by fuzzy match quality (best matches first).
/// Supports whitespace- and slash-separated tokens: all tokens must match.
pub fn fuzzy_filter<T>(items: Vec<T>, query: &str, get_text: impl Fn(&T) -> String) -> Vec<T> {
    if query.trim().is_empty() {
        return items;
    }

    let tokens: Vec<&str> = query
        .trim()
        .split([' ', '\t', '\n', '\r', '/'])
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return items;
    }

    let mut results: Vec<(T, f64)> = Vec::new();
    for item in items {
        let text = get_text(&item);
        let mut total_score = 0.0f64;
        let mut all_match = true;
        for token in &tokens {
            let m = fuzzy_match(token, &text);
            if m.matches {
                total_score += m.score;
            } else {
                all_match = false;
                break;
            }
        }
        if all_match {
            results.push((item, total_score));
        }
    }

    results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    results.into_iter().map(|(item, _)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert!(fuzzy_match("", "anything").matches);
        assert_eq!(fuzzy_match("", "anything").score, 0.0);
    }

    #[test]
    fn exact_match_bonus() {
        let m = fuzzy_match("rust", "rust");
        assert!(m.matches);
        assert!(m.score < 0.0);
    }

    #[test]
    fn subsequence_match() {
        assert!(fuzzy_match("rs", "rust").matches);
        assert!(!fuzzy_match("xr", "rust").matches);
        // Characters must appear in order — "t" comes after "r" in "rust",
        // so "tr" does not match (verified against pi-tui fuzzyMatch).
        assert!(!fuzzy_match("tr", "rust").matches);
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_match("RUST", "rust").matches);
    }

    #[test]
    fn gap_penalty_and_boundary_bonus() {
        let boundary = fuzzy_match("ws", "web search").score;
        let gapped = fuzzy_match("wr", "web search").score;
        assert!(boundary < gapped, "boundary bonus should score better");
    }

    #[test]
    fn alphanumeric_swap() {
        // "gpt4" should also match text containing "4gpt".
        let m = fuzzy_match("gpt4", "4gpt model");
        assert!(m.matches, "letter/digit swap should match");
        // And the direct match still wins.
        assert!(fuzzy_match("gpt4", "gpt4 model").matches);
    }

    #[test]
    fn filter_requires_all_tokens() {
        let items = vec![
            "model selector".to_owned(),
            "session picker".to_owned(),
            "model".to_owned(),
        ];
        let out = fuzzy_filter(items, "model sel", |s| s.clone());
        assert_eq!(out, vec!["model selector".to_owned()]);
    }

    #[test]
    fn filter_sorts_by_score() {
        let items = vec![
            "rust tui render".to_owned(),
            "render rust".to_owned(),
            "rust".to_owned(),
        ];
        // Both match "rust", but the exact-ish "rust" alone ranks first.
        let out = fuzzy_filter(items, "rust", |s| s.clone());
        assert_eq!(out[0], "rust");
    }

    #[test]
    fn filter_slash_separated() {
        let items = vec!["a/b/c".to_owned(), "abc".to_owned()];
        let out = fuzzy_filter(items, "a/b", |s| s.clone());
        // Both match both tokens ("a" and "b" appear in each); order follows
        // the TS score (verified against pi-tui fuzzyFilter).
        assert_eq!(out, vec!["a/b/c".to_owned(), "abc".to_owned()]);
    }
}
