//! Turn pagination (`packages/transcript/src/pagination/paginate.ts`).

use dimi_wire::item::Item;

use crate::apply::turn_ordinal;

/// `TurnPageQuery` (paginate.ts 19–30).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPageQuery {
    pub before_turn: Option<String>,
    pub after_turn: Option<String>,
    pub page_size: i64,
}

/// `TurnPage` (paginate.ts 24–27).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnPage {
    pub items: Vec<Item>,
    pub has_more: bool,
}

/// `paginateTurns` (paginate.ts 31–45): `afterTurn` wins over `beforeTurn`;
/// no cursor reads the NEWEST page.
pub fn paginate_turns(items: &[Item], query: TurnPageQuery) -> TurnPage {
    let page_size = query.page_size.max(1);
    if items.is_empty() {
        return TurnPage {
            items: Vec::new(),
            has_more: false,
        };
    }
    let segments = split_segments(items);
    if let Some(after) = &query.after_turn {
        let filtered: Vec<&Segment<'_>> = segments
            .iter()
            .filter(|seg| {
                seg.turn_id
                    .as_ref()
                    .is_some_and(|id| compare_turn_ids(id, after) > 0)
            })
            .collect();
        page(&filtered, page_size, Direction::Newer)
    } else if let Some(before) = &query.before_turn {
        let filtered: Vec<&Segment<'_>> = segments
            .iter()
            .filter(|seg| {
                seg.turn_id
                    .as_ref()
                    .is_none_or(|id| compare_turn_ids(id, before) < 0)
            })
            .collect();
        page(&filtered, page_size, Direction::Older)
    } else {
        page(
            &segments.iter().collect::<Vec<_>>(),
            page_size,
            Direction::Older,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Older,
    Newer,
}

/// One timeline segment: a turn plus the non-turn items that follow it (or
/// the leading non-turn units as a head segment with no turn id).
///
/// Segments borrow the input timeline — `split_segments` never clones items
/// and `page` clones only the items that land in the returned page, so a
/// page call over a large timeline is O(timeline) reference work plus
/// O(page) clones instead of deep-cloning the whole timeline per page.
#[derive(Debug, Clone, PartialEq)]
struct Segment<'a> {
    turn_id: Option<&'a str>,
    items: Vec<&'a Item>,
}

/// `splitSegments` (paginate.ts 53–75).
fn split_segments(items: &[Item]) -> Vec<Segment<'_>> {
    let mut segments: Vec<Segment<'_>> = Vec::new();
    for entry in items {
        if let Item::Turn { turn_id, .. } = entry {
            segments.push(Segment {
                turn_id: Some(turn_id.as_str()),
                items: vec![entry],
            });
        } else if let Some(last) = segments.last_mut() {
            last.items.push(entry);
        } else {
            segments.push(Segment {
                turn_id: None,
                items: vec![entry],
            });
        }
    }
    segments
}

/// `page` (paginate.ts 77–95).
fn page(segments: &[&Segment<'_>], page_size: i64, direction: Direction) -> TurnPage {
    let head: Option<&Segment<'_>> = match segments.first() {
        Some(seg) if seg.turn_id.is_none() => Some(seg),
        _ => None,
    };
    let turn_segments: Vec<&&Segment<'_>> = segments
        .iter()
        .filter(|seg| seg.turn_id.is_some())
        .collect();

    let size = page_size as usize;
    let (selected, reaches_first_turn) = match direction {
        Direction::Older => {
            let start = turn_segments.len().saturating_sub(size);
            let selected = &turn_segments[start..];
            (selected, selected.len() == turn_segments.len())
        }
        Direction::Newer => {
            let end = size.min(turn_segments.len());
            (&turn_segments[..end], false)
        }
    };

    let has_more = turn_segments.len() > selected.len() && !selected.is_empty();

    let mut items = Vec::new();
    if direction == Direction::Older && reaches_first_turn {
        if let Some(head) = head {
            items.extend(head.items.iter().copied().cloned());
        }
    }
    for seg in selected {
        items.extend(seg.items.iter().copied().cloned());
    }
    TurnPage { items, has_more }
}

/// `compareTurnIds` — numeric comparison (`t2 < t10`).
pub fn compare_turn_ids(a: &str, b: &str) -> i64 {
    turn_ordinal(a) - turn_ordinal(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Build a `{"kind":"turn", ...}` item (minimal shape, like the
    /// differential fixture) with a numeric turn id.
    fn turn_item(id: &str) -> Item {
        serde_json::from_str(&format!(
            r#"{{"kind":"turn","turnId":"{id}","ordinal":0,"state":"completed","origin":{{"kind":"user"}},"steps":[]}}"#
        ))
        .unwrap()
    }

    fn marker_item(id: &str) -> Item {
        serde_json::from_str(&format!(
            r#"{{"kind":"marker","markerId":"{id}","marker":"m"}}"#
        ))
        .unwrap()
    }

    /// A timeline of `turn_count` turns, each followed by `markers_per_turn`
    /// markers — the shape of a long-lived session's cold snapshot.
    fn timeline(turn_count: usize, markers_per_turn: usize) -> Vec<Item> {
        let mut items = Vec::with_capacity(turn_count * (1 + markers_per_turn));
        for turn in 0..turn_count {
            items.push(turn_item(&format!("t{turn}")));
            for marker in 0..markers_per_turn {
                items.push(marker_item(&format!("t{turn}-m{marker}")));
            }
        }
        items
    }

    /// Paging through the whole timeline must not clone the input per page.
    /// Perf smoke: a generous wall bound catches accidental O(timeline)
    /// cloning regressions (a 50k-item sweep takes seconds when every page
    /// deep-clones the timeline, milliseconds when segments borrow it).
    #[test]
    fn full_pagination_sweep_stays_fast() {
        let items = timeline(5_000, 9); // 50k items, 5k turn segments
        let mut before: Option<String> = None;
        let mut pages = 0usize;
        let start = Instant::now();
        loop {
            let page = paginate_turns(
                &items,
                TurnPageQuery {
                    before_turn: before.clone(),
                    after_turn: None,
                    page_size: 20,
                },
            );
            pages += 1;
            if page.items.is_empty() {
                break;
            }
            let previous = before.clone();
            // The client cursor is the OLDEST turn in the page ("older than
            // this turn"); scanning forward finds it (the head segment, when
            // present, is non-turn markers).
            for item in page.items.iter() {
                if let Item::Turn { turn_id, .. } = item {
                    before = Some(turn_id.to_string());
                    break;
                }
            }
            if !page.has_more || before == previous {
                break;
            }
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(900),
            "full pagination sweep took {elapsed:?} across {pages} pages"
        );
        // 5000 turns / 20 per page ≈ 250 pages.
        assert!(
            (245..=260).contains(&pages),
            "expected ~250 pages, got {pages}"
        );
    }

    /// The page cursor (oldest turn id in the page) must strictly progress
    /// toward the oldest turn — the sweep above terminates only when it does.
    #[test]
    fn pages_terminate_at_oldest_turn() {
        let items = timeline(100, 2);
        let mut before: Option<String> = None;
        let mut oldest_turn_seen: Option<String> = None;
        loop {
            let page = paginate_turns(
                &items,
                TurnPageQuery {
                    before_turn: before.clone(),
                    after_turn: None,
                    page_size: 7,
                },
            );
            if page.items.is_empty() {
                break;
            }
            let mut oldest_turn_in_page: Option<String> = None;
            for item in page.items.iter() {
                if let Item::Turn { turn_id, .. } = item {
                    oldest_turn_in_page = Some(turn_id.to_string());
                    break;
                }
            }
            if let Some(id) = &oldest_turn_in_page {
                if let Some(last) = &oldest_turn_seen {
                    assert!(
                        compare_turn_ids(id, last) < 0,
                        "cursor regressed: {id} >= {last}"
                    );
                }
                oldest_turn_seen = oldest_turn_in_page.clone();
                before = oldest_turn_in_page;
            } else {
                break;
            }
            if !page.has_more {
                break;
            }
        }
        assert_eq!(
            oldest_turn_seen.as_deref(),
            Some("t0"),
            "sweep must reach the oldest turn"
        );
    }
}
