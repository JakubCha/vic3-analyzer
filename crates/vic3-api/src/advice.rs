//! Advisor: ranked, player-scoped suggestions per domain (economy / laws / war),
//! each with an optional [`Check`] the next save can confirm as done.
//!
//! Economy and war advice come from the solved market (profit per level, local
//! prices and deficits, idle workers); laws advice is the next step toward a
//! [`LawGoal`] ranked by interest-group support ([`LAW_PATHS`]). Checks are evaluated
//! against whatever save is loaded when they are asked, so the UI can track a
//! suggestion across autosaves.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use vic3_defs::GameDefs;
use vic3_prices::{
    alerts_with, Alert, AlertKind, AlertsOptions, World, WorldCountry,
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
    /// Which goal this serves; `None` = shown for every goal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<Goal>,
    /// The game mechanism behind the advice, for players learning the game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// `None` → the player marks it done by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Check>,
}

/// Advisor payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdviceResult {
    pub advice: Vec<Advice>,
    pub limitations: Vec<String>,
    /// Budget guard for the played country.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<crate::budget::BudgetReport>,
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
    /// The player's country has `law` active.
    LawActive { law: String },
    /// The player's country has credit headroom again.
    Solvent,
    /// The player's weekly balance is at least `at_least`.
    WeeklyBalance { at_least: f64 },
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

/// What the player wants the laws to achieve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LawGoal {
    /// Growth: construction, trade, literacy, revenue.
    Economy,
    /// Maximum control for the ruler: autocracy, police state, no dissent.
    Authority,
    /// A liberal democracy: universal suffrage, liberties, equal citizenship.
    Democracy,
}

/// What the player wants the economy to achieve (Economy tab).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EconomyGoal {
    /// Profit per construction level: GDP growth.
    Growth,
    /// Cheaper goods that pops buy, where their needs go unmet.
    LivingStandards,
    /// Construction capacity and its inputs, then heavy industry.
    Industrialization,
    /// Close local deficits, strategic goods first, even at lower profit.
    SelfSufficiency,
}

/// Goal tag on an [`Advice`]; serialized as the bare goal name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Goal {
    Law(LawGoal),
    Economy(EconomyGoal),
}

/// Per goal: (law group, path from start to target). The next step is the path
/// law after the current one; a current law off the path is placed by its
/// `progressiveness`. Laws missing from the defs (other patches, mods) drop out.
const LAW_PATHS: &[(LawGoal, &str, &[&str])] = &[
    (LawGoal::Economy, "lawgroup_economic_system", &["law_traditionalism", "law_agrarianism", "law_interventionism", "law_laissez_faire"]),
    (LawGoal::Economy, "lawgroup_trade_policy", &["law_isolationism", "law_mercantilism", "law_protectionism"]),
    (LawGoal::Economy, "lawgroup_land_reform", &["law_serfdom", "law_tenant_farmers", "law_homesteading"]),
    (LawGoal::Economy, "lawgroup_taxation", &["law_land_based_taxation", "law_per_capita_based_taxation", "law_proportional_taxation"]),
    (LawGoal::Economy, "lawgroup_education_system", &["law_no_schools", "law_religious_schools", "law_private_schools", "law_public_schools"]),
    (LawGoal::Economy, "lawgroup_bureaucracy", &["law_hereditary_bureaucrats", "law_appointed_bureaucrats"]),
    (LawGoal::Authority, "lawgroup_distribution_of_power", &["law_universal_suffrage", "law_census_voting", "law_wealth_voting", "law_landed_voting", "law_oligarchy", "law_autocracy"]),
    (LawGoal::Authority, "lawgroup_free_speech", &["law_protected_speech", "law_right_of_assembly", "law_censorship", "law_outlawed_dissent"]),
    (LawGoal::Authority, "lawgroup_internal_security", &["law_guaranteed_liberties", "law_no_home_affairs", "law_national_guard", "law_secret_police"]),
    (LawGoal::Authority, "lawgroup_bureaucracy", &["law_elected_bureaucrats", "law_appointed_bureaucrats"]),
    (LawGoal::Democracy, "lawgroup_distribution_of_power", &["law_autocracy", "law_oligarchy", "law_landed_voting", "law_wealth_voting", "law_census_voting", "law_universal_suffrage"]),
    (LawGoal::Democracy, "lawgroup_free_speech", &["law_outlawed_dissent", "law_censorship", "law_right_of_assembly", "law_protected_speech"]),
    (LawGoal::Democracy, "lawgroup_internal_security", &["law_secret_police", "law_national_guard", "law_no_home_affairs", "law_guaranteed_liberties"]),
    (LawGoal::Democracy, "lawgroup_bureaucracy", &["law_hereditary_bureaucrats", "law_appointed_bureaucrats", "law_elected_bureaucrats"]),
    (LawGoal::Democracy, "lawgroup_citizenship", &["law_ethnostate", "law_national_supremacy", "law_racial_segregation", "law_cultural_exclusion", "law_multicultural"]),
    (LawGoal::Democracy, "lawgroup_church_and_state", &["law_state_religion", "law_freedom_of_conscience", "law_total_separation"]),
    (LawGoal::Democracy, "lawgroup_rights_of_women", &["law_no_womens_rights", "law_women_own_property", "law_women_in_the_workplace", "law_womens_suffrage"]),
];

/// Next law on `path` after the player's current law in that group.
fn next_step<'a>(defs: &GameDefs, path: &[&'a str], current: &str) -> Option<&'a str> {
    let path: Vec<&str> = path.iter().copied().filter(|law| defs.laws.contains_key(*law)).collect();
    if let Some(i) = path.iter().position(|law| *law == current) {
        return path.get(i + 1).copied();
    }
    // Off the path (regional variant etc.): first path law past it toward the target.
    let prog = |law: &str| defs.laws.get(law).map(|l| l.progressiveness);
    let (cur, first, last) = (prog(current)?, prog(path.first()?)?, prog(path.last()?)?);
    let rising = last >= first;
    path.into_iter()
        .find(|law| prog(law).is_some_and(|p| if rising { p > cur } else { p < cur }))
}

/// Interest-group verdict on one law: (label, clout, in government) per side.
struct Support {
    for_: Vec<(String, f64, bool)>,
    against: Vec<(String, f64, bool)>,
}

impl Support {
    fn of(defs: &GameDefs, country: &WorldCountry, law: &str) -> Self {
        let mut support = Support { for_: Vec::new(), against: Vec::new() };
        for ig in &country.interest_groups {
            let stance: i32 = ig
                .ideologies
                .iter()
                .filter_map(|ideology| defs.ideology_law_stances.get(ideology)?.get(law))
                .map(|s| i32::from(*s))
                .sum();
            let row = (vic3_prices::script_label(defs, &ig.definition), ig.clout, ig.in_government);
            match stance.signum() {
                1 => support.for_.push(row),
                -1 => support.against.push(row),
                _ => {}
            }
        }
        for side in [&mut support.for_, &mut support.against] {
            side.sort_by(|a, b| b.1.total_cmp(&a.1));
        }
        support
    }
    fn clout(side: &[(String, f64, bool)]) -> f64 {
        side.iter().map(|r| r.1).sum()
    }
    fn list(side: &[(String, f64, bool)]) -> String {
        if side.is_empty() {
            return "nobody".into();
        }
        side.iter()
            .map(|(name, clout, gov)| {
                format!("{name} {:.0}%{}", clout * 100.0, if *gov { " (in government)" } else { "" })
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Laws advice: the next step toward each goal, ranked by real IG support.
fn law_advice(defs: &GameDefs, country: &WorldCountry, education_short: bool, out: &mut Vec<Advice>) {
    let label = |law: &str| vic3_prices::script_label(defs, law);
    for &(goal, group, path) in LAW_PATHS {
        let Some(current) = country
            .laws
            .iter()
            .find(|law| defs.laws.get(law.as_str()).is_some_and(|l| l.group == group))
        else {
            continue;
        };
        let Some(next) = next_step(defs, path, current) else { continue };
        let support = Support::of(defs, country, next);
        let (pro, con) = (Support::clout(&support.for_), Support::clout(&support.against));
        let gov_backer = support.for_.iter().any(|r| r.2);
        let locked = defs.laws.get(next).filter(|l| {
            !l.unlocking_technologies.is_empty()
                && !l.unlocking_technologies.iter().any(|t| country.techs.contains(t))
        });
        let (verdict, mut priority) = if let Some(law) = locked {
            let techs: Vec<String> = law.unlocking_technologies.iter().map(|t| label(t)).collect();
            (format!("Not available yet: research {}.", techs.join(" or ")), 2)
        } else if gov_backer && pro >= con * 1.5 {
            ("Good odds: backed by the government and outweighs the opposition.".to_string(), 1)
        } else if support.for_.is_empty() && support.against.is_empty() {
            ("No interest group cares either way: it rests on legitimacy alone.".to_string(), 2)
        } else if pro >= con {
            (
                if gov_backer {
                    "Contested: expect a slow enactment; boost legitimacy or the backers' clout first."
                } else {
                    "No backer in government: add a supporting group to the government before enacting."
                }
                .to_string(),
                2,
            )
        } else {
            let backer = support.for_.iter().find(|r| !r.2).map(|r| r.0.clone());
            (
                match backer {
                    Some(name) => format!("Opposed: likely to fail and radicalise. Bring {name} into government first or wait for its clout to grow."),
                    None => "Opposed and nobody backs it: not enactable now.".to_string(),
                },
                2,
            )
        };
        if goal == LawGoal::Economy && group == "lawgroup_education_system" && education_short && locked.is_none() {
            priority = 1;
        }
        let goal_name = match goal {
            LawGoal::Economy => "economy",
            LawGoal::Authority => "authority",
            LawGoal::Democracy => "democracy",
        };
        out.push(Advice {
            id: format!("law:{goal_name}:{group}"),
            domain: Domain::Laws,
            priority,
            title: format!("Enact {} (now: {})", label(next), label(current)),
            detail: format!(
                "Next step toward {goal_name}. For: {} — against: {}. {verdict}",
                Support::list(&support.for_),
                Support::list(&support.against)
            ),
            goal: Some(Goal::Law(goal)),
            why: None,
            check: Some(Check::LawActive { law: next.to_string() }),
        });
    }
}

/// Player country and its owned state ids (same strict filter as SQL `alerts()`).
pub(crate) fn player(world: &World) -> Option<(&WorldCountry, BTreeSet<u32>)> {
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

pub(crate) fn building_levels(
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

/// One good across the player's states: local orders and order-weighted price.
#[derive(Default)]
struct LocalGood {
    buy: f64,
    sell: f64,
    value: f64,
    weight: f64,
}

/// One building type in one player state, summed over its building instances.
struct Site {
    type_name: String,
    type_label: String,
    state_id: u32,
    levels: f64,
    staffing: f64,
    profit: f64,
    revenue: f64,
    outputs: BTreeSet<String>,
    short_inputs: BTreeSet<String>,
}

impl Site {
    fn profit_per_level(&self) -> f64 {
        self.profit / self.levels.max(1.0)
    }
    fn staffed(&self) -> f64 {
        if self.levels > 0.0 { self.staffing / self.levels } else { 0.0 }
    }
}

fn pct(x: f64) -> String {
    format!("{:+.0}%", x * 100.0)
}

/// Goods a goal-less war run cares about, and the extra strategic weight in
/// [`EconomyGoal::SelfSufficiency`].
const STRATEGIC_GOODS: &[&str] = &[
    "coal", "iron", "sulfur", "lead", "hardwood", "wood", "tools", "steel", "oil", "rubber", "grain",
];

/// Construction inputs and heavy industry ([`EconomyGoal::Industrialization`]).
const INDUSTRIAL_GOODS: &[&str] = &[
    "wood", "hardwood", "fabric", "iron", "coal", "tools", "steel", "glass", "engines", "explosives",
    "lead", "sulfur", "electricity",
];

/// One advice run: which goods count, how sites and shortages are scored.
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Economy(EconomyGoal),
    War,
}

impl Focus {
    fn tag(self) -> &'static str {
        match self {
            Focus::War => "war",
            Focus::Economy(EconomyGoal::Growth) => "growth",
            Focus::Economy(EconomyGoal::LivingStandards) => "living_standards",
            Focus::Economy(EconomyGoal::Industrialization) => "industrialization",
            Focus::Economy(EconomyGoal::SelfSufficiency) => "self_sufficiency",
        }
    }
    fn domain(self) -> Domain {
        if self == Focus::War { Domain::War } else { Domain::Economy }
    }
    fn goal(self) -> Option<Goal> {
        match self {
            Focus::War => None,
            Focus::Economy(goal) => Some(Goal::Economy(goal)),
        }
    }
}

/// The solved market as seen from the player's states.
struct Market<'a> {
    world: &'a World,
    defs: &'a GameDefs,
    prices: &'a vic3_prices::PricesResult,
    states: &'a BTreeSet<u32>,
    idle_states: &'a BTreeSet<u32>,
    needy_states: &'a BTreeSet<u32>,
    local: BTreeMap<&'a str, LocalGood>,
    sites: Vec<Site>,
    consumer_goods: BTreeSet<String>,
}

impl<'a> Market<'a> {
    fn new(
        world: &'a World,
        defs: &'a GameDefs,
        prices: &'a vic3_prices::PricesResult,
        states: &'a BTreeSet<u32>,
        idle_states: &'a BTreeSet<u32>,
        needy_states: &'a BTreeSet<u32>,
    ) -> Self {
        let mut local: BTreeMap<&str, LocalGood> = BTreeMap::new();
        for sg in prices.state_goods.iter().filter(|g| states.contains(&g.state_id)) {
            let entry = local.entry(sg.name.as_str()).or_default();
            entry.buy += sg.buy;
            entry.sell += sg.sell;
            entry.value += sg.price * (sg.buy + sg.sell);
            entry.weight += sg.buy + sg.sell;
        }
        let mut sites: BTreeMap<(String, u32), Site> = BTreeMap::new();
        for b in &prices.buildings {
            let Some(state_id) = b.state_id.filter(|s| states.contains(s)) else { continue };
            // Subsistence farms are not investments.
            if b.building_type_name.starts_with("building_subsistence") {
                continue;
            }
            let site = sites
                .entry((b.building_type_name.clone(), state_id))
                .or_insert_with(|| Site {
                    type_name: b.building_type_name.clone(),
                    type_label: b.building_type_label.clone(),
                    state_id,
                    levels: 0.0,
                    staffing: 0.0,
                    profit: 0.0,
                    revenue: 0.0,
                    outputs: BTreeSet::new(),
                    short_inputs: BTreeSet::new(),
                });
            site.levels += b.level;
            site.staffing += b.staffing;
            site.profit += b.profit;
            site.revenue += b.revenue;
            site.outputs.extend(b.outputs.iter().filter(|o| o.quantity > 0.0).map(|o| o.name.clone()));
            site.short_inputs.extend(b.short_inputs.iter().cloned());
        }
        let consumer_goods = defs
            .pop_needs
            .iter()
            .flat_map(|need| need.entries.iter())
            .filter_map(|entry| defs.goods_order.get(entry.good.as_usize()).cloned())
            .collect();
        Market {
            world,
            defs,
            prices,
            states,
            idle_states,
            needy_states,
            local,
            sites: sites.into_values().collect(),
            consumer_goods,
        }
    }

    fn state_label(&self, id: u32) -> String {
        self.prices
            .states
            .iter()
            .find(|s| s.id == id)
            .map_or_else(|| format!("state {id}"), |s| s.label.clone())
    }

    fn good_label(&self, name: &str) -> String {
        self.prices
            .goods
            .iter()
            .find(|g| g.name == name)
            .map_or_else(|| name.to_string(), |g| g.label.clone())
    }

    /// Local price relative to base: +0.4 = 40% above base (a shortage signal).
    fn premium(&self, name: &str) -> f64 {
        let (Some(g), Some(l)) = (self.prices.goods.iter().find(|g| g.name == name), self.local.get(name)) else {
            return 0.0;
        };
        let price = if l.weight > 0.0 { l.value / l.weight } else { g.price };
        if g.base > 0.0 { price / g.base - 1.0 } else { 0.0 }
    }

    /// Share of local demand the player's states do not produce (0 = covered).
    fn deficit_share(&self, name: &str) -> f64 {
        self.local
            .get(name)
            .filter(|l| l.buy > 0.0)
            .map_or(0.0, |l| ((l.buy - l.sell) / l.buy).clamp(0.0, 1.0))
    }

    fn current(&self, s: &Site) -> f64 {
        building_levels(self.world, self.defs, self.states, &s.type_name, Some(s.state_id))
    }

    fn counts(&self, focus: Focus, good: &str) -> bool {
        let military = MILITARY_GOODS.contains(&good);
        match focus {
            Focus::War => military,
            Focus::Economy(_) if military => false,
            Focus::Economy(EconomyGoal::Growth | EconomyGoal::SelfSufficiency) => true,
            Focus::Economy(EconomyGoal::LivingStandards) => self.consumer_goods.contains(good),
            Focus::Economy(EconomyGoal::Industrialization) => INDUSTRIAL_GOODS.contains(&good),
        }
    }

    /// The site's most relevant output for `focus` and its score signal.
    fn best_output(&self, focus: Focus, s: &Site) -> Option<(String, f64)> {
        s.outputs
            .iter()
            .filter(|g| self.counts(focus, g))
            .map(|g| {
                let signal = match focus {
                    Focus::Economy(EconomyGoal::SelfSufficiency) => self.deficit_share(g),
                    _ => self.premium(g),
                };
                (g.clone(), signal)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Expansion score for a profitable, fully staffed site; `None` = not relevant to `focus`.
    fn expand_score(&self, focus: Focus, s: &Site) -> Option<f64> {
        let (good, signal) = self.best_output(focus, s)?;
        let ppl = s.profit_per_level();
        let idle = if self.idle_states.contains(&s.state_id) { 1.25 } else { 1.0 };
        Some(match focus {
            Focus::Economy(EconomyGoal::LivingStandards) => {
                if signal <= 0.0 {
                    return None; // already cheap: more of it barely helps pops
                }
                let needy = if self.needy_states.contains(&s.state_id) { 1.25 } else { 1.0 };
                ppl.max(1.0) * (1.0 + 2.0 * signal.min(0.75)) * needy * idle
            }
            // Cheap military goods are already oversupplied: no more factories.
            Focus::War if signal <= 0.0 => return None,
            Focus::Economy(EconomyGoal::SelfSufficiency) => {
                if signal <= 0.0 {
                    return None;
                }
                let strategic = if STRATEGIC_GOODS.contains(&good.as_str()) { 2.0 } else { 1.0 };
                ppl.max(1.0) * signal * strategic * idle
            }
            _ => ppl * (1.0 + signal.clamp(0.0, 0.75)) * idle,
        })
    }

    /// Why this goal wants more of `good`, appended to the detail text.
    fn goal_reason(&self, focus: Focus, good: &str) -> String {
        match focus {
            Focus::Economy(EconomyGoal::LivingStandards) => format!(
                " Cheaper {} raises standard of living for the pops that buy it.",
                self.good_label(good)
            ),
            Focus::Economy(EconomyGoal::Industrialization) => format!(
                " {} feeds construction and heavy industry.",
                self.good_label(good)
            ),
            Focus::Economy(EconomyGoal::SelfSufficiency) => format!(
                " You produce only {:.0}% of the {} you use.",
                (1.0 - self.deficit_share(good)) * 100.0,
                self.good_label(good)
            ),
            _ => String::new(),
        }
    }

    /// Sections 1–2 for one goal: expand the best sites, then fill shortages.
    fn focus_advice(&self, focus: Focus, out: &mut Vec<Advice>) {
        let tag = focus.tag();
        let mut ranked: Vec<(f64, &Site)> = self
            .sites
            .iter()
            .filter(|s| s.revenue > 0.0 && s.profit > 0.0 && s.staffed() >= 0.9)
            .filter_map(|s| Some((self.expand_score(focus, s)?, s)))
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut suggested: BTreeSet<(String, u32)> = BTreeSet::new();
        for (rank, (_, site)) in ranked.iter().take(5).enumerate() {
            let mut detail = format!(
                "Each level earns about {:.0} profit and is {:.0}% staffed.",
                site.profit_per_level(),
                site.staffed() * 100.0
            );
            if let Some((good, _)) = self.best_output(focus, site) {
                detail.push_str(&format!(
                    " It sells {} at {} vs base price.",
                    self.good_label(&good),
                    pct(self.premium(&good))
                ));
                detail.push_str(&self.goal_reason(focus, &good));
            }
            if self.idle_states.contains(&site.state_id) {
                detail.push_str(" Workers are idle in this state, so new levels will fill.");
            }
            suggested.insert((site.type_name.clone(), site.state_id));
            out.push(Advice {
                id: format!("{tag}:expand:{}:{}", site.type_name, site.state_id),
                domain: focus.domain(),
                goal: focus.goal(),
                priority: if rank < 2 { 1 } else { 2 },
                title: format!("Expand {} in {}", site.type_label, self.state_label(site.state_id)),
                detail,
                why: None,
                check: Some(Check::BuildingLevels {
                    building_type: site.type_name.clone(),
                    state_id: Some(site.state_id),
                    at_least: self.current(site) + 1.0,
                }),
            });
        }

        let (min_gap, min_premium) = match focus {
            Focus::Economy(EconomyGoal::Growth) => (1.15, 0.15),
            Focus::Economy(EconomyGoal::SelfSufficiency) => (1.05, f64::NEG_INFINITY),
            _ => (1.05, 0.05),
        };
        let mut short: Vec<(&str, &LocalGood, f64)> = self
            .local
            .iter()
            .filter(|(name, _)| self.counts(focus, name))
            .map(|(name, l)| (*name, l, self.premium(name)))
            .filter(|(_, l, p)| l.buy > l.sell * min_gap && *p > min_premium)
            .collect();
        let urgency = |x: &(&str, &LocalGood, f64)| match focus {
            Focus::Economy(EconomyGoal::SelfSufficiency) => {
                let strategic = if STRATEGIC_GOODS.contains(&x.0) { 2.0 } else { 1.0 };
                self.deficit_share(x.0) * strategic * x.1.buy
            }
            _ => (x.1.buy - x.1.sell) * (1.0 + x.2),
        };
        short.sort_by(|a, b| urgency(b).total_cmp(&urgency(a)));
        for (good, l, p) in short.into_iter().take(5) {
            let why = format!(
                "{} costs {} vs base price; your states buy {:.0} but produce {:.0}.{}",
                self.good_label(good),
                pct(p),
                l.buy,
                l.sell,
                self.goal_reason(focus, good)
            );
            let priority = if p > 0.4 || focus == Focus::War || self.deficit_share(good) > 0.5 { 1 } else { 2 };
            let producer = self
                .sites
                .iter()
                .filter(|s| s.outputs.contains(good))
                .max_by(|a, b| a.profit_per_level().total_cmp(&b.profit_per_level()));
            match producer {
                Some(site) if suggested.contains(&(site.type_name.clone(), site.state_id)) => {}
                Some(site) => {
                    suggested.insert((site.type_name.clone(), site.state_id));
                    out.push(Advice {
                        id: format!("{tag}:short:{good}:{}:{}", site.type_name, site.state_id),
                        domain: focus.domain(),
                        goal: focus.goal(),
                        priority,
                        title: format!(
                            "Build more {} in {} for {}",
                            site.type_label,
                            self.state_label(site.state_id),
                            self.good_label(good)
                        ),
                        detail: format!(
                            "{why} This is your best-earning {} ({:.0} profit per level).",
                            site.type_label,
                            site.profit_per_level()
                        ),
                        why: None,
                        check: Some(Check::BuildingLevels {
                            building_type: site.type_name.clone(),
                            state_id: Some(site.state_id),
                            at_least: self.current(site) + 1.0,
                        }),
                    });
                }
                None => {
                    // Nobody at home makes it: name the most common producer elsewhere.
                    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
                    for b in self
                        .prices
                        .buildings
                        .iter()
                        .filter(|b| b.outputs.iter().any(|o| o.name == good && o.quantity > 0.0))
                    {
                        *counts
                            .entry((b.building_type_name.as_str(), b.building_type_label.as_str()))
                            .or_default() += 1;
                    }
                    let Some(((type_name, type_label), _)) = counts.into_iter().max_by_key(|(_, n)| *n) else {
                        continue;
                    };
                    out.push(Advice {
                        id: format!("{tag}:start:{good}:{type_name}"),
                        domain: focus.domain(),
                        goal: focus.goal(),
                        priority,
                        title: format!("Start producing {}: build a {}", self.good_label(good), type_label),
                        detail: format!(
                            "{why} None of your states has a {type_label} yet; pick a state with the resource and idle workers."
                        ),
                        why: None,
                        check: Some(Check::BuildingLevels {
                            building_type: type_name.to_string(),
                            state_id: None,
                            at_least: building_levels(self.world, self.defs, self.states, type_name, None) + 1.0,
                        }),
                    });
                }
            }
        }
    }

    /// Industrialization: construction capacity is what every other build waits on.
    fn construction_advice(&self, out: &mut Vec<Advice>) {
        let Some(site) = self
            .sites
            .iter()
            .filter(|s| s.type_name == "building_construction_sector")
            .max_by(|a, b| {
                let key = |s: &Site| (self.idle_states.contains(&s.state_id), s.staffed());
                key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal)
            })
        else {
            return;
        };
        let focus = Focus::Economy(EconomyGoal::Industrialization);
        out.push(Advice {
            id: format!("{}:construction:{}", focus.tag(), site.state_id),
            domain: Domain::Economy,
            goal: focus.goal(),
            priority: 1,
            title: format!("Expand the Construction Sector in {}", self.state_label(site.state_id)),
            detail: format!(
                "More construction points speed up every other build. It is {:.0}% staffed here{}; keep its inputs (wood, iron, tools, later steel and glass) cheap.",
                site.staffed() * 100.0,
                if self.idle_states.contains(&site.state_id) { " and workers are idle" } else { "" }
            ),
            why: None,
            check: Some(Check::BuildingLevels {
                building_type: site.type_name.clone(),
                state_id: Some(site.state_id),
                at_least: self.current(site) + 1.0,
            }),
        });
    }

    /// Sections 3–4 (every goal): loss-makers and oversupplied goods.
    fn warnings(&self, out: &mut Vec<Advice>) {
        let mut losing: Vec<&Site> = self
            .sites
            .iter()
            .filter(|s| s.revenue > 0.0 && s.profit < 0.0 && s.levels >= 1.0)
            .collect();
        losing.sort_by(|a, b| a.profit.total_cmp(&b.profit));
        for site in losing.into_iter().take(3) {
            let inputs = if site.short_inputs.is_empty() {
                String::new()
            } else {
                let names: Vec<String> = site.short_inputs.iter().map(|g| self.good_label(g)).collect();
                format!(" Its inputs are short: {}.", names.join(", "))
            };
            out.push(Advice {
                id: format!("loss:{}:{}", site.type_name, site.state_id),
                domain: Domain::Economy,
                goal: None,
                priority: 2,
                title: format!("Fix the loss-making {} in {}", site.type_label, self.state_label(site.state_id)),
                detail: format!(
                    "It loses about {:.0} per level.{inputs} Switch to a cheaper production method, secure its inputs, or stop expanding it.",
                    -site.profit_per_level()
                ),
                why: None,
                check: None,
            });
        }

        let mut glut: Vec<(&str, &LocalGood, f64)> = self
            .local
            .iter()
            .map(|(name, l)| (*name, l, self.premium(name)))
            .filter(|(_, l, p)| l.sell > l.buy * 1.3 && *p < -0.2)
            .collect();
        glut.sort_by(|a, b| a.2.total_cmp(&b.2));
        for (good, l, p) in glut.into_iter().take(2) {
            out.push(Advice {
                id: format!("glut:{good}"),
                domain: if MILITARY_GOODS.contains(&good) { Domain::War } else { Domain::Economy },
                goal: None,
                priority: 2,
                title: format!("Don't build more {} producers", self.good_label(good)),
                detail: format!(
                    "{} sells at {} vs base price: your states produce {:.0} but buy only {:.0}. New levels would earn less than they cost.",
                    self.good_label(good),
                    pct(p),
                    l.sell,
                    l.buy
                ),
                why: None,
                check: None,
            });
        }
    }
}

/// Market-driven advice from the solved prices for every economy goal plus war
/// supply. One pass per goal over buildings and local goods; no re-solves.
fn market_advice(
    world: &World,
    defs: &GameDefs,
    prices: &vic3_prices::PricesResult,
    states: &BTreeSet<u32>,
    idle_states: &BTreeSet<u32>,
    needy_states: &BTreeSet<u32>,
    out: &mut Vec<Advice>,
) {
    let market = Market::new(world, defs, prices, states, idle_states, needy_states);
    market.construction_advice(out);
    for goal in [
        EconomyGoal::Growth,
        EconomyGoal::LivingStandards,
        EconomyGoal::Industrialization,
        EconomyGoal::SelfSufficiency,
    ] {
        market.focus_advice(Focus::Economy(goal), out);
    }
    market.focus_advice(Focus::War, out);
    market.warnings(out);
}

/// Ranked advice for the played country.
///
/// Returns an empty list with a limitation when the save has no player tag.
pub fn advice(world: &World, defs: &GameDefs, prices: &vic3_prices::PricesResult) -> AdviceResult {
    let mut limitations = vec![
        "Law support sums each interest group's ideology stances (its own and its leader's) weighted by clout; it ignores legitimacy, movements and enactment events.".to_string(),
        "Economy prices are one whole-save solve: \"your states buy/produce\" is local demand and output, not trade routes.".to_string(),
        "War advice covers military-goods supply and finances only, not enemy strength.".to_string(),
        "Playbook items (\"Playbook · …\") are rules written from strategy guides and the 1.13.9 game files for Prussia, Britain, France, Austria, Sardinia/Italy and the USA plus general ones; their thresholds are judgement calls, and journal-entry choices, relations and radicals are not read from the save.".to_string(),
    ];
    let Some((country, states)) = player(world) else {
        limitations.push("No played country in this save; nothing to advise.".into());
        return AdviceResult {
            advice: Vec::new(),
            limitations,
            budget: None,
        };
    };

    // Detectors only: alert mitigations re-solve the world and take ~1 s each.
    let scan = alerts_with(
        world,
        defs,
        prices,
        AlertsOptions {
            with_mitigations: false,
            mitigation_ids: None,
        },
    );
    let mine: Vec<&Alert> = scan
        .alerts
        .iter()
        .filter(|a| a.state_id.is_some_and(|id| states.contains(&id)))
        .collect();
    let in_states = |kind: AlertKind| -> BTreeMap<u32, usize> {
        let mut counts = BTreeMap::new();
        for a in mine.iter().filter(|a| a.kind == kind) {
            *counts.entry(a.state_id.unwrap_or_default()).or_default() += 1;
        }
        counts
    };
    let idle_states: BTreeSet<u32> = in_states(AlertKind::Underemployed).into_keys().collect();
    let unfilled = in_states(AlertKind::UnfilledEducation);
    let education_short = !unfilled.is_empty();

    let mut advice = Vec::new();
    let needy_states: BTreeSet<u32> = in_states(AlertKind::NeedsUnmet).into_keys().collect();
    market_advice(world, defs, prices, &states, &idle_states, &needy_states, &mut advice);

    if let Some((&state_id, &count)) = unfilled.iter().max_by_key(|(_, n)| **n) {
        let total: usize = unfilled.values().sum();
        let label = prices
            .states
            .iter()
            .find(|s| s.id == state_id)
            .map_or_else(|| format!("state {state_id}"), |s| s.label.clone());
        advice.push(Advice {
            goal: None,
            id: format!("education:{state_id}"),
            domain: Domain::Economy,
            priority: if total >= 10 { 1 } else { 2 },
            title: format!("Train skilled workers in {label}"),
            detail: format!(
                "{total} of your buildings can't fill skilled jobs (engineers, clerks, machinists) for lack of literate pops, most of them ({count}) in {label}. A university there and a schooling law raise qualifications."
            ),
            why: None,
            check: Some(Check::BuildingLevels {
                building_type: "building_university".into(),
                state_id: Some(state_id),
                at_least: building_levels(world, defs, &states, "building_university", Some(state_id)) + 1.0,
            }),
        });
    }

    law_advice(defs, country, education_short, &mut advice);

    // Budget guard first: its ids ("budget:…") sort ahead within each priority.
    let budget = crate::budget::report(defs, country);
    if let Some(report) = &budget {
        crate::budget::budget_advice(report, &mut advice);
    }
    crate::playbook::playbook_advice(
        &crate::playbook::Facts {
            world,
            defs,
            prices,
            country,
            states: &states,
            budget: budget.as_ref(),
        },
        &mut advice,
    );

    // Within a priority: budget guard, then playbook, then market advice.
    let rank = |a: &Advice| match a.id.split(':').next() {
        Some("budget") => 0,
        Some("playbook") => 1,
        _ => 2,
    };
    advice.sort_by(|a, b| (a.priority, rank(a), &a.id).cmp(&(b.priority, rank(b), &b.id)));
    AdviceResult { advice, limitations, budget }
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
            Check::LawActive { law } => country.laws.iter().any(|active| active == law),
            Check::Solvent => country.credit_headroom.is_none_or(|h| h > 0.0),
            Check::WeeklyBalance { at_least } => country.weekly_balance.is_some_and(|b| b >= *at_least),
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
    fn law_goal_steps_use_interest_group_support() {
        // Fixture defs: law_autocracy (-100), law_census_voting (50, needs `democracy`);
        // ideology_paternalistic strongly approves autocracy, disapproves census voting.
        let mut snap = snapshot();
        let me = player_mut(&mut snap.world);
        me.laws.push("law_autocracy".into());
        me.interest_groups = vec![vic3_prices::WorldInterestGroup {
            definition: "ig_landowners".into(),
            clout: 0.4,
            in_government: true,
            approval: Some(2.0),
            ideologies: vec!["ideology_paternalistic".into()],
        }];
        let laws = |snap: &crate::AnalysisSnapshot| {
            advice(&snap.world, &snap.defs, &snap.prices)
                .advice
                .into_iter()
                .filter(|a| a.domain == Domain::Laws)
                .collect::<Vec<_>>()
        };

        // Locked by tech; authority is already at its target (autocracy).
        let items = laws(&snap);
        let step = items
            .iter()
            .find(|a| a.id == "law:democracy:lawgroup_distribution_of_power")
            .expect("democracy step");
        assert_eq!(step.goal, Some(Goal::Law(LawGoal::Democracy)));
        assert!(step.detail.contains("research"), "{}", step.detail);
        assert!(!items.iter().any(|a| a.goal == Some(Goal::Law(LawGoal::Authority))));

        // Unlocked: the landowners in government oppose it.
        player_mut(&mut snap.world).techs.push("democracy".into());
        let items = laws(&snap);
        let step = items
            .iter()
            .find(|a| a.id == "law:democracy:lawgroup_distribution_of_power")
            .expect("democracy step");
        assert!(step.detail.contains("against:") && step.detail.contains("40%"), "{}", step.detail);
        assert!(step.detail.contains("Opposed"), "{}", step.detail);
        let check = step.check.clone().expect("law check");
        assert_eq!(check, Check::LawActive { law: "law_census_voting".into() });
        assert_eq!(checks_done(&snap.world, &snap.defs, std::slice::from_ref(&check)), [false]);

        player_mut(&mut snap.world).laws.push("law_census_voting".into());
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
        player_mut(&mut snap.world).credit_headroom = Some(-100.0);
        let result = advice(&snap.world, &snap.defs, &snap.prices);
        let domains: Vec<Domain> = result
            .advice
            .iter()
            .filter(|a| a.check == Some(Check::Solvent))
            .map(|a| a.domain)
            .collect();
        assert_eq!(domains, [Domain::Economy, Domain::War]);
        assert!(result.advice.windows(2).all(|w| w[0].priority <= w[1].priority));

        // Credit missing from the save is not insolvency.
        player_mut(&mut snap.world).credit_headroom = None;
        let result = advice(&snap.world, &snap.defs, &snap.prices);
        assert!(result.advice.iter().all(|a| a.check != Some(Check::Solvent)));
    }

    #[test]
    fn profitable_staffed_site_is_advised_for_expansion() {
        let mut snap = snapshot();
        let (_, states) = player(&snap.world).expect("player");
        let b = snap
            .prices
            .buildings
            .iter_mut()
            .find(|b| b.state_id.is_some_and(|s| states.contains(&s)))
            .expect("player building");
        b.building_type_name = "building_coal_mine".into();
        b.level = 2.0;
        b.staffing = 2.0;
        b.revenue = 5000.0;
        b.profit = 3000.0;
        let state = b.state_id.unwrap();
        let result = advice(&snap.world, &snap.defs, &snap.prices);
        let item = result
            .advice
            .iter()
            .find(|a| a.id == format!("growth:expand:building_coal_mine:{state}"))
            .expect("expand advice");
        assert_eq!(item.domain, Domain::Economy);
        assert_eq!(item.goal, Some(Goal::Economy(EconomyGoal::Growth)));
        assert_eq!(serde_json::to_value(item.goal).unwrap(), "growth");
        assert!(item.detail.contains("1500 profit"), "{}", item.detail);
        // Goal-specific runs tag their own items; loss/glut warnings stay goal-less.
        assert!(result
            .advice
            .iter()
            .filter(|a| a.domain == Domain::Economy && a.goal.is_some())
            .all(|a| matches!(a.goal, Some(Goal::Economy(_)))));
        assert!(matches!(
            item.check,
            Some(Check::BuildingLevels { state_id: Some(s), at_least, .. }) if s == state && at_least >= 1.0
        ));
    }
}
