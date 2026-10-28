//! Budget guard: where the money goes, how long the treasury lasts, and the
//! levers that close a deficit. Read straight from the save's budget block
//! (`balance_trend`, `country_building_budget`), no market solve.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vic3_defs::GameDefs;
use vic3_prices::WorldCountry;

use crate::advice::{Advice, Check, Domain};

/// How close the country is to default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Balance is positive or unknown.
    Safe,
    /// Losing money, with more than a year before debt reaches half the credit limit.
    Deficit,
    /// Debt at or near half the credit limit (the game's unhealthy line), or default within two years.
    Danger,
    /// Over the credit limit, or default within half a year.
    Critical,
}

/// One budget category, e.g. military wages, with its largest buildings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetGroup {
    pub group: String,
    pub label: String,
    pub amount: f64,
    /// Share of total weekly income or expenses.
    pub share: f64,
    /// Largest buildings in the group: (label, weekly amount).
    pub top: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetReport {
    pub risk: Risk,
    pub treasury: f64,
    pub weekly_balance: Option<f64>,
    pub weekly_income: Option<f64>,
    pub weekly_expenses: Option<f64>,
    pub debt: f64,
    pub credit_limit: Option<f64>,
    /// Weeks until the treasury is empty and borrowing starts (deficit only).
    pub weeks_to_debt: Option<f64>,
    /// Weeks until debt passes the credit limit (deficit only).
    pub weeks_to_default: Option<f64>,
    /// Weeks until debt reaches half the credit limit, where the game counts the
    /// economy as unhealthy (deficit only). Borrowing before that is cheap.
    pub weeks_to_unhealthy: Option<f64>,
    /// What the deficit pays for, when losing money.
    pub deficit_kind: Option<DeficitKind>,
    pub expenses: Vec<BudgetGroup>,
    pub incomes: Vec<BudgetGroup>,
    /// What a war with the whole army mobilised would do to this budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub war: Option<WarEstimate>,
}

/// Budget at war, a lower bound: a mobilised battalion consumes twice its peacetime
/// goods (static modifier `mobilized_battalion_consumption`, `building_goods_input_mult = 1`,
/// 1.13.9). Not included: mobilisation options, raised conscripts, lost trade and taxes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WarEstimate {
    /// Added weekly cost with every battalion mobilised.
    pub extra_weekly: f64,
    pub weekly_balance: f64,
    /// Weeks of war until debt reaches half the credit limit / the credit limit.
    pub weeks_to_unhealthy: Option<f64>,
    pub weeks_to_default: Option<f64>,
}

/// What a deficit pays for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeficitKind {
    /// Construction is at least 30% of spending and debt is under half the limit: it pays back.
    Investment,
    /// Running costs (army, administration) are the gap.
    Upkeep,
}

/// Share of spending above which a deficit counts as investment.
pub const INVESTMENT_SHARE: f64 = 0.3;

impl BudgetReport {
    /// Weekly construction spending (construction goods).
    pub fn construction_spend(&self) -> f64 {
        self.expense(&["construction_goods"])
    }

    fn expense(&self, groups: &[&str]) -> f64 {
        self.expenses.iter().filter(|g| groups.contains(&g.group.as_str())).map(|g| g.amount).sum()
    }
}

/// Short money figure: 7.2K, 17.1M.
pub fn money(x: f64) -> String {
    let a = x.abs();
    let sign = if x < 0.0 { "−" } else { "" };
    if a >= 1e6 {
        format!("{sign}{:.1}M", a / 1e6)
    } else if a >= 1e3 {
        format!("{sign}{:.1}K", a / 1e3)
    } else {
        format!("{sign}{a:.0}")
    }
}

fn weeks(w: f64) -> String {
    if w >= 104.0 {
        format!("{w:.0} weeks (~{:.1} years)", w / 52.0)
    } else {
        format!("{w:.0} weeks")
    }
}

fn group_label(key: &str) -> String {
    match key {
        "government_goods" => "Government goods".into(),
        "government_wages" => "Government wages".into(),
        "military_goods" => "Military goods".into(),
        "military_wages" => "Military wages".into(),
        "construction_goods" => "Construction".into(),
        "subsidies" => "Subsidies".into(),
        "government_share_losses" => "State-owned building losses".into(),
        "income_taxes" => "Income tax".into(),
        "poll_taxes" => "Poll tax".into(),
        "dividends_taxes" => "Dividends tax".into(),
        "government_share_dividends" => "State-owned dividends".into(),
        "tariffs" => "Tariffs".into(),
        other => vic3_defs::pretty_id(other),
    }
}

fn groups(defs: &GameDefs, country: &WorldCountry, income: bool, total: Option<f64>) -> Vec<BudgetGroup> {
    let mut by_group: BTreeMap<&str, Vec<(&str, f64)>> = BTreeMap::new();
    for line in country.budget_lines.iter().filter(|l| l.income == income) {
        by_group.entry(&line.group).or_default().push((&line.building_type, line.amount));
    }
    let listed: f64 = by_group.values().flatten().map(|(_, a)| a).sum();
    let total = total.unwrap_or(listed).max(listed);
    let share = |a: f64| if total > 0.0 { a / total } else { 0.0 };
    let mut out: Vec<BudgetGroup> = by_group
        .into_iter()
        .map(|(group, mut items)| {
            items.sort_by(|a, b| b.1.total_cmp(&a.1));
            let amount = items.iter().map(|(_, a)| a).sum();
            BudgetGroup {
                group: group.into(),
                label: group_label(group),
                amount,
                share: share(amount),
                top: items.into_iter().take(3).map(|(b, a)| (defs.display_label(b), a)).collect(),
            }
        })
        .collect();
    // Interest, diplomacy, consumption taxes, minting… are not per building.
    let other = total - listed;
    if other > total * 0.01 {
        out.push(BudgetGroup {
            group: "other".into(),
            label: "Other (not per building: interest, diplomacy, consumption taxes, minting…)".into(),
            amount: other,
            share: share(other),
            top: Vec::new(),
        });
    }
    out.sort_by(|a, b| b.amount.total_cmp(&a.amount));
    out
}

/// Budget report for `country`; `None` when the save carries no budget.
pub fn report(defs: &GameDefs, country: &WorldCountry) -> Option<BudgetReport> {
    let balance = country.weekly_balance;
    if balance.is_none() && country.budget_lines.is_empty() {
        return None;
    }
    // The save omits `principal` when there is no debt.
    let debt = country.debt_principal.unwrap_or(0.0);
    let limit = country.credit_limit;
    let over_limit = country.credit_headroom.is_some_and(|h| h <= 0.0) || limit.is_some_and(|l| debt >= l);
    let treasury = country.treasury;
    // Weeks until debt reaches `fraction` of the credit limit at today's deficit.
    let weeks_to = |fraction: f64| match (balance, limit) {
        (Some(b), Some(l)) if b < 0.0 => Some(((treasury + l * fraction - debt) / -b).max(0.0)),
        _ => None,
    };
    let losing = balance.is_some_and(|b| b < 0.0);
    let weeks_to_debt = balance.filter(|b| *b < 0.0).map(|b| (treasury / -b).max(0.0));
    let (weeks_to_default, weeks_to_unhealthy) = (weeks_to(1.0), weeks_to(0.5));
    let debt_share = limit.filter(|l| *l > 0.0).map_or(0.0, |l| debt / l);
    // An empty treasury only means borrowing starts; urgency comes from the credit limit.
    let risk = if over_limit || weeks_to_default.is_some_and(|w| w < 26.0) {
        Risk::Critical
    } else if debt_share >= 0.5 || weeks_to_default.is_some_and(|w| w < 104.0) || weeks_to_unhealthy.is_some_and(|w| w < 52.0) {
        Risk::Danger
    } else if losing {
        Risk::Deficit
    } else {
        Risk::Safe
    };
    let expenses = groups(defs, country, false, country.weekly_expenses);
    let construction: f64 = expenses.iter().filter(|g| g.group == "construction_goods").map(|g| g.share).sum();
    let deficit_kind = losing.then_some(if construction >= INVESTMENT_SHARE && debt_share < 0.5 {
        DeficitKind::Investment
    } else {
        DeficitKind::Upkeep
    });
    let war = balance.map(|b| {
        let extra: f64 = expenses.iter().filter(|g| g.group == "military_goods").map(|g| g.amount).sum();
        let at_war = b - extra;
        let weeks = |fraction: f64| limit.filter(|_| at_war < 0.0).map(|l| ((treasury + l * fraction - debt) / -at_war).max(0.0));
        WarEstimate { extra_weekly: extra, weekly_balance: at_war, weeks_to_unhealthy: weeks(0.5), weeks_to_default: weeks(1.0) }
    });
    Some(BudgetReport {
        risk,
        treasury,
        weekly_balance: balance,
        weekly_income: country.weekly_income,
        weekly_expenses: country.weekly_expenses,
        debt,
        credit_limit: limit,
        weeks_to_debt,
        weeks_to_default,
        weeks_to_unhealthy,
        deficit_kind,
        expenses,
        incomes: groups(defs, country, true, country.weekly_income),
        war,
    })
}

fn describe(groups: &[BudgetGroup], n: usize) -> String {
    groups
        .iter()
        .take(n)
        .map(|g| {
            let top = g.top.first().map_or(String::new(), |(b, a)| format!(", mostly {b} {}", money(*a)));
            format!("{} {} ({:.0}%{top})", g.label, money(g.amount), g.share * 100.0)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// How to remove a peacetime battalion, in the game's own button names (1.13.9 localization).
const ARMY_DOWNSIZE: &str = "Army: open an army, its Buildings tab, a Barracks, and press Downsize once per battalion you do not need in peacetime.";

/// Budget-guard advice: runway, then the biggest levers, cost side first.
pub fn budget_advice(r: &BudgetReport, out: &mut Vec<Advice>) {
    let urgent = r.risk >= Risk::Danger;
    let spend = r.weekly_expenses.unwrap_or_else(|| r.expenses.iter().map(|g| g.amount).sum());
    let share = |groups: &[&str]| if spend > 0.0 { r.expense(groups) / spend } else { 0.0 };

    if r.risk == Risk::Safe {
        // Hoarding: a growing treasury with idle construction is wasted growth.
        let builds = r.expense(&["construction_goods"]);
        if r.weekly_balance.is_some_and(|b| b > 0.0) && builds == 0.0 && r.treasury > spend * 20.0 {
            out.push(Advice {
                id: "budget:spend".into(),
                domain: Domain::Economy,
                priority: 2,
                title: format!("Put the {} treasury to work", money(r.treasury)),
                detail: format!(
                    "You gain {} a week and nothing is being built. Queue construction: the budget pays for it.",
                    money(r.weekly_balance.unwrap_or_default())
                ),
                steps: vec![
                    "Construction panel: queue the expansions the analyzer's Now list ranks first (shortest payback).".into(),
                    "Keep queuing until the weekly balance in the Budget panel is close to zero.".into(),
                ],
                why_now: Vec::new(),
                why: Some("Money in the treasury earns nothing. Construction is paid from the state budget (construction goods), and every finished building pays wages and taxes back. A small deficit while building is normal; a surplus with an empty queue is growth you are not getting.".into()),
                goal: None,
                check: None,
            });
        }
        return;
    }

    let balance = r.weekly_balance.unwrap_or_default();
    let mut detail = match (r.weekly_income, r.weekly_expenses) {
        (Some(i), Some(e)) => format!("You earn {} and spend {} a week ({}). ", money(i), money(e), money(balance)),
        _ => format!("Weekly balance {}. ", money(balance)),
    };
    if r.debt > 0.0 {
        detail += &format!("Debt {} of a {} credit limit. ", money(r.debt), r.credit_limit.map_or("unknown".into(), money));
    } else if let Some(w) = r.weeks_to_debt {
        detail += &format!("The {} treasury lasts about {}, then the state borrows. ", money(r.treasury), weeks(w));
    }
    if let Some(w) = r.weeks_to_unhealthy.filter(|_| r.risk < Risk::Critical) {
        detail += &format!("Debt reaches half the credit limit in about {}. ", weeks(w));
    } else if let Some(w) = r.weeks_to_default {
        detail += &format!("At this rate the credit limit is reached in about {}. ", weeks(w));
    }
    let build_share = share(&["construction_goods"]);
    let investment = r.deficit_kind == Some(DeficitKind::Investment) && !urgent;
    let upkeep = 1.0 - build_share;
    if investment {
        detail += &format!(
            "{:.0}% of spending is construction, which pays back: keep building and fix the {} of upkeep instead ({}).",
            build_share * 100.0,
            money(spend * upkeep),
            describe(&r.expenses.iter().filter(|g| g.group != "construction_goods").cloned().collect::<Vec<_>>(), 2)
        );
    } else {
        detail += &format!("Biggest costs: {}.", describe(&r.expenses, 3));
    }
    let over = r.risk == Risk::Critical && r.weeks_to_default.is_none_or(|w| w <= 0.0);
    out.push(Advice {
        id: "budget:balance".into(),
        domain: Domain::Economy,
        priority: if urgent { 1 } else { 2 },
        title: match (r.risk, r.weeks_to_default) {
            (Risk::Critical, _) if over => "Over the credit limit: cut spending now".into(),
            (Risk::Critical, Some(w)) => format!("Default in about {w:.0} weeks"),
            _ if investment => format!("Investment deficit: {} a week, keep building", money(balance)),
            _ => format!("Budget deficit: {} a week", money(balance)),
        },
        detail,
        steps: if investment {
            vec![
                "Keep the construction queue full (Construction panel): the deficit is paying for growth.".into(),
                "Cut the upkeep instead: follow the military and administration items in this list.".into(),
            ]
        } else {
            let mut s = vec![
                "Budget panel: set Military Wages and Government Wages one step lower.".into(),
                ARMY_DOWNSIZE.into(),
            ];
            if urgent {
                s.push("Budget panel: raise the tax level one step.".into());
                s.push("Construction panel: press Cancel Construction on the most expensive item in the queue.".into());
            }
            s
        },
        why_now: Vec::new(),
        why: Some(if investment {
            "Borrowing to build is how a Victoria 3 economy grows: new buildings pay wages, taxes and dividends and make their goods cheaper, so the debt pays itself back. An empty treasury only means the state starts borrowing; the danger line is debt at half the credit limit, where the game counts the economy as unhealthy and interest starts to bite. Until then, cut costs that earn nothing (an oversized peacetime army, overpriced government goods) rather than construction or the tax level.".into()
        } else {
            "A deficit is not a problem by itself: once the treasury is empty the state borrows, and borrowing to build pays back through the taxes and dividends of the new buildings. It becomes one when it pays for upkeep (army, administration) instead of growth: debt only grows, interest is added to spending, and when debt passes the credit limit the country defaults, with severe penalties. Fix the deficit well before debt reaches half the credit limit.".into()
        }),
        goal: None,
        check: Some(if over { Check::Solvent } else { Check::WeeklyBalance { at_least: 0.0 } }),
    });

    let lines = |groups: &[&str]| {
        let parts: Vec<String> = r
            .expenses
            .iter()
            .filter(|g| groups.contains(&g.group.as_str()))
            .map(|g| {
                let top: Vec<String> = g.top.iter().map(|(b, a)| format!("{b} {}", money(*a))).collect();
                format!("{} {} ({})", g.label, money(g.amount), top.join(", "))
            })
            .collect();
        parts.join("; ")
    };

    let military = share(&["military_goods", "military_wages"]);
    if military >= 0.2 {
        out.push(Advice {
            id: "budget:military".into(),
            domain: Domain::War,
            // The biggest cost that earns nothing: first lever in any sizeable deficit.
            priority: if urgent || military >= 0.3 { 1 } else { 2 },
            title: format!("Military upkeep is {:.0}% of spending", military * 100.0),
            detail: format!("{}.", lines(&["military_goods", "military_wages"])),
            steps: vec![
                format!("{ARMY_DOWNSIZE} Cavalry and artillery battalions use the most goods."),
                "Budget panel: set Military Wages one step lower.".into(),
                "Keep the conscription centers: they cost almost nothing until you raise conscripts for a war.".into(),
            ],
            why_now: Vec::new(),
            why: Some("Every battalion pays wages to its soldiers and officers (barracks) and consumes arms, ammunition and other goods from the market (the army logistics line). In peacetime you can lower military wages in the Budget panel (soldiers' standard of living and the Armed Forces' approval fall), disband battalions you don't need, or keep older, cheaper unit equipment until a war is coming. Mobilising multiplies these costs, so a war while in deficit drains the treasury fast.".into()),
            goal: None,
            check: None,
        });
    }
    if urgent {
        out.push(Advice {
            id: "budget:war".into(),
            domain: Domain::War,
            priority: 1,
            title: "Do not start a war yet".into(),
            detail: "Mobilised armies cost far more than peacetime ones, and the budget is already heading for default.".into(),
            steps: vec![
                "Diplomacy panel: back down from or settle any Diplomatic Play you are in before it turns into a war.".into(),
                "Start no Diplomatic Play until the weekly balance in the Budget panel is positive.".into(),
            ],
            why_now: Vec::new(),
            why: Some("Mobilisation adds wages and supply goods for every mobilised battalion, and lost trade during a war cuts tax income. Starting a war in debt is the classic road to default.".into()),
            goal: None,
            check: Some(if over { Check::Solvent } else { Check::WeeklyBalance { at_least: 0.0 } }),
        });
    }

    let government = share(&["government_goods", "government_wages"]);
    if government >= 0.2 {
        out.push(Advice {
            id: "budget:government".into(),
            domain: Domain::Economy,
            priority: 2,
            title: format!("Administration is {:.0}% of spending", government * 100.0),
            detail: format!("{}.", lines(&["government_goods", "government_wages"])),
            steps: vec![
                "Construction panel: queue Paper Mills in a state with Logging Camps: paper is the main good government buildings buy.".into(),
                "Budget panel: set Government Wages one step lower, only while bureaucracy shows a surplus (Budget panel, Bureaucracy).".into(),
            ],
            why_now: Vec::new(),
            why: Some("Government administrations produce bureaucracy, which laws, institutions and taxes consume; cutting below what you use brings penalties, so cut the price instead of the buildings. Government goods are mostly paper: a cheap local supply (paper mills, wood) lowers this line. Government wages can be lowered in the Budget panel at the cost of bureaucrats' standard of living.".into()),
            goal: None,
            check: None,
        });
    }

    let support = r.expense(&["subsidies", "government_share_losses"]);
    if spend > 0.0 && support / spend >= 0.05 {
        out.push(Advice {
            id: "budget:subsidies".into(),
            domain: Domain::Economy,
            priority: 2,
            title: format!("Subsidies and loss-making state buildings cost {} a week", money(support)),
            detail: format!("{}.", lines(&["subsidies", "government_share_losses"])),
            steps: vec![
                "Open each building named above and turn off its subsidy in the building panel, unless it makes construction materials or arms you need.".into(),
                "For a loss-making state-owned building: open it and press Downsize, or pick a cheaper production method.".into(),
            ],
            why_now: Vec::new(),
            why: Some("Subsidies keep unprofitable buildings staffed at the state's expense. They are worth it for what you need (construction inputs, arms before a war); otherwise turn them off in the building's panel and let the market decide.".into()),
            goal: None,
            check: None,
        });
    }

    // Higher taxes cost legitimacy and standard of living; an investment deficit does not need them.
    if investment {
        return;
    }
    out.push(Advice {
        id: "budget:revenue".into(),
        domain: Domain::Economy,
        priority: 2,
        title: "Raise revenue: the tax level one step up".into(),
        detail: format!("Income: {}.", describe(&r.incomes, 4)),
        steps: vec![
            "Budget panel: raise the tax level one step.".into(),
            "Budget panel, Consumption Taxes: if the option is available, tax a good your pops buy a lot of (liquor, tobacco, wine).".into(),
        ],
        why_now: Vec::new(),
        why: Some("The tax level in the Budget panel scales income and poll taxes: one step up is the quickest fix, but it lowers pops' standard of living and grows radicals. Consumption taxes on goods your pops buy a lot of add income without touching the tax level. In the long run revenue follows the economy: profitable, well-paid buildings pay income tax, and taxation laws decide who pays.".into()),
        goal: None,
        check: Some(Check::WeeklyBalance { at_least: 0.0 }),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use vic3_prices::BudgetLine;

    fn line(income: bool, group: &str, building: &str, amount: f64) -> BudgetLine {
        BudgetLine { income, group: group.into(), building_type: building.into(), amount }
    }

    /// Prussia 1836 in miniature: deficit driven by the army.
    fn prussia() -> WorldCountry {
        WorldCountry {
            treasury: 486_803.0,
            weekly_balance: Some(-7_231.0),
            weekly_income: Some(57_854.0),
            weekly_expenses: Some(65_085.0),
            credit_limit: Some(17_058_032.0),
            budget_lines: vec![
                line(false, "military_goods", "building_army_logistics_center", 24_769.0),
                line(false, "military_wages", "building_barrack", 14_560.0),
                line(false, "government_goods", "building_government_administration", 15_262.0),
                line(false, "government_wages", "building_government_administration", 6_316.0),
                line(true, "poll_taxes", "building_subsistence_farm", 19_417.0),
                line(true, "income_taxes", "building_manor_house", 1_104.0),
            ],
            ..WorldCountry::default()
        }
    }

    #[test]
    fn deficit_report_has_runway_and_breakdown() {
        let defs = GameDefs::default();
        let r = report(&defs, &prussia()).expect("report");
        assert_eq!(r.risk, Risk::Deficit);
        assert_eq!(r.weeks_to_debt.map(f64::round), Some(67.0));
        assert_eq!(r.expenses[0].group, "military_goods");
        // Unlisted spending lands in "other"; shares are of the save totals.
        let other = r.expenses.iter().find(|g| g.group == "other").expect("other");
        assert!((other.amount - (65_085.0 - 60_907.0)).abs() < 1e-6);
        assert!((r.expenses.iter().map(|g| g.share).sum::<f64>() - 1.0).abs() < 1e-9);

        let mut out = Vec::new();
        budget_advice(&r, &mut out);
        let ids: Vec<&str> = out.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["budget:balance", "budget:military", "budget:government", "budget:revenue"]);
        assert_eq!(out[0].priority, 2);
        assert!(out[0].detail.contains("67 weeks"), "{}", out[0].detail);
        assert!(out.iter().all(|a| a.why.is_some() && !a.steps.is_empty()));

        // Mobilised, the army's goods bill doubles: 24.8K more a week.
        let war = r.war.expect("war estimate");
        assert!((war.extra_weekly - 24_769.0).abs() < 1e-6);
        assert!((war.weekly_balance - (-7_231.0 - 24_769.0)).abs() < 1e-6);
        let to_default = (486_803.0 + 17_058_032.0) / 32_000.0;
        assert!(war.weeks_to_default.is_some_and(|w| (w - to_default).abs() < 1e-6));
    }

    #[test]
    fn debt_with_deficit_is_danger_and_over_limit_is_critical() {
        let defs = GameDefs::default();
        let mut c = prussia();
        c.treasury = 0.0;
        // A little debt against a 17M limit is cheap borrowing, not danger.
        c.debt_principal = Some(1_000_000.0);
        assert_eq!(report(&defs, &c).expect("report").risk, Risk::Deficit);
        // Past half the credit limit it is.
        c.debt_principal = Some(9_000_000.0);
        let r = report(&defs, &c).expect("report");
        assert_eq!(r.risk, Risk::Danger);
        let mut out = Vec::new();
        budget_advice(&r, &mut out);
        assert!(out.iter().any(|a| a.id == "budget:war" && a.priority == 1));

        c.debt_principal = c.credit_limit;
        let r = report(&defs, &c).expect("report");
        assert_eq!(r.risk, Risk::Critical);
        let mut out = Vec::new();
        budget_advice(&r, &mut out);
        assert_eq!(out[0].check, Some(Check::Solvent));
    }

    #[test]
    fn construction_heavy_deficit_is_investment_and_skips_tax_advice() {
        // Prussia 1836.2.11: −32.6K a week, 35.6K of it construction, treasury nearly gone.
        let defs = GameDefs::default();
        let mut c = prussia();
        c.treasury = 330_000.0;
        c.weekly_balance = Some(-32_557.0);
        c.weekly_income = Some(68_413.0);
        c.weekly_expenses = Some(100_970.0);
        c.budget_lines.push(line(false, "construction_goods", "building_construction_sector", 35_600.0));
        let r = report(&defs, &c).expect("report");
        assert_eq!(r.deficit_kind, Some(DeficitKind::Investment));
        // Ten weeks of treasury, but years before debt reaches half of the 17M limit.
        assert!(r.weeks_to_debt.unwrap() < 26.0 && r.weeks_to_unhealthy.unwrap() > 200.0);
        assert_eq!(r.risk, Risk::Deficit);
        let mut out = Vec::new();
        budget_advice(&r, &mut out);
        assert!(out[0].title.starts_with("Investment deficit"), "{}", out[0].title);
        assert!(out[0].detail.contains("keep building"), "{}", out[0].detail);
        let military = out.iter().find(|a| a.id == "budget:military").expect("military");
        assert_eq!(military.priority, 1);
        assert!(out.iter().all(|a| a.id != "budget:revenue"), "no tax advice for an investment deficit");
    }

    #[test]
    fn surplus_with_idle_construction_suggests_spending() {
        let defs = GameDefs::default();
        let mut c = prussia();
        c.weekly_balance = Some(5_000.0);
        c.treasury = 5_000_000.0;
        let r = report(&defs, &c).expect("report");
        assert_eq!(r.risk, Risk::Safe);
        let mut out = Vec::new();
        budget_advice(&r, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "budget:spend");
    }
}
