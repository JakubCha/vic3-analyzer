//! Playbook: hand-written rules distilled from strategy guides (in our own
//! words), each fired by conditions on the loaded save. Rules live as data in
//! `playbook/*.toml`; adding a country is adding a file to [`FILES`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use vic3_defs::GameDefs;
use vic3_prices::{World, WorldCountry};
use vic3save::PdsDate;

use crate::advice::{building_levels, Advice, Check, Domain};
use crate::budget::BudgetReport;

/// Embedded rule files: general first, then per country.
const FILES: &[(&str, &str)] = &[
    ("general", include_str!("../playbook/general.toml")),
    ("pru", include_str!("../playbook/pru.toml")),
    ("gbr", include_str!("../playbook/gbr.toml")),
    ("usa", include_str!("../playbook/usa.toml")),
    ("ita", include_str!("../playbook/ita.toml")),
    ("fra", include_str!("../playbook/fra.toml")),
    ("aus", include_str!("../playbook/aus.toml")),
];

#[derive(Debug, Deserialize)]
struct RuleFile {
    #[serde(default, rename = "rule")]
    rules: Vec<Rule>,
}

/// One playbook rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    /// Country tags the rule is for; empty = every country.
    #[serde(default)]
    pub countries: Vec<String>,
    pub domain: Domain,
    #[serde(default = "worthwhile")]
    pub priority: u8,
    /// Playbook goal: budget, industry, military, reforms, unification, …
    pub goal: String,
    /// opening, early, mid, late.
    pub phase: String,
    pub title: String,
    pub detail: String,
    pub why: String,
    #[serde(default)]
    pub sources: Vec<String>,
    /// Patch the sources target, e.g. "1.13".
    pub patch: String,
    #[serde(default)]
    pub when: When,
    /// Confirms the rule was followed on a later save.
    #[serde(default)]
    pub done: Option<Check>,
}

fn worthwhile() -> u8 {
    2
}

/// Conditions; every one that is set must hold.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct When {
    pub from_year: Option<i16>,
    pub until_year: Option<i16>,
    /// All active.
    #[serde(default)]
    pub laws: Vec<String>,
    /// None active.
    #[serde(default)]
    pub laws_missing: Vec<String>,
    /// All researched.
    #[serde(default)]
    pub techs: Vec<String>,
    /// None researched.
    #[serde(default)]
    pub techs_missing: Vec<String>,
    /// National levels of a building type below the value.
    #[serde(default)]
    pub levels_below: BTreeMap<String, f64>,
    #[serde(default)]
    pub levels_at_least: BTreeMap<String, f64>,
    pub balance_below: Option<f64>,
    pub balance_at_least: Option<f64>,
    /// Weekly balance as a share of weekly income (−0.1 = losing 10% of income).
    pub balance_share_of_income_below: Option<f64>,
    pub balance_share_of_income_at_least: Option<f64>,
    pub in_debt: Option<bool>,
    /// Debt over credit limit.
    pub debt_share_of_limit_at_least: Option<f64>,
    pub debt_share_of_limit_below: Option<f64>,
    /// No debt yet, losing money, and the treasury lasts fewer weeks than this.
    pub runway_weeks_below: Option<f64>,
    /// Share of weekly spending, by budget group key or the families
    /// `military` (goods + wages) and `government` (goods + wages).
    #[serde(default)]
    pub expense_share_at_least: BTreeMap<String, f64>,
    /// `"a/b"` → expense of group (or family) a over b at least the value.
    #[serde(default)]
    pub expense_ratio_at_least: BTreeMap<String, f64>,
    /// Share of weekly income by budget group key.
    #[serde(default)]
    pub income_share_at_least: BTreeMap<String, f64>,
    #[serde(default)]
    pub income_share_below: BTreeMap<String, f64>,
    #[serde(default)]
    pub igs_in_government: Vec<String>,
    #[serde(default)]
    pub igs_not_in_government: Vec<String>,
    /// Interest-group clout (0–1) at least the value.
    #[serde(default)]
    pub ig_clout_at_least: BTreeMap<String, f64>,
    /// Interest-group clout below the value (absent groups count as 0).
    #[serde(default)]
    pub ig_clout_below: BTreeMap<String, f64>,
    /// Market price over base minus 1 (0.2 = 20% above base) at least the value.
    #[serde(default)]
    pub price_above_base: BTreeMap<String, f64>,
    /// Like `price_above_base`, but one good is enough.
    #[serde(default)]
    pub any_price_above_base: BTreeMap<String, f64>,
    /// Market price over base at most the value (1.0 = at base).
    #[serde(default)]
    pub price_ratio_at_most: BTreeMap<String, f64>,
    /// Latest national literacy (0–1) below the value.
    pub literacy_below: Option<f64>,
    /// Latest average standard of living below the value.
    pub sol_below: Option<f64>,
    /// Latest radicals over population (0–1) at least the value.
    pub radicals_share_at_least: Option<f64>,
    /// Owned states using more infrastructure than they have, at least this many.
    pub infrastructure_short_states_at_least: Option<usize>,
}

/// All embedded rules, parsed once.
pub fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        FILES
            .iter()
            .flat_map(|(name, text)| {
                toml::from_str::<RuleFile>(text)
                    .unwrap_or_else(|e| panic!("playbook/{name}.toml: {e}"))
                    .rules
            })
            .collect()
    })
}

/// What a rule's conditions are evaluated against.
pub struct Facts<'a> {
    pub world: &'a World,
    pub defs: &'a GameDefs,
    pub prices: &'a vic3_prices::PricesResult,
    pub country: &'a WorldCountry,
    pub states: &'a BTreeSet<u32>,
    pub budget: Option<&'a BudgetReport>,
}

impl Facts<'_> {
    /// (amount, share) of an expense group or family; zero without a budget.
    fn expense(&self, key: &str) -> (f64, f64) {
        let Some(b) = self.budget else { return (0.0, 0.0) };
        let single = [key];
        let groups: &[&str] = match key {
            "military" => &["military_goods", "military_wages"],
            "government" => &["government_goods", "government_wages"],
            _ => &single,
        };
        b.expenses
            .iter()
            .filter(|g| groups.contains(&g.group.as_str()))
            .fold((0.0, 0.0), |(a, s), g| (a + g.amount, s + g.share))
    }

    fn income_share(&self, key: &str) -> f64 {
        self.budget.map_or(0.0, |b| b.incomes.iter().filter(|g| g.group == key).map(|g| g.share).sum())
    }

    /// Price over base, when the good is on the market.
    fn price_ratio(&self, good: &str) -> Option<f64> {
        self.prices.goods.iter().find(|g| g.name == good).filter(|g| g.base > 0.0).map(|g| g.price / g.base)
    }

    fn ig(&self, id: &str) -> Option<&vic3_prices::WorldInterestGroup> {
        self.country.interest_groups.iter().find(|ig| ig.definition == id)
    }

    fn holds(&self, w: &When) -> bool {
        let c = self.country;
        let year = self.world.game_date.map(|d| d.year());
        let has_law = |l: &String| c.laws.contains(l);
        let has_tech = |t: &String| c.techs.contains(t);
        let levels = |b: &str| building_levels(self.world, self.defs, self.states, b, None);
        let balance = c.weekly_balance;
        let debt = c.debt_principal.unwrap_or(0.0);
        let income_share = balance.zip(c.weekly_income.filter(|i| *i > 0.0)).map(|(b, i)| b / i);
        let debt_share = c.credit_limit.filter(|l| *l > 0.0).map(|l| debt / l);
        let runway = balance.filter(|b| *b < 0.0 && debt == 0.0).map(|b| c.treasury / -b);
        let ratio = |key: &String| {
            let (a, b) = key.split_once('/').unwrap_or((key.as_str(), ""));
            let below = self.expense(b).0;
            (below > 0.0).then(|| self.expense(a).0 / below)
        };
        // Trends are only kept for the player; without them these conditions do not hold.
        let latest = |s: Option<&vic3_load::TrendSeries>| s.and_then(|s| s.chronological()).and_then(|(_, v)| v.last().copied());
        let trends = c.trends.as_deref();
        let pops = trends.and_then(|t| t.pop_statistics.as_ref());
        let literacy = latest(trends.and_then(|t| t.literacy.as_ref()));
        let sol = latest(trends.and_then(|t| t.avgsoltrend.as_ref()));
        let population = latest(pops.and_then(|p| p.trend_population.as_ref())).filter(|p| *p > 0.0);
        let radicals = latest(pops.and_then(|p| p.trend_radicals.as_ref())).zip(population).map(|(r, p)| r / p);
        let infra_short = || {
            let owned: BTreeSet<u32> = c.states.iter().copied().collect();
            self.world
                .states
                .iter()
                .filter(|s| owned.contains(&s.id) && s.infrastructure_usage.zip(s.infrastructure).is_some_and(|(u, i)| u > i))
                .count()
        };
        w.from_year.is_none_or(|y| year.is_some_and(|now| now >= y))
            && w.until_year.is_none_or(|y| year.is_some_and(|now| now <= y))
            && w.laws.iter().all(has_law)
            && !w.laws_missing.iter().any(has_law)
            && w.techs.iter().all(has_tech)
            && !w.techs_missing.iter().any(has_tech)
            && w.levels_below.iter().all(|(b, n)| levels(b) < *n)
            && w.levels_at_least.iter().all(|(b, n)| levels(b) >= *n)
            && w.balance_below.is_none_or(|x| balance.is_some_and(|b| b < x))
            && w.balance_at_least.is_none_or(|x| balance.is_some_and(|b| b >= x))
            && w.balance_share_of_income_below.is_none_or(|x| income_share.is_some_and(|s| s < x))
            && w.balance_share_of_income_at_least.is_none_or(|x| income_share.is_some_and(|s| s >= x))
            && w.in_debt.is_none_or(|d| (debt > 0.0) == d)
            && w.debt_share_of_limit_at_least.is_none_or(|x| debt_share.is_some_and(|s| s >= x))
            && w.debt_share_of_limit_below.is_none_or(|x| debt_share.is_none_or(|s| s < x))
            && w.runway_weeks_below.is_none_or(|x| runway.is_some_and(|r| r < x))
            && w.expense_share_at_least.iter().all(|(k, x)| self.expense(k).1 >= *x)
            && w.expense_ratio_at_least.iter().all(|(k, x)| ratio(k).is_some_and(|r| r >= *x))
            && w.income_share_at_least.iter().all(|(k, x)| self.income_share(k) >= *x)
            && w.income_share_below.iter().all(|(k, x)| self.income_share(k) < *x)
            && w.igs_in_government.iter().all(|id| self.ig(id).is_some_and(|ig| ig.in_government))
            && w.igs_not_in_government.iter().all(|id| self.ig(id).is_some_and(|ig| !ig.in_government))
            && w.ig_clout_at_least.iter().all(|(id, x)| self.ig(id).is_some_and(|ig| ig.clout >= *x))
            && w.ig_clout_below.iter().all(|(id, x)| self.ig(id).is_none_or(|ig| ig.clout < *x))
            && w.price_above_base.iter().all(|(good, x)| self.price_ratio(good).is_some_and(|r| r - 1.0 >= *x))
            && (w.any_price_above_base.is_empty()
                || w.any_price_above_base.iter().any(|(good, x)| self.price_ratio(good).is_some_and(|r| r - 1.0 >= *x)))
            && w.price_ratio_at_most.iter().all(|(good, x)| self.price_ratio(good).is_some_and(|r| r <= *x))
            && w.literacy_below.is_none_or(|x| literacy.is_some_and(|l| l < x))
            && w.sol_below.is_none_or(|x| sol.is_some_and(|s| s < x))
            && w.radicals_share_at_least.is_none_or(|x| radicals.is_some_and(|r| r >= x))
            && w.infrastructure_short_states_at_least.is_none_or(|n| infra_short() >= n)
    }
}

/// Matching rules for the played country, as advice.
pub fn playbook_advice(facts: &Facts<'_>, out: &mut Vec<Advice>) {
    let tag = facts.country.tag.as_str();
    for rule in rules() {
        if !(rule.countries.is_empty() || rule.countries.iter().any(|c| c == tag)) || !facts.holds(&rule.when) {
            continue;
        }
        // Rules already followed stay quiet.
        if rule.done.as_ref().is_some_and(|check| crate::advice::checks_done(facts.world, facts.defs, std::slice::from_ref(check)) == [true]) {
            continue;
        }
        let sources = if rule.sources.is_empty() {
            String::new()
        } else {
            format!(" Sources ({}): {}", rule.patch, rule.sources.join(", "))
        };
        out.push(Advice {
            id: format!("playbook:{}", rule.id),
            domain: rule.domain,
            priority: rule.priority,
            title: rule.title.clone(),
            detail: format!("Playbook · {} · {}: {}", rule.goal, rule.phase, rule.detail),
            why: Some(format!("{}{sources}", rule.why)),
            goal: None,
            check: rule.done.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_rules_parse_with_unique_ids() {
        let all = rules();
        assert!(!all.is_empty());
        let ids: BTreeSet<&str> = all.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids.len(), all.len(), "duplicate rule id");
        assert!(all.iter().all(|r| !r.why.is_empty() && matches!(r.priority, 1 | 2)));
    }

    #[test]
    fn prussia_1836_fires_its_rules_and_no_other_country() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let save = std::fs::read(dir.join("../vic3-load/tests/fixtures/plaintext.txt")).expect("save");
        let defs = vic3_defs::load_from_path(dir.join("../vic3-defs/tests/fixtures")).expect("defs");
        let blob = vic3_defs::encode_blob(&defs).expect("blob");
        let mut snap = crate::load_analysis_snapshot(&save, None, &blob, "{}", false).expect("snapshot");
        let line = |group: &str, amount| vic3_prices::BudgetLine {
            income: false,
            group: group.into(),
            building_type: "building_barrack".into(),
            amount,
        };
        // The fixture save is dated 1836; give its player Prussia's tag and budget
        // (done-checks read the world's player).
        let player = snap.world.countries.iter_mut().find(|c| Some(&c.tag) == snap.world.player_tag.as_ref()).expect("player");
        *player = WorldCountry {
            tag: "PRU".into(),
            treasury: 486_803.0,
            weekly_balance: Some(-7_231.0),
            weekly_income: Some(57_854.0),
            weekly_expenses: Some(65_085.0),
            budget_lines: vec![line("military_goods", 24_769.0), line("military_wages", 15_420.0)],
            ..player.clone()
        };
        snap.world.player_tag = Some("PRU".into());
        let country = snap.world.country_by_tag("PRU").expect("PRU");
        let budget = crate::budget::report(&snap.defs, country);
        let facts = Facts {
            world: &snap.world,
            defs: &snap.defs,
            prices: &snap.prices,
            country,
            states: &BTreeSet::new(),
            budget: budget.as_ref(),
        };
        let mut out = Vec::new();
        playbook_advice(&facts, &mut out);
        let ids: BTreeSet<&str> = out.iter().map(|a| a.id.as_str()).collect();
        for id in ["playbook:pru.opening-army", "playbook:pru.opening-plan", "playbook:gen.army-share", "playbook:gen.goods-hungry-units"] {
            assert!(ids.contains(id), "{id} missing from {ids:?}");
        }
        assert!(ids.iter().all(|id| id.starts_with("playbook:pru.") || id.starts_with("playbook:gen.")), "{ids:?}");
        let army = out.iter().find(|a| a.id == "playbook:pru.opening-army").expect("army");
        assert_eq!(army.priority, 1);
        assert!(army.why.as_deref().is_some_and(|w| w.contains("Sources (1.13.9)")));
    }

    fn trend(values: &[f64]) -> vic3_load::TrendSeries {
        let values: Vec<String> = values.iter().map(f64::to_string).collect();
        let text = format!("sample_rate=28 count=5214 channels={{ 0={{ date=1860.1.1 index={} values={{ {} }} }} }}", values.len(), values.join(" "));
        jomini::text::de::from_utf8_slice(text.as_bytes()).expect("trend")
    }

    #[test]
    fn trend_and_infrastructure_conditions_read_the_latest_values() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let save = std::fs::read(dir.join("../vic3-load/tests/fixtures/plaintext.txt")).expect("save");
        let defs = vic3_defs::load_from_path(dir.join("../vic3-defs/tests/fixtures")).expect("defs");
        let blob = vic3_defs::encode_blob(&defs).expect("blob");
        let mut snap = crate::load_analysis_snapshot(&save, None, &blob, "{}", false).expect("snapshot");
        let tag = snap.world.player_tag.clone().expect("player");
        let player = snap.world.countries.iter_mut().find(|c| c.tag == tag).expect("player");
        let no_trends = player.clone();
        player.trends = Some(Box::new(vic3_load::CountryTrends {
            literacy: Some(trend(&[0.1, 0.4])),
            avgsoltrend: Some(trend(&[12.0, 8.0])),
            pop_statistics: Some(vic3_load::CountryPopStatistics {
                trend_population: Some(trend(&[1000.0, 1000.0])),
                trend_radicals: Some(trend(&[10.0, 200.0])),
                ..Default::default()
            }),
            ..Default::default()
        }));
        let state = *player.states.first().expect("owned state");
        let s = snap.world.states.iter_mut().find(|s| s.id == state).expect("state");
        s.infrastructure = Some(10.0);
        s.infrastructure_usage = Some(12.0);
        let country = snap.world.country_by_tag(&tag).expect("country");
        let none = BTreeSet::new();
        let facts = |country| Facts { world: &snap.world, defs: &snap.defs, prices: &snap.prices, country, states: &none, budget: None };
        let when = |f: fn(&mut When)| {
            let mut w = When::default();
            f(&mut w);
            w
        };
        let f = facts(country);
        assert!(f.holds(&when(|w| w.literacy_below = Some(0.5))));
        assert!(!f.holds(&when(|w| w.literacy_below = Some(0.3))), "latest literacy is 0.4, not the oldest 0.1");
        assert!(f.holds(&when(|w| w.sol_below = Some(10.0))));
        assert!(f.holds(&when(|w| w.radicals_share_at_least = Some(0.15))));
        assert!(!f.holds(&when(|w| w.radicals_share_at_least = Some(0.25))));
        assert!(f.holds(&when(|w| w.infrastructure_short_states_at_least = Some(1))));
        assert!(!f.holds(&when(|w| w.infrastructure_short_states_at_least = Some(2))));
        // Without trends (other countries) the trend conditions never fire.
        assert!(!facts(&no_trends).holds(&when(|w| w.literacy_below = Some(1.0))));
    }
}
