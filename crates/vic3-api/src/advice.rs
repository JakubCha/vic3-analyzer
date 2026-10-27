//! Advisor: ranked, player-scoped suggestions per domain (economy / laws / war),
//! each with an optional [`Check`] the next save can confirm as done.
//!
//! Economy and war advice reuse [`vic3_prices::alerts_with`] mitigations; laws
//! advice is a vanilla heuristic table ([`LAW_ADVICE`]). Checks are evaluated
//! against whatever save is loaded when they are asked, so the UI can track a
//! suggestion across autosaves.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use vic3_defs::GameDefs;
use vic3_prices::{
    alerts_with, Alert, AlertKind, AlertsOptions, MitigationAction, World, WorldCountry,
};

/// Advisor tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Economy,
    Laws,
    War,
}

/// One suggestion. `priority` 1 is urgent, 2 is worthwhile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Advice {
    pub id: String,
    pub domain: Domain,
    pub priority: u8,
    pub title: String,
    pub detail: String,
    /// `None` → the player marks it done by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Check>,
}

/// Advisor payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdviceResult {
    pub advice: Vec<Advice>,
    pub limitations: Vec<String>,
}

/// Condition on a later save that means the suggestion was carried out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Check {
    /// Player's levels of `building_type` (in `state_id`, or nationally) reach `at_least`.
    BuildingLevels {
        building_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_id: Option<u32>,
        at_least: f64,
    },
    /// Building `building_id` runs `method`.
    ProductionMethod { building_id: u32, method: String },
    /// The player's country no longer has `law` active.
    LawInactive { law: String },
    /// The player's country has credit headroom again.
    Solvent,
}

const MILITARY_GOODS: &[&str] = &[
    "ammunition",
    "small_arms",
    "artillery",
    "tanks",
    "aeroplanes",
    "manowars",
    "ironclads",
];

/// Vanilla laws that commonly hold an economy back: (law, title, why, priority).
///
/// shortcut: vanilla law ids only and no interest-group support / tech-unlock check;
/// upgrade when the IR parses interest groups and `common/laws`.
const LAW_ADVICE: &[(&str, &str, &str, u8)] = &[
    (
        "law_serfdom",
        "Abolish serfdom",
        "Serfs are tied to the land. Tenant Farmers or Homesteading frees peasants to take factory jobs.",
        1,
    ),
    (
        "law_traditionalism",
        "Leave Traditionalism",
        "Interventionism or Laissez-faire give more construction and a larger private investment share.",
        1,
    ),
    (
        "law_agrarianism",
        "Leave Agrarianism",
        "Agrarianism penalises industry. Interventionism or Laissez-faire suit an industrial economy.",
        2,
    ),
    (
        "law_isolationism",
        "Leave Isolationism",
        "Isolationism blocks trade. Protectionism or Free Trade let you import shortages and export surplus.",
        2,
    ),
    (
        "law_no_schools",
        "Introduce schooling",
        "Without schools literacy stays low, so machinist, engineer and clerk jobs stay unfilled.",
        2,
    ),
    (
        "law_land_based_taxation",
        "Modernise taxation",
        "Land-based taxation taxes little besides aristocrats. Consumption, per-capita or proportional taxes raise more.",
        2,
    ),
    (
        "law_hereditary_bureaucrats",
        "Professionalise the bureaucracy",
        "Appointed or elected bureaucrats produce more bureaucracy, so you can afford more institutions.",
        2,
    ),
];

/// Player country and its owned state ids (same strict filter as SQL `alerts()`).
fn player(world: &World) -> Option<(&WorldCountry, BTreeSet<u32>)> {
    let country = world.country_by_tag(world.player_tag.as_deref()?)?;
    let mut states: BTreeSet<u32> = country.states.iter().copied().collect();
    states.extend(
        world
            .states
            .iter()
            .filter(|state| state.country == Some(country.id))
            .map(|state| state.id),
    );
    Some((country, states))
}

fn building_levels(
    world: &World,
    defs: &GameDefs,
    states: &BTreeSet<u32>,
    building_type: &str,
    state_id: Option<u32>,
) -> f64 {
    world
        .buildings
        .iter()
        .filter(|b| b.state.is_some_and(|s| states.contains(&s)))
        .filter(|b| state_id.is_none() || b.state == state_id)
        .filter(|b| {
            defs.building_types_order
                .get(b.building_type_id.as_usize())
                .is_some_and(|name| name == building_type)
        })
        .map(|b| b.level)
        .sum()
}

/// Turn an alert's top mitigation into a suggestion.
fn alert_advice(world: &World, defs: &GameDefs, states: &BTreeSet<u32>, alert: &Alert) -> Option<Advice> {
    let top = alert.mitigations.iter().min_by_key(|m| m.rank)?;
    let is_military = alert.kind == AlertKind::GoodsShortage
        && alert
            .good_name
            .as_deref()
            .is_some_and(|good| MILITARY_GOODS.contains(&good));
    let check = match &top.action {
        Some(MitigationAction::Build {
            building_type_name,
            state_id,
            extra_levels,
        }) => Some(Check::BuildingLevels {
            building_type: building_type_name.clone(),
            state_id: *state_id,
            at_least: building_levels(world, defs, states, building_type_name, *state_id)
                + f64::from(extra_levels.unwrap_or(1)),
        }),
        Some(MitigationAction::Pm {
            building_id,
            production_method,
            ..
        }) => Some(Check::ProductionMethod {
            building_id: *building_id,
            method: production_method.clone(),
        }),
        _ => None,
    };
    let mut detail = format!("{}: {} {}", alert.title, alert.summary, top.detail);
    if let Some(effect) = &top.effect {
        detail.push_str(&format!(" Expected effect: {effect}."));
    }
    Some(Advice {
        id: format!("{}:{}", alert.id, top.id),
        domain: if is_military { Domain::War } else { Domain::Economy },
        priority: alert.severity.clamp(1, 2),
        title: top.title.clone(),
        detail,
        check,
    })
}

/// Ranked advice for the played country.
///
/// Returns an empty list with a limitation when the save has no player tag.
pub fn advice(world: &World, defs: &GameDefs, prices: &vic3_prices::PricesResult) -> AdviceResult {
    let mut limitations = vec![
        "Laws advice is a vanilla heuristic: it ignores interest-group support and tech unlocks.".to_string(),
        "War advice covers military-goods supply and finances only, not enemy strength.".to_string(),
    ];
    let Some((country, states)) = player(world) else {
        limitations.push("No played country in this save; nothing to advise.".into());
        return AdviceResult {
            advice: Vec::new(),
            limitations,
        };
    };

    // Mitigations are costly; build them only for the player's alerts.
    let scan = alerts_with(
        world,
        defs,
        prices,
        AlertsOptions {
            with_mitigations: false,
            mitigation_ids: None,
        },
    );
    let ids: BTreeSet<String> = scan
        .alerts
        .iter()
        .filter(|a| a.state_id.is_none_or(|id| states.contains(&id)))
        .map(|a| a.id.clone())
        .collect();
    let alerts = alerts_with(
        world,
        defs,
        prices,
        AlertsOptions {
            with_mitigations: true,
            mitigation_ids: Some(ids.clone()),
        },
    );
    let education_short = alerts.alerts.iter().any(|a| {
        ids.contains(&a.id)
            && matches!(a.kind, AlertKind::UnfilledEducation | AlertKind::UnfilledPops)
    });

    let mut advice: Vec<Advice> = alerts
        .alerts
        .iter()
        .filter(|a| ids.contains(&a.id))
        .filter_map(|a| alert_advice(world, defs, &states, a))
        .collect();

    for &(law, title, why, priority) in LAW_ADVICE {
        if !country.laws.iter().any(|active| active == law) {
            continue;
        }
        let priority = if law == "law_no_schools" && education_short { 1 } else { priority };
        advice.push(Advice {
            id: format!("law:{law}"),
            domain: Domain::Laws,
            priority,
            title: title.into(),
            detail: why.into(),
            check: Some(Check::LawInactive { law: law.into() }),
        });
    }

    if !country.solvent {
        let headroom = country
            .credit_headroom
            .map_or_else(|| "unknown".to_string(), |h| format!("{h:.0}"));
        for (domain, title, why) in [
            (
                Domain::Economy,
                "Restore credit headroom",
                "You are at or over your credit limit. Raise taxes, cut spending or build profitable state-owned industry before defaulting.",
            ),
            (
                Domain::War,
                "Do not start a war yet",
                "Mobilisation costs money and you have no credit left, so a war would push you into default.",
            ),
        ] {
            advice.push(Advice {
                id: format!("solvent:{domain:?}"),
                domain,
                priority: 1,
                title: title.into(),
                detail: format!("{why} Credit headroom: {headroom}."),
                check: Some(Check::Solvent),
            });
        }
    }

    advice.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
    AdviceResult { advice, limitations }
}

/// Whether each check holds on the loaded save. `false` when there is no player.
pub fn checks_done(world: &World, defs: &GameDefs, checks: &[Check]) -> Vec<bool> {
    let Some((country, states)) = player(world) else {
        return vec![false; checks.len()];
    };
    checks
        .iter()
        .map(|check| match check {
            Check::BuildingLevels {
                building_type,
                state_id,
                at_least,
            } => building_levels(world, defs, &states, building_type, *state_id) >= *at_least,
            Check::ProductionMethod {
                building_id,
                method,
            } => world
                .buildings
                .iter()
                .any(|b| b.id == *building_id && b.production_methods.iter().any(|pm| pm == method)),
            Check::LawInactive { law } => !country.laws.iter().any(|active| active == law),
            Check::Solvent => country.solvent,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn snapshot() -> crate::AnalysisSnapshot {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let save = std::fs::read(dir.join("../vic3-load/tests/fixtures/plaintext.txt")).expect("save");
        let defs = vic3_defs::load_from_path(dir.join("../vic3-defs/tests/fixtures")).expect("defs");
        let blob = vic3_defs::encode_blob(&defs).expect("blob");
        crate::load_analysis_snapshot(&save, None, &blob, "{}", false).expect("snapshot")
    }

    fn player_mut(world: &mut World) -> &mut WorldCountry {
        let tag = world.player_tag.clone().expect("fixture has a player");
        world.countries.iter_mut().find(|c| c.tag == tag).expect("player country")
    }

    #[test]
    fn law_advice_fires_and_check_tracks_repeal() {
        let mut snap = snapshot();
        player_mut(&mut snap.world).laws.push("law_serfdom".into());
        let result = advice(&snap.world, &snap.defs, &snap.prices);
        let serfdom = result
            .advice
            .iter()
            .find(|a| a.id == "law:law_serfdom")
            .expect("serfdom advice");
        assert_eq!(serfdom.domain, Domain::Laws);
        let check = serfdom.check.clone().expect("law check");
        assert_eq!(checks_done(&snap.world, &snap.defs, std::slice::from_ref(&check)), [false]);

        player_mut(&mut snap.world).laws.retain(|law| law != "law_serfdom");
        assert_eq!(checks_done(&snap.world, &snap.defs, &[check]), [true]);
    }

    #[test]
    fn building_levels_check_counts_player_levels() {
        let snap = snapshot();
        let (_, states) = player(&snap.world).expect("player");
        let building = snap
            .world
            .buildings
            .iter()
            .find(|b| b.state.is_some_and(|s| states.contains(&s)))
            .expect("player building");
        let name = snap.defs.building_types_order[building.building_type_id.as_usize()].clone();
        let have = building_levels(&snap.world, &snap.defs, &states, &name, building.state);
        assert!(have >= building.level);
        let check = |at_least| Check::BuildingLevels {
            building_type: name.clone(),
            state_id: building.state,
            at_least,
        };
        assert_eq!(
            checks_done(&snap.world, &snap.defs, &[check(have), check(have + 1.0)]),
            [true, false]
        );
    }

    #[test]
    fn insolvency_advises_economy_and_war() {
        let mut snap = snapshot();
        player_mut(&mut snap.world).solvent = false;
        let result = advice(&snap.world, &snap.defs, &snap.prices);
        let domains: Vec<Domain> = result
            .advice
            .iter()
            .filter(|a| a.check == Some(Check::Solvent))
            .map(|a| a.domain)
            .collect();
        assert_eq!(domains, [Domain::Economy, Domain::War]);
        assert!(result.advice.windows(2).all(|w| w[0].priority <= w[1].priority));
    }
}
