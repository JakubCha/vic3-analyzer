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
    /// Steps in the game's interface; empty = derived by [`steps`].
    #[serde(default)]
    pub how: Vec<String>,
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

    fn levels(&self, building: &str) -> f64 {
        building_levels(self.world, self.defs, self.states, building, None)
    }

    fn ratio(&self, key: &str) -> Option<f64> {
        let (a, b) = key.split_once('/').unwrap_or((key, ""));
        let below = self.expense(b).0;
        (below > 0.0).then(|| self.expense(a).0 / below)
    }

    fn metrics(&self) -> Metrics {
        let c = self.country;
        let debt = c.debt_principal.unwrap_or(0.0);
        let balance = c.weekly_balance;
        // Trends are only kept for the player; without them these conditions do not hold.
        let latest = |s: Option<&vic3_load::TrendSeries>| s.and_then(|s| s.chronological()).and_then(|(_, v)| v.last().copied());
        let trends = c.trends.as_deref();
        let pops = trends.and_then(|t| t.pop_statistics.as_ref());
        let population = latest(pops.and_then(|p| p.trend_population.as_ref())).filter(|p| *p > 0.0);
        let owned: BTreeSet<u32> = c.states.iter().copied().collect();
        Metrics {
            year: self.world.game_date.map(|d| d.year()),
            balance,
            debt,
            income_share: balance.zip(c.weekly_income.filter(|i| *i > 0.0)).map(|(b, i)| b / i),
            debt_share: c.credit_limit.filter(|l| *l > 0.0).map(|l| debt / l),
            runway: balance.filter(|b| *b < 0.0 && debt == 0.0).map(|b| c.treasury / -b),
            literacy: latest(trends.and_then(|t| t.literacy.as_ref())),
            sol: latest(trends.and_then(|t| t.avgsoltrend.as_ref())),
            radicals: latest(pops.and_then(|p| p.trend_radicals.as_ref())).zip(population).map(|(r, p)| r / p),
            infra_short: self
                .world
                .states
                .iter()
                .filter(|s| owned.contains(&s.id) && s.infrastructure_usage.zip(s.infrastructure).is_some_and(|(u, i)| u > i))
                .count(),
        }
    }

    fn holds(&self, w: &When) -> bool {
        let c = self.country;
        let Metrics { year, balance, debt, income_share, debt_share, runway, literacy, sol, radicals, infra_short } = self.metrics();
        let has_law = |l: &String| c.laws.contains(l);
        let has_tech = |t: &String| c.techs.contains(t);
        let levels = |b: &str| self.levels(b);
        let ratio = |key: &String| self.ratio(key);
        let infra_short = || infra_short;
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

    /// The save's values for the rule's measurable conditions, against their thresholds.
    pub fn why_now(&self, w: &When) -> Vec<String> {
        let m = self.metrics();
        let label = |id: &str| self.defs.display_label(id);
        let pct = |x: f64| format!("{:.0}%", x * 100.0);
        let mut out = Vec::new();
        let mut line = |now: Option<String>, rule: String| {
            if let Some(now) = now {
                out.push(format!("{now} (rule: {rule})"));
            }
        };
        let debt = |x: f64| m.debt_share.map(|s| format!("Debt is {} of the credit limit", pct(s))).or(Some(format!("No debt (rule: {})", pct(x))));
        if let Some(x) = w.debt_share_of_limit_at_least {
            line(debt(x), format!("from {}", pct(x)));
        }
        if let Some(x) = w.debt_share_of_limit_below {
            line(debt(x), format!("below {}", pct(x)));
        }
        let balance = m.balance.map(|b| format!("Weekly balance is {}", crate::budget::money(b)));
        if let Some(x) = w.balance_below {
            line(balance.clone(), format!("below {}", crate::budget::money(x)));
        }
        if let Some(x) = w.balance_at_least {
            line(balance.clone(), format!("at least {}", crate::budget::money(x)));
        }
        let share = m.income_share.map(|s| format!("Balance is {} of income", pct(s)));
        if let Some(x) = w.balance_share_of_income_below {
            line(share.clone(), format!("below {}", pct(x)));
        }
        if let Some(x) = w.balance_share_of_income_at_least {
            line(share.clone(), format!("at least {}", pct(x)));
        }
        if let Some(x) = w.runway_weeks_below {
            line(m.runway.map(|r| format!("The treasury lasts {r:.0} weeks")), format!("under {x:.0}"));
        }
        for (k, x) in &w.expense_share_at_least {
            line(Some(format!("{} is {} of spending", pretty_group(k), pct(self.expense(k).1))), format!("from {}", pct(*x)));
        }
        for (k, x) in &w.expense_ratio_at_least {
            line(self.ratio(k).map(|r| format!("{} is {r:.1}× {}", pretty_group(k.split('/').next().unwrap_or(k)), pretty_group(k.split('/').nth(1).unwrap_or("")))), format!("from {x:.1}×"));
        }
        for (k, x) in w.income_share_at_least.iter().chain(&w.income_share_below) {
            line(Some(format!("{} is {} of income", pretty_group(k), pct(self.income_share(k)))), format!("threshold {}", pct(*x)));
        }
        for (b, n) in &w.levels_below {
            line(Some(format!("{}: {:.0} levels", label(b), self.levels(b))), format!("below {n:.0}"));
        }
        for (b, n) in &w.levels_at_least {
            line(Some(format!("{}: {:.0} levels", label(b), self.levels(b))), format!("at least {n:.0}"));
        }
        let price = |g: &str| self.price_ratio(g).map(|r| format!("{} is {:+.0}% against base price", label(g), (r - 1.0) * 100.0));
        for (g, x) in w.price_above_base.iter().chain(&w.any_price_above_base) {
            line(price(g), format!("from +{}", pct(*x)));
        }
        for (g, x) in &w.price_ratio_at_most {
            line(price(g), format!("at most {:+.0}%", (x - 1.0) * 100.0));
        }
        for (id, x) in w.ig_clout_at_least.iter().chain(&w.ig_clout_below) {
            let clout = self.ig(id).map_or(0.0, |ig| ig.clout);
            line(Some(format!("{} clout is {}", label(id), pct(clout))), format!("threshold {}", pct(*x)));
        }
        if let Some(x) = w.literacy_below {
            line(m.literacy.map(|l| format!("Literacy is {}", pct(l))), format!("below {}", pct(x)));
        }
        if let Some(x) = w.sol_below {
            line(m.sol.map(|s| format!("Average standard of living is {s:.1}")), format!("below {x:.0}"));
        }
        if let Some(x) = w.radicals_share_at_least {
            line(m.radicals.map(|r| format!("Radicals are {} of the population", pct(r))), format!("from {}", pct(x)));
        }
        if let Some(n) = w.infrastructure_short_states_at_least {
            line(Some(format!("{} of your states are short of infrastructure", m.infra_short)), format!("at least {n}"));
        }
        if m.debt > 0.0 && out.is_empty() {
            // Nothing measurable in the rule: keep the budget context visible.
            out.extend(m.debt_share.map(|s| format!("Debt is {} of the credit limit", pct(s))));
        }
        out
    }
}

/// A game term explained in Polish (`playbook/glossary.toml`).
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Term {
    pub key: String,
    /// English phrases rule texts use.
    pub en: Vec<String>,
    pub pl_name: String,
    pub pl: String,
}

/// The embedded glossary, parsed once.
pub fn glossary() -> &'static [Term] {
    #[derive(Deserialize)]
    struct File {
        term: Vec<Term>,
    }
    static TERMS: OnceLock<Vec<Term>> = OnceLock::new();
    TERMS.get_or_init(|| toml::from_str::<File>(include_str!("../playbook/glossary.toml")).expect("playbook/glossary.toml").term)
}

/// Glossary terms whose English phrases appear (whole words) in `text`.
pub fn terms_in(text: &str) -> Vec<&'static Term> {
    let lower = text.to_lowercase();
    let word = |p: &str| {
        lower.match_indices(p).any(|(at, _)| {
            !lower[..at].chars().next_back().is_some_and(char::is_alphanumeric) && !lower[at + p.len()..].chars().next().is_some_and(char::is_alphanumeric)
        })
    };
    glossary().iter().filter(|t| t.en.iter().any(|p| word(p))).collect()
}

/// A rule explained for a player: its texts, steps and glossary terms, plus whether
/// it applies to `loaded` (world, defs, prices) and the save's values behind it.
/// `id` may carry the advice prefix (`playbook:gen.tax-bridge`). `None` = no such rule.
pub fn explain(id: &str, loaded: Option<(&World, &GameDefs, &vic3_prices::PricesResult)>) -> Option<serde_json::Value> {
    let id = id.strip_prefix("playbook:").unwrap_or(id);
    let rule = rules().iter().find(|r| r.id == id)?;
    let text = format!("{} {} {}", rule.title, rule.detail, rule.why);
    let mut out = serde_json::json!({
        "id": rule.id,
        "title": rule.title,
        "goal": rule.goal,
        "phase": rule.phase,
        "detail": rule.detail,
        "why": rule.why,
        "sources": rule.sources,
        "patch": rule.patch,
        "when": format!("{:?}", rule.when),
        "steps": steps(rule, &vic3_defs::pretty_id),
        "terms": terms_in(&text),
    });
    if let Some((world, defs, prices)) = loaded {
        if let Some((country, states)) = crate::advice::player(world) {
            let budget = crate::budget::report(defs, country);
            let facts = Facts { world, defs, prices, country, states: &states, budget: budget.as_ref() };
            let for_country = rule.countries.is_empty() || rule.countries.iter().any(|c| *c == country.tag);
            out["applies_now"] = serde_json::json!(for_country && facts.holds(&rule.when));
            out["why_now"] = serde_json::json!(facts.why_now(&rule.when));
            out["steps"] = serde_json::json!(steps(rule, &|id| defs.display_label(id)));
        }
    }
    Some(out)
}

/// Values the rule conditions and their explanations share.
struct Metrics {
    year: Option<i16>,
    balance: Option<f64>,
    debt: f64,
    income_share: Option<f64>,
    debt_share: Option<f64>,
    runway: Option<f64>,
    literacy: Option<f64>,
    sol: Option<f64>,
    radicals: Option<f64>,
    infra_short: usize,
}

fn pretty_group(key: &str) -> String {
    match key {
        "military" => "Military upkeep".into(),
        "government" => "Government upkeep".into(),
        _ => vic3_defs::pretty_id(key),
    }
}

/// How to carry out `rule` in the game's interface: the rule's own `how`, or steps
/// derived from what it asks for (a law, a building, a technology, the budget).
pub fn steps(rule: &Rule, label: &dyn Fn(&str) -> String) -> Vec<String> {
    if !rule.how.is_empty() {
        return rule.how.clone();
    }
    let w = &rule.when;
    let mut out = Vec::new();
    let law = match &rule.done {
        Some(Check::LawActive { law }) => Some(law.clone()),
        _ => None,
    };
    let building = match &rule.done {
        Some(Check::BuildingLevels { building_type, .. }) => Some(building_type.clone()),
        _ => w.levels_below.keys().next().cloned(),
    };
    if let Some(law) = law {
        out.push(format!("Open the Politics panel, Laws tab, and find {}.", label(&law)));
        out.push("Press Enact. Enactment takes months: keep the interest groups that support it in government, and watch the chance of success.".into());
        out.push("Before it passes, check its effect on the budget: schools, health, police and welfare laws cost money every week.".into());
    } else if let Some(Check::LawInactive { law }) = &rule.done {
        out.push(format!("Open the Politics panel, Laws tab, and find the law group of {}.", label(law)));
        out.push("Enact the replacement the detail names; the old law ends when it passes.".into());
    }
    if let Some(b) = building {
        out.push(format!("Open the Construction panel and choose {}.", label(&b)));
        out.push("Pick the state the analyzer's Now list or Market tab names (resources, workers, payback) and queue a level.".into());
    } else if let Some(good) = w.price_above_base.keys().chain(w.any_price_above_base.keys()).next() {
        out.push(format!("Open the Market panel and find {}: its tooltip shows which buildings make it.", label(good)));
        out.push("The analyzer's Market tab names the building, the state to build in and any input that is dear too.".into());
    }
    if let Some(tech) = w.techs_missing.first() {
        out.push(format!("Open the Technology panel and put {} first in its queue.", label(tech)));
    }
    let budget = w.balance_below.is_some()
        || w.debt_share_of_limit_at_least.is_some()
        || w.runway_weeks_below.is_some()
        || !w.expense_share_at_least.is_empty()
        || !w.expense_ratio_at_least.is_empty();
    if out.is_empty() && budget {
        out.push("Open the Budget panel: the tax level and the military and government wage levels are set there and take effect weekly.".into());
        out.push("Army costs: in the Military panel disband battalions you do not need in peacetime, or switch them to cheaper equipment.".into());
    }
    if out.is_empty() {
        out.push("Follow the detail above; ask Muse (tool explain_rule) if a step is unclear.".into());
    }
    out
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
            steps: steps(rule, &|id| facts.defs.display_label(id)),
            why_now: facts.why_now(&rule.when),
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
        assert!(out.iter().all(|a| !a.steps.is_empty()), "every playbook item says how to do it");
        let share = out.iter().find(|a| a.id == "playbook:gen.army-share").expect("army share");
        assert!(share.why_now.iter().any(|l| l.starts_with("Military upkeep is") && l.contains("(rule: from")), "{:?}", share.why_now);
    }

    #[test]
    fn explain_names_steps_and_polish_terms() {
        let e = explain("playbook:gen.tax-bridge", None).expect("rule");
        assert_eq!(e["id"], "gen.tax-bridge");
        assert!(e["steps"].as_array().is_some_and(|s| !s.is_empty()));
        let terms: Vec<&str> = e["terms"].as_array().unwrap().iter().filter_map(|t| t["key"].as_str()).collect();
        assert!(terms.contains(&"credit_limit"), "{terms:?}");
        assert!(explain("nope", None).is_none());
        assert!(glossary().iter().all(|t| !t.en.is_empty() && !t.pl.is_empty()));
    }

    #[test]
    fn steps_follow_what_the_rule_asks_for() {
        let by_id = |id: &str| rules().iter().find(|r| r.id == id).expect(id);
        let law = steps(by_id("gen.welfare-when-solvent"), &vic3_defs::pretty_id);
        assert!(law[0].contains("Politics panel") && law[0].contains("Poor Laws"), "{law:?}");
        let building = steps(by_id("gen.mid-universities"), &vic3_defs::pretty_id);
        assert!(building[0].contains("Construction panel") && building[0].contains("University"), "{building:?}");
        let tech = steps(by_id("gen.early-railways-research"), &vic3_defs::pretty_id);
        assert!(tech.iter().any(|s| s.contains("Technology panel")), "{tech:?}");
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
