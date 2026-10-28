//! What a followed suggestion did: when an advice item with a check first shows,
//! the player's budget numbers are stored; once a later save passes the check,
//! the numbers then are stored beside them, so the player sees the effect.

use std::path::Path;

use serde::{Deserialize, Serialize};
use vic3_defs::GameDefs;
use vic3_prices::World;
use vic3save::PdsDate;

use crate::advice::{checks_done, AdviceResult, Check};

/// Tracked items kept in the store; older ones drop off.
const MAX_TRACKED: usize = 300;
/// Outcomes returned, newest first.
const MAX_OUTCOMES: usize = 10;

/// The player's budget at one save.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// In-game date, `year.month.day`.
    pub date: String,
    pub weekly_balance: Option<f64>,
    pub treasury: f64,
    /// Debt over credit limit.
    pub debt_share: Option<f64>,
    /// Military share of weekly spending.
    pub military_share: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Tracked {
    country: String,
    id: String,
    title: String,
    check: Check,
    before: Snapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after: Option<Snapshot>,
}

/// One followed suggestion with the budget before and after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub id: String,
    pub title: String,
    pub before: Snapshot,
    pub after: Snapshot,
    /// The changes in words, e.g. "weekly balance −32.6K → −20.1K".
    pub summary: String,
}

/// The played country's budget numbers in `world`.
pub fn snapshot(world: &World, defs: &GameDefs) -> Option<Snapshot> {
    let tag = world.player_tag.as_deref()?;
    let country = world.country_by_tag(tag)?;
    let date = world.game_date.map_or_else(String::new, |d| format!("{}.{}.{}", d.year(), d.month(), d.day()));
    let military = crate::budget::report(defs, country).map(|b| {
        b.expenses.iter().filter(|g| matches!(g.group.as_str(), "military_goods" | "military_wages")).map(|g| g.share).sum()
    });
    Some(Snapshot {
        date,
        weekly_balance: country.weekly_balance,
        treasury: country.treasury,
        debt_share: country.credit_limit.filter(|l| *l > 0.0).map(|l| country.debt_principal.unwrap_or(0.0) / l),
        military_share: military,
    })
}

fn summary(before: &Snapshot, after: &Snapshot) -> String {
    let money = crate::budget::money;
    let pct = |x: f64| format!("{:.0}%", x * 100.0);
    let mut parts = Vec::new();
    if let (Some(a), Some(b)) = (before.weekly_balance, after.weekly_balance) {
        parts.push(format!("weekly balance {} → {}", money(a), money(b)));
    }
    if let (Some(a), Some(b)) = (before.military_share, after.military_share) {
        if (a - b).abs() >= 0.01 {
            parts.push(format!("military {} → {} of spending", pct(a), pct(b)));
        }
    }
    if let (Some(a), Some(b)) = (before.debt_share, after.debt_share) {
        if (a - b).abs() >= 0.01 {
            parts.push(format!("debt {} → {} of the credit limit", pct(a), pct(b)));
        }
    }
    parts.push(format!("treasury {} → {}", money(before.treasury), money(after.treasury)));
    format!("{} → {}: {}", before.date, after.date, parts.join(", "))
}

/// Record new checked items of `result`, close the ones `world` now passes, store
/// them at `store` (JSON lines) and return the followed ones, newest first.
///
/// # Errors
///
/// Writing the store.
pub fn track(store: &Path, result: &AdviceResult, world: &World, defs: &GameDefs) -> std::io::Result<Vec<Outcome>> {
    let Some(now) = snapshot(world, defs) else { return Ok(Vec::new()) };
    let country = world.player_tag.clone().unwrap_or_default();
    let mut all: Vec<Tracked> = std::fs::read_to_string(store)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let mut changed = false;
    for t in all.iter_mut().filter(|t| t.country == country && t.after.is_none() && t.before.date != now.date) {
        if checks_done(world, defs, std::slice::from_ref(&t.check)) == [true] {
            t.after = Some(now.clone());
            changed = true;
        }
    }
    for item in &result.advice {
        let Some(check) = &item.check else { continue };
        if all.iter().any(|t| t.country == country && t.id == item.id) {
            continue;
        }
        // Already true when first seen: nothing to measure.
        if checks_done(world, defs, std::slice::from_ref(check)) == [true] {
            continue;
        }
        all.push(Tracked { country: country.clone(), id: item.id.clone(), title: item.title.clone(), check: check.clone(), before: now.clone(), after: None });
        changed = true;
    }
    if all.len() > MAX_TRACKED {
        all.drain(..all.len() - MAX_TRACKED);
        changed = true;
    }
    if changed {
        if let Some(dir) = store.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text: String = all.iter().filter_map(|t| serde_json::to_string(t).ok()).map(|l| l + "\n").collect();
        std::fs::write(store, text)?;
    }
    Ok(all
        .iter()
        .rev()
        .filter(|t| t.country == country)
        .filter_map(|t| {
            let after = t.after.clone()?;
            Some(Outcome { id: t.id.clone(), title: t.title.clone(), summary: summary(&t.before, &after), before: t.before.clone(), after })
        })
        .take(MAX_OUTCOMES)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_names_the_changes() {
        let before = Snapshot { date: "1836.2.11".into(), weekly_balance: Some(-32_600.0), treasury: 300_000.0, debt_share: Some(0.0), military_share: Some(0.40) };
        let after = Snapshot { date: "1837.1.1".into(), weekly_balance: Some(-20_100.0), treasury: 120_000.0, debt_share: Some(0.0), military_share: Some(0.28) };
        assert_eq!(
            summary(&before, &after),
            "1836.2.11 → 1837.1.1: weekly balance −32.6K → −20.1K, military 40% → 28% of spending, treasury 300.0K → 120.0K"
        );
    }

    #[test]
    fn items_are_tracked_once_and_closed_when_their_check_passes() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let save = std::fs::read(dir.join("../vic3-load/tests/fixtures/plaintext.txt")).expect("save");
        let defs = vic3_defs::load_from_path(dir.join("../vic3-defs/tests/fixtures")).expect("defs");
        let blob = vic3_defs::encode_blob(&defs).expect("blob");
        let mut snap = crate::load_analysis_snapshot(&save, None, &blob, "{}", false).expect("snapshot");
        let root = dir.join(format!("../../target/tmp/oiko-outcomes-{}", std::process::id()));
        let store = root.join("advice_log.jsonl");
        let item = crate::advice::Advice {
            id: "budget:balance".into(),
            domain: crate::advice::Domain::Economy,
            priority: 1,
            title: "Fix the balance".into(),
            detail: String::new(),
            goal: None,
            why: None,
            steps: Vec::new(),
            why_now: Vec::new(),
            check: Some(Check::WeeklyBalance { at_least: 1.0e12 }),
        };
        let result = AdviceResult { advice: vec![item.clone()], limitations: Vec::new(), budget: None, market: Vec::new(), outcomes: Vec::new() };
        assert!(track(&store, &result, &snap.world, &snap.defs).unwrap().is_empty());
        assert!(track(&store, &result, &snap.world, &snap.defs).unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&store).unwrap().lines().count(), 1, "tracked once");
        // A later save where the check passes.
        let tag = snap.world.player_tag.clone().unwrap();
        snap.world.countries.iter_mut().find(|c| c.tag == tag).unwrap().weekly_balance = Some(2.0e12);
        snap.world.game_date = Some(vic3_load::Vic3Date::from_ymdh(1900, 1, 1, 0));
        let outcomes = track(&store, &result, &snap.world, &snap.defs).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].summary.contains("→ 1900.1.1"), "{}", outcomes[0].summary);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
