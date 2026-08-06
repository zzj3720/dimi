//! Working tips — the toolbar tips shown behind the composing spinner (port
//! of `apps/dimi/src/tui/components/chrome/working-tips.ts` plus the
//! `buildWeightedTips` helper from `apps/dimi/src/tui/components/chrome/footer.ts`
//! and the `WORKING_TIPS` table from `apps/dimi/src/tui/constant/tips.ts`).

use std::sync::LazyLock;

/// One toolbar tip (`ToolbarTip` in the TS source).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarTip {
    pub text: &'static str,
    /// Long/important tips render on their own. They never pair with a
    /// neighbour and never appear as the second half of someone else's pair.
    pub solo: bool,
    /// Rotation weight: a higher value makes the tip recur more often.
    /// Defaults to 1.
    pub priority: u32,
}

impl ToolbarTip {
    pub const fn new(text: &'static str, solo: bool, priority: u32) -> Self {
        ToolbarTip {
            text,
            solo,
            priority,
        }
    }
}

/// Subset of toolbar tips shown behind the composing spinner (`WORKING_TIPS`).
pub const WORKING_TIPS: &[ToolbarTip] = &[
    ToolbarTip::new(
        "ctrl-s to add guidance without waiting for the turn to finish",
        true,
        2,
    ),
    ToolbarTip::new(
        "/tasks to check progress and status for background tasks",
        false,
        2,
    ),
    ToolbarTip::new("/init: generate AGENTS.md", false, 2),
    ToolbarTip::new("Try /dance for a hidden Easter egg", false, 1),
    ToolbarTip::new(
        "/plugins: manage plugins — try the \"Dimi Datasource\" for reliable financial, economic, and academic data",
        true,
        3,
    ),
    ToolbarTip::new(
        "ask Dimi to schedule tasks, e.g. \"remind me at 5pm\"",
        true,
        3,
    ),
    ToolbarTip::new("/sessions to browse and resume earlier sessions", true, 1),
    ToolbarTip::new("/web: use the Web UI for a better experience", true, 1),
    ToolbarTip::new("@: mention files", false, 2),
    ToolbarTip::new("! to run a shell command", false, 2),
];

/// Tip rotation interval (ms) — `TIP_ROTATE_INTERVAL_MS` in the TS source.
pub const TIP_ROTATE_INTERVAL_MS: u64 = 10_000;

/// Expand tips into a rotation sequence using smooth weighted round-robin
/// (the nginx SWRR algorithm). Higher-`priority` tips appear more often while
/// staying evenly spread, so a tip generally does not land next to its own
/// duplicate. Deterministic and computed once at module load (port of
/// `buildWeightedTips` in `footer.ts`).
pub fn build_weighted_tips(tips: &[ToolbarTip]) -> Vec<&ToolbarTip> {
    struct Item<'a> {
        tip: &'a ToolbarTip,
        weight: u32,
        current: i64,
    }
    let mut items: Vec<Item> = tips
        .iter()
        .map(|tip| Item {
            tip,
            weight: tip.priority.max(1),
            current: 0,
        })
        .collect();
    let total: u32 = items.iter().map(|it| it.weight).sum();

    let mut seq: Vec<&ToolbarTip> = Vec::new();
    for _ in 0..total {
        let mut best = 0usize;
        for idx in 0..items.len() {
            items[idx].current += i64::from(items[idx].weight);
            if items[idx].current > items[best].current {
                best = idx;
            }
        }
        items[best].current -= i64::from(total);
        seq.push(items[best].tip);
    }
    seq
}

/// The `WORKING_TIP_ROTATION` singleton — built once at module load.
static WORKING_TIP_ROTATION: LazyLock<Vec<&'static ToolbarTip>> =
    LazyLock::new(|| build_weighted_tips(WORKING_TIPS));

/// The working tip at `now_ms` (port of `currentWorkingTip`). Deterministic
/// for a fixed timestamp.
pub fn current_working_tip(now_ms: u64) -> Option<&'static ToolbarTip> {
    let rotation = WORKING_TIP_ROTATION.as_slice();
    if rotation.is_empty() {
        return None;
    }
    let index = ((now_ms / TIP_ROTATE_INTERVAL_MS) % rotation.len() as u64) as usize;
    Some(rotation[index])
}

/// Pick a random tip from the weighted working-tip rotation. If `exclude_text`
/// is provided and there are other tips available, avoid returning the same
/// text twice in a row (port of `pickRandomWorkingTip`).
pub fn pick_random_working_tip(exclude_text: Option<&str>) -> Option<&'static ToolbarTip> {
    pick_random_working_tip_seeded(exclude_text, seed_from_time())
}

/// [`pick_random_working_tip`] with an explicit seed — the pure, testable core.
/// `seed` is mixed via splitmix64 so adjacent seeds (e.g. successive `now`
/// ticks) do not land on adjacent rotation slots.
pub fn pick_random_working_tip_seeded(
    exclude_text: Option<&str>,
    seed: u64,
) -> Option<&'static ToolbarTip> {
    let rotation = WORKING_TIP_ROTATION.as_slice();
    if rotation.is_empty() {
        return None;
    }
    let candidates: Vec<&ToolbarTip> = match exclude_text {
        Some(excluded) if rotation.len() > 1 => rotation
            .iter()
            .copied()
            .filter(|t| t.text != excluded)
            .collect(),
        _ => rotation.to_vec(),
    };
    let pool = if candidates.is_empty() {
        rotation
    } else {
        &candidates
    };
    let index = (splitmix64(seed) as usize) % pool.len();
    Some(pool[index])
}

/// A small deterministic PRNG (splitmix64) used to turn a seed into an index.
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Seed from the wall clock (nanoseconds since the UNIX epoch).
fn seed_from_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_weighted_tips_respects_weights() {
        // Total weight of WORKING_TIPS = 2+2+2+1+3+3+1+1+2+2 = 19 → rotation
        // length 19. Each tip appears exactly `weight` times.
        let rotation = build_weighted_tips(WORKING_TIPS);
        assert_eq!(rotation.len(), 19);
        for tip in WORKING_TIPS {
            let count = rotation.iter().filter(|t| **t == tip).count();
            assert_eq!(count as u32, tip.priority.max(1), "tip {:?}", tip.text);
        }
        // No tip immediately repeats itself (SWRR spreading).
        for w in rotation.windows(2) {
            assert_ne!(w[0].text, w[1].text);
        }
    }

    #[test]
    fn current_working_tip_is_deterministic() {
        let now = 1_000_000u64;
        assert_eq!(current_working_tip(now), current_working_tip(now));
        // Pinned against the TS capture: now=1_000_000 → "@: mention files".
        assert_eq!(
            current_working_tip(1_000_000).map(|t| t.text),
            Some("@: mention files")
        );
        assert_eq!(
            current_working_tip(250_000).map(|t| t.text),
            Some("! to run a shell command")
        );
        // 10s later the rotation advances by one slot.
        let next = current_working_tip(1_000_000 + TIP_ROTATE_INTERVAL_MS);
        let cur = current_working_tip(1_000_000).unwrap();
        assert_ne!(next.unwrap().text, cur.text);
    }

    #[test]
    fn pick_random_working_tip_from_rotation() {
        let tip = pick_random_working_tip(None).expect("rotation non-empty");
        assert!(WORKING_TIPS.iter().any(|t| t.text == tip.text));
    }

    #[test]
    fn pick_random_working_tip_respects_exclusion() {
        // Try many seeds; with the exclusion in place we must never return
        // the excluded text (rotation has >1 distinct texts).
        let first = pick_random_working_tip_seeded(None, 1234).unwrap();
        for seed in 0..200 {
            let next = pick_random_working_tip_seeded(Some(first.text), seed).unwrap();
            assert_ne!(next.text, first.text, "seed {seed}");
        }
    }

    #[test]
    fn pick_random_working_tip_deterministic_for_seed() {
        assert_eq!(
            pick_random_working_tip_seeded(None, 42),
            pick_random_working_tip_seeded(None, 42)
        );
    }
}
