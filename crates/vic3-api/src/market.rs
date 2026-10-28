//! Market doctor: for goods far from their base price on the player's market,
//! which buildings make or use them, where the player already makes them, and
//! whether the real cause is a dear input one step up the chain.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use vic3_defs::GameDefs;
use vic3_prices::PricesResult;

/// Price over base at or above which a good counts as dear.
const DEAR: f64 = 1.2;
/// At or below which it counts as cheap.
const CHEAP: f64 = 0.8;
const MAX_DEAR: usize = 6;
const MAX_CHEAP: usize = 4;
const MAX_NAMES: usize = 3;

/// One good the doctor looked at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketCase {
    pub good: String,
    pub label: String,
    /// Price over base minus 1 (0.3 = 30% dearer).
    pub price_vs_base: f64,
    pub dear: bool,
    /// Building types that make it (labels).
    pub producers: Vec<String>,
    /// Inputs of those buildings that are dear too: fix these first.
    pub dear_inputs: Vec<String>,
    /// Building types that use it (labels), for cheap goods.
    pub consumers: Vec<String>,
    /// Where the player already makes it: state label, building label, levels, profit per level.
    pub yours: Vec<(String, String, f64, f64)>,
    /// What to do, in one or two sentences.
    pub advice: String,
    /// Concrete actions, one per line: which method to switch where, what to build where.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<String>,
}

/// `good`'s output of production method `pm` (0 when it makes none).
fn output_of(defs: &GameDefs, pm: &str, good: &str) -> f64 {
    defs.production_methods
        .get(pm)
        .map_or(0.0, |m| m.outputs.iter().filter(|(g, _)| defs.goods_order.get(g.as_usize()).is_some_and(|n| n == good)).map(|(_, q)| q).sum())
}

fn uses(defs: &GameDefs, pm: &str, input: &str) -> bool {
    defs.production_methods
        .get(pm)
        .is_some_and(|m| m.inputs.iter().any(|(g, _)| defs.goods_order.get(g.as_usize()).is_some_and(|n| n == input)))
}

/// Concrete ways around a dear `input` of the player's buildings making `good`:
/// switch the method that uses it (naming both methods, the states and the output
/// change), and build what makes the input (where the player already does, if so).
fn input_fixes(defs: &GameDefs, prices: &PricesResult, states: &BTreeSet<u32>, producers: &[&String], good: &str, input: &str) -> Vec<String> {
    let label = |id: &str| defs.display_label(id);
    let state_label = |id: u32| prices.states.iter().find(|s| s.id == id).map_or_else(|| format!("state {id}"), |s| s.label.clone());
    // (building, method, alternative) → states running it.
    let mut switches: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
    for site in prices.buildings.iter().filter(|b| producers.contains(&&b.building_type_name) && b.state_id.is_some_and(|s| states.contains(&s)) && b.level > 0.0) {
        let Some(bt) = defs.building_types.get(&site.building_type_name) else { continue };
        for pm in site.production_method_ids.iter().filter(|pm| uses(defs, pm, input)) {
            let group = bt.production_method_groups.iter().filter_map(|g| defs.production_method_groups.get(g)).find(|g| g.contains(pm));
            if let Some(alt) = group.and_then(|g| g.iter().find(|alt| *alt != pm && !uses(defs, alt, input))) {
                switches.entry((site.building_type_name.clone(), pm.clone(), alt.clone())).or_default().push(state_label(site.state_id.unwrap_or_default()));
            }
        }
    }
    let mut out: Vec<String> = switches
        .into_iter()
        .map(|((building, pm, alt), mut where_)| {
            let count = where_.len();
            where_.truncate(MAX_NAMES);
            let more = if count > MAX_NAMES { format!(" and {} more", count - MAX_NAMES) } else { String::new() };
            let (from, to) = (output_of(defs, &pm, good), output_of(defs, &alt, good));
            let change = if from > 0.0 && to < from {
                format!(" It makes about {:.0}% less {} but needs no {}.", (1.0 - to / from) * 100.0, label(good), label(input))
            } else {
                format!(" It needs no {}.", label(input))
            };
            format!(
                "Switch {} in {}{more} from \"{}\" to \"{}\": click the building, then pick the method in its production methods.{change}",
                label(&building),
                where_.join(", "),
                label(&pm),
                label(&alt)
            )
        })
        .collect();
    // Or make the input: at the player's best site for it, else name the building.
    let makers = buildings_with(defs, input, true);
    let best = prices
        .buildings
        .iter()
        .filter(|b| makers.contains(&b.building_type_name) && b.state_id.is_some_and(|s| states.contains(&s)) && b.level > 0.0)
        .max_by(|a, b| (a.profit / a.level).total_cmp(&(b.profit / b.level)));
    out.push(match best {
        Some(b) => format!("Construction panel: build more {} in {}; it makes {}.", b.building_type_label, state_label(b.state_id.unwrap_or_default()), label(input)),
        None => match makers.iter().next() {
            Some(m) => format!("Construction panel: build {} in a state with the resource and free workers; it makes {}.", label(m), label(input)),
            None => format!("No building you can build makes {}: it has to be imported.", label(input)),
        },
    });
    out
}

/// Building types whose methods output (`output = true`) or input `good`, with the other side's goods.
fn buildings_with(defs: &GameDefs, good: &str, output: bool) -> BTreeSet<String> {
    let id = |g: &vic3_defs::GoodId| defs.goods_order.get(g.as_usize()).map(String::as_str);
    defs.building_types
        .iter()
        .filter(|(name, _)| !name.starts_with("building_subsistence"))
        .filter(|(_, b)| {
            b.production_method_groups.iter().filter_map(|g| defs.production_method_groups.get(g)).flatten().filter_map(|pm| defs.production_methods.get(pm)).any(|pm| {
                let side = if output { &pm.outputs } else { &pm.inputs };
                side.iter().any(|(g, _)| id(g) == Some(good))
            })
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// Goods `building` consumes with the methods the player's sites run, or with each
/// group's first (starting) method when the player has none.
fn running_inputs(defs: &GameDefs, prices: &PricesResult, states: &BTreeSet<u32>, building: &str) -> BTreeSet<String> {
    let mine: Vec<&String> = prices
        .buildings
        .iter()
        .filter(|b| b.building_type_name == building && b.state_id.is_some_and(|s| states.contains(&s)))
        .flat_map(|b| &b.production_method_ids)
        .collect();
    let pms: Vec<&String> = if mine.is_empty() {
        defs.building_types
            .get(building)
            .map(|b| b.production_method_groups.iter().filter_map(|g| defs.production_method_groups.get(g)?.first()).collect())
            .unwrap_or_default()
    } else {
        mine
    };
    pms.into_iter()
        .filter_map(|pm| defs.production_methods.get(pm))
        .flat_map(|pm| &pm.inputs)
        .filter_map(|(g, _)| defs.goods_order.get(g.as_usize()).cloned())
        .collect()
}

/// Dear and cheap goods on the market `prices` was solved for, seen from the player's `states`.
pub fn market_doctor(defs: &GameDefs, prices: &PricesResult, states: &BTreeSet<u32>) -> Vec<MarketCase> {
    let ratio: BTreeMap<&str, f64> = prices
        .goods
        .iter()
        .filter(|g| g.base > 0.0 && g.buy + g.sell > 0.0)
        .map(|g| (g.name.as_str(), g.price / g.base))
        .collect();
    let label = |id: &str| defs.display_label(id);
    let state_label = |id: u32| prices.states.iter().find(|s| s.id == id).map_or_else(|| format!("state {id}"), |s| s.label.clone());
    let mut goods: Vec<&vic3_prices::GoodPrice> = prices.goods.iter().filter(|g| ratio.get(g.name.as_str()).is_some_and(|r| *r >= DEAR || *r <= CHEAP)).collect();
    goods.sort_by(|a, b| (ratio[b.name.as_str()] - 1.0).abs().total_cmp(&(ratio[a.name.as_str()] - 1.0).abs()));
    let (mut dear_n, mut cheap_n) = (0, 0);
    let mut out = Vec::new();
    for g in goods {
        let r = ratio[g.name.as_str()];
        let dear = r >= DEAR;
        let slot = if dear { &mut dear_n } else { &mut cheap_n };
        if *slot >= if dear { MAX_DEAR } else { MAX_CHEAP } {
            continue;
        }
        *slot += 1;
        let producers = buildings_with(defs, &g.name, true);
        // Player's sites making it, best profit per level first.
        let mut yours: Vec<(String, String, f64, f64)> = prices
            .buildings
            .iter()
            .filter(|b| producers.contains(&b.building_type_name) && b.state_id.is_some_and(|s| states.contains(&s)) && b.level > 0.0)
            .map(|b| (state_label(b.state_id.unwrap_or_default()), b.building_type_label.clone(), b.level, b.profit / b.level))
            .collect();
        yours.sort_by(|a, b| b.3.total_cmp(&a.3));
        // The player's producer types first: those are the ones to expand or fix.
        let owned: BTreeSet<&String> = producers
            .iter()
            .filter(|p| prices.buildings.iter().any(|b| &b.building_type_name == *p && b.state_id.is_some_and(|s| states.contains(&s)) && b.level > 0.0))
            .collect();
        let mut ordered: Vec<&String> = owned.iter().copied().collect();
        ordered.extend(producers.iter().filter(|p| !owned.contains(p)));
        let relevant: Vec<&String> = if owned.is_empty() { ordered.clone() } else { owned.iter().copied().collect() };
        let dear_ids: Vec<String> = relevant
            .iter()
            .flat_map(|b| running_inputs(defs, prices, states, b))
            .filter(|i| dear && *i != g.name && ratio.get(i.as_str()).is_some_and(|r| *r >= DEAR))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(MAX_NAMES)
            .collect();
        let dear_inputs: Vec<String> = dear_ids.iter().map(|i| label(i)).collect();
        let mut fixes: Vec<String> = dear_ids.iter().flat_map(|i| input_fixes(defs, prices, states, &relevant, &g.name, i)).collect();
        yours.truncate(MAX_NAMES);
        let consumers: Vec<String> = if dear { Vec::new() } else { buildings_with(defs, &g.name, false).iter().take(MAX_NAMES).map(|b| label(b)).collect() };
        let producer_labels: Vec<String> = ordered.iter().take(MAX_NAMES).map(|b| label(b)).collect();
        let pct = (r - 1.0) * 100.0;
        let advice = if dear && !dear_inputs.is_empty() {
            let building = yours.first().map_or_else(|| producer_labels.first().cloned().unwrap_or_default(), |y| y.1.clone());
            let verb = if dear_inputs.len() > 1 { "are also" } else { "is also" };
            format!("{} is {pct:+.0}%, but your {building} run on {}, which {verb} dear: new levels would earn little until you do one of these.", g.label, dear_inputs.join(", "))
        } else if consumers.is_empty() && !dear {
            format!("{} is {pct:+.0}%: nothing you can build uses it, so queue no more of its producers.", g.label)
        } else if dear {
            match yours.first() {
                Some((state, building, _, profit)) if *profit > 0.0 => {
                    fixes.push(format!("Construction panel: queue one more level of {building} in {state}."));
                    format!("{} is {pct:+.0}%: expand {building} in {state}, your most profitable site for it.", g.label)
                }
                _ => {
                    fixes.push(format!("Construction panel: build {} in a state with the resource and free workers.", producer_labels.join(" or ")));
                    format!("{} is {pct:+.0}%: you make too little of it.", g.label)
                }
            }
        } else if yours.is_empty() {
            fixes.push(format!("Construction panel: {} use it and now buy it cheap; build one where you have free workers.", consumers.join(", ")));
            format!("{} is {pct:+.0}%: cheap input for {}.", g.label, consumers.join(", "))
        } else {
            let (state, building, _, _) = &yours[0];
            fixes.push(format!("Queue no more levels of {building}."));
            fixes.push(format!("Construction panel: build {} in {state}, next to your {building}: it uses the cheap {}.", consumers.first().cloned().unwrap_or_default(), g.label));
            format!("{} is {pct:+.0}%: you make more than your market uses.", g.label)
        };
        out.push(MarketCase {
            good: g.name.clone(),
            label: g.label.clone(),
            price_vs_base: r - 1.0,
            dear,
            producers: producer_labels,
            dear_inputs,
            consumers,
            yours,
            advice,
            fixes,
        });
    }
    out
}

#[cfg(test)]
mod market_tests {
    use super::*;

    #[test]
    fn producers_and_cases_come_from_the_methods_and_prices() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let save = std::fs::read(dir.join("../vic3-load/tests/fixtures/plaintext.txt")).expect("save");
        let defs = vic3_defs::load_from_path(dir.join("../vic3-defs/tests/fixtures")).expect("defs");
        let blob = vic3_defs::encode_blob(&defs).expect("blob");
        let mut snap = crate::load_analysis_snapshot(&save, None, &blob, "{}", false).expect("snapshot");
        // Some building whose method outputs a good is found as its producer.
        let (building, good) = snap
            .defs
            .building_types
            .iter()
            .filter(|(name, _)| !name.starts_with("building_subsistence"))
            .find_map(|(name, b)| {
                let pm = b.production_method_groups.iter().filter_map(|g| snap.defs.production_method_groups.get(g)).flatten().find_map(|pm| snap.defs.production_methods.get(pm))?;
                let (g, _) = pm.outputs.first()?;
                Some((name.clone(), snap.defs.goods_order[g.as_usize()].clone()))
            })
            .expect("a producing building in the fixture defs");
        assert!(buildings_with(&snap.defs, &good, true).contains(&building));
        // Make that good dear: it becomes the first case, with its producer named.
        let g = snap.prices.goods.iter_mut().find(|g| g.name == good).expect("good priced");
        g.price = g.base * 3.0;
        g.buy = g.buy.max(1.0);
        let cases = market_doctor(&snap.defs, &snap.prices, &BTreeSet::new());
        let first = cases.first().expect("case");
        assert_eq!(first.good, good);
        assert!(first.dear && (first.price_vs_base - 2.0).abs() < 1e-9);
        assert!(first.producers.contains(&snap.defs.display_label(&building)), "{first:?}");
        assert!(!first.advice.is_empty());
    }
}
