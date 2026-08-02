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
        let filtered: Vec<&Segment> = segments
            .iter()
            .filter(|seg| {
                seg.turn_id
                    .as_ref()
                    .is_some_and(|id| compare_turn_ids(id, after) > 0)
            })
            .collect();
        page(&filtered, page_size, Direction::Newer)
    } else if let Some(before) = &query.before_turn {
        let filtered: Vec<&Segment> = segments
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
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    turn_id: Option<String>,
    items: Vec<Item>,
}

/// `splitSegments` (paginate.ts 53–75).
fn split_segments(items: &[Item]) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    for entry in items {
        if let Item::Turn { turn_id, .. } = entry {
            segments.push(Segment {
                turn_id: Some(turn_id.as_str().to_owned()),
                items: vec![entry.clone()],
            });
        } else if let Some(last) = segments.last_mut() {
            last.items.push(entry.clone());
        } else {
            segments.push(Segment {
                turn_id: None,
                items: vec![entry.clone()],
            });
        }
    }
    segments
}

/// `page` (paginate.ts 77–95).
fn page(segments: &[&Segment], page_size: i64, direction: Direction) -> TurnPage {
    let head: Option<&Segment> = match segments.first() {
        Some(seg) if seg.turn_id.is_none() => Some(seg),
        _ => None,
    };
    let turn_segments: Vec<&&Segment> = segments
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
            items.extend(head.items.iter().cloned());
        }
    }
    for seg in selected {
        items.extend(seg.items.iter().cloned());
    }
    TurnPage { items, has_more }
}

/// `compareTurnIds` — numeric comparison (`t2 < t10`).
pub fn compare_turn_ids(a: &str, b: &str) -> i64 {
    turn_ordinal(a) - turn_ordinal(b)
}
