//! The player's states side by side (in the spirit of the More Spreadsheets mod):
//! population, unemployment, peasants, literacy, infrastructure and the best
//! build the advisor found there.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use vic3_prices::{PricesResult, World};

use crate::advice::Advice;

/// One owned state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateRow {
    pub id: u32,
    pub label: String,
    pub population: f64,
    pub workforce: f64,
    /// Working-age pops without a workplace (the game's "unemployed working adults").
    pub unemployed: f64,
    /// Peasants' share of the workforce: labour a new building can hire.
    pub peasant_share: f64,
    pub literacy: f64,
    pub infrastructure: Option<f64>,
    pub infrastructure_usage: Option<f64>,
    /// Title of the advisor's top build in this state, if any.
    pub best_build: Option<String>,
}

/// Rows for `states`, most populous first; `advice` must be in ranked order.
pub fn state_overview(world: &World, prices: &PricesResult, states: &BTreeSet<u32>, advice: &[Advice]) -> Vec<StateRow> {
    #[derive(Default)]
    struct Sum {
        population: f64,
        workforce: f64,
        unemployed: f64,
        peasants: f64,
        literate: f64,
    }
    let mut sums: BTreeMap<u32, Sum> = BTreeMap::new();
    for pop in prices.state_pops.iter().filter(|p| states.contains(&p.state_id)) {
        let s = sums.entry(pop.state_id).or_default();
        let work = pop.workforce.unwrap_or(0.0);
        s.population += pop.demand_size.unwrap_or(work + pop.dependents.unwrap_or(0.0));
        s.workforce += work;
        s.literate += pop.literate.unwrap_or(0.0);
        if pop.profession_name.as_deref() == Some("peasants") {
            s.peasants += work;
        }
    }
    // Unemployment from the save's own pops: the price rows merge pops and lose their
    // workplace. Workers without one match the game's "unemployed working adults"
    // (Brandenburg 1836: 37.6K in the save, 40.5K in the game a week later).
    for pop in world.state_pops.iter().filter(|p| p.workplace_id.is_none()) {
        if let Some(s) = pop.state.filter(|s| states.contains(s)).and_then(|s| sums.get_mut(&s)) {
            s.unemployed += pop.workforce.unwrap_or(0.0);
        }
    }
    let mut rows: Vec<StateRow> = sums
        .into_iter()
        .map(|(id, s)| {
            let info = prices.states.iter().find(|st| st.id == id);
            let suffix = format!(":{id}");
            let best = advice.iter().find(|a| a.id.ends_with(&suffix) && (a.id.contains(":expand:") || a.id.contains(":short:") || a.id.contains(":construction:"))).map(|a| a.title.clone());
            StateRow {
                id,
                label: info.map_or_else(|| format!("state {id}"), |st| st.label.clone()),
                population: s.population,
                workforce: s.workforce,
                unemployed: s.unemployed,
                peasant_share: if s.workforce > 0.0 { s.peasants / s.workforce } else { 0.0 },
                // `literate` counts literate workers: over the workforce it matches the save's national literacy.
                literacy: if s.workforce > 0.0 { s.literate / s.workforce } else { 0.0 },
                infrastructure: info.and_then(|st| st.infrastructure),
                infrastructure_usage: info.and_then(|st| st.infrastructure_usage),
                best_build: best,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.population.total_cmp(&a.population));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_sum_the_states_pops_and_name_the_best_build() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let save = std::fs::read(dir.join("../vic3-load/tests/fixtures/plaintext.txt")).expect("save");
        let defs = vic3_defs::load_from_path(dir.join("../vic3-defs/tests/fixtures")).expect("defs");
        let blob = vic3_defs::encode_blob(&defs).expect("blob");
        let snap = crate::load_analysis_snapshot(&save, None, &blob, "{}", false).expect("snapshot");
        let state = snap.prices.state_pops.iter().next().expect("a pop").state_id;
        let advice = vec![Advice {
            id: format!("growth:expand:building_logging_camp:{state}"),
            domain: crate::advice::Domain::Economy,
            priority: 1,
            title: "Expand Logging Camps here".into(),
            detail: String::new(),
            goal: None,
            why: None,
            steps: Vec::new(),
            why_now: Vec::new(),
            check: None,
        }];
        let rows = state_overview(&snap.world, &snap.prices, &BTreeSet::from([state]), &advice);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(row.population > 0.0 && row.workforce > 0.0);
        assert!((0.0..=1.0).contains(&row.peasant_share) && (0.0..=1.0).contains(&row.literacy));
        assert_eq!(row.best_build.as_deref(), Some("Expand Logging Camps here"));
    }
}
