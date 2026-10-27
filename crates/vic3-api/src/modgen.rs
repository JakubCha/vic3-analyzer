//! Turns playbook rules into Victoria 3 mod files: one journal entry per rule,
//! added to the player by a monthly on_action while the rule's conditions hold.
//! Conditions the game script cannot express (per-category budget shares, income
//! mix) make a rule skipped, with the reason reported.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::advice::{Check, Domain};
use crate::playbook::{Rule, When};

/// Every generated file starts with this (the game wants UTF-8 with BOM).
pub const BOM: &str = "\u{feff}";

/// Generated mod files (paths relative to the mod root) and rules left out.
#[derive(Debug, Default)]
pub struct GeneratedMod {
    pub files: Vec<(String, String)>,
    pub skipped: Vec<(String, String)>,
    pub generated: usize,
}

fn num(x: f64) -> String {
    let s = format!("{x:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn je_name(rule: &Rule) -> String {
    let id: String = rule.id.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    format!("je_oiko_pb_{id}")
}

fn level_value(building: &str) -> String {
    format!("oiko_lvl_{building}")
}

/// Script triggers for `when`, or why the game cannot check it.
fn when_triggers(w: &When, levels: &mut BTreeSet<String>) -> Result<Vec<String>, String> {
    let mut t = Vec::new();
    if let Some(y) = w.from_year {
        t.push(format!("game_date >= {y}.1.1"));
    }
    if let Some(y) = w.until_year {
        t.push(format!("game_date < {}.1.1", y + 1));
    }
    t.extend(w.laws.iter().map(|l| format!("has_law_or_variant = law_type:{l}")));
    t.extend(w.laws_missing.iter().map(|l| format!("NOT = {{ has_law_or_variant = law_type:{l} }}")));
    t.extend(w.techs.iter().map(|x| format!("has_technology_researched = {x}")));
    t.extend(w.techs_missing.iter().map(|x| format!("NOT = {{ has_technology_researched = {x} }}")));
    for (b, n) in &w.levels_below {
        levels.insert(b.clone());
        t.push(format!("{} < {}", level_value(b), num(*n)));
    }
    for (b, n) in &w.levels_at_least {
        levels.insert(b.clone());
        t.push(format!("{} >= {}", level_value(b), num(*n)));
    }
    if let Some(x) = w.balance_below {
        t.push(format!("net_fixed_income < {}", num(x)));
    }
    if let Some(x) = w.balance_at_least {
        t.push(format!("net_fixed_income >= {}", num(x)));
    }
    if let Some(x) = w.balance_share_of_income_below {
        t.push(format!("oiko_pb_balance_share < {}", num(x)));
    }
    if let Some(x) = w.balance_share_of_income_at_least {
        t.push(format!("oiko_pb_balance_share >= {}", num(x)));
    }
    match w.in_debt {
        Some(true) => t.push("scaled_debt > 0".into()),
        Some(false) => t.push("scaled_debt <= 0".into()),
        None => {}
    }
    if let Some(x) = w.debt_share_of_limit_at_least {
        t.push(format!("scaled_debt >= {}", num(x)));
    }
    if let Some(x) = w.debt_share_of_limit_below {
        t.push(format!("scaled_debt < {}", num(x)));
    }
    if let Some(x) = w.runway_weeks_below {
        t.push("scaled_debt <= 0".into());
        t.push("net_fixed_income < 0".into());
        t.push(format!("oiko_weeks_to_debt < {}", num(x)));
    }
    for (group, x) in &w.expense_share_at_least {
        if group != "military" {
            return Err(format!("the game has no trigger for the {group} share of spending"));
        }
        t.push(format!("military_expenses_share >= {}", num(*x)));
    }
    if !w.expense_ratio_at_least.is_empty() {
        return Err("the game has no trigger for one expense line over another".into());
    }
    if !w.income_share_at_least.is_empty() || !w.income_share_below.is_empty() {
        return Err("the game has no trigger for the share of one income line".into());
    }
    t.extend(w.igs_in_government.iter().map(|ig| format!("ig:{ig} ?= {{ is_in_government = yes }}")));
    t.extend(w.igs_not_in_government.iter().map(|ig| format!("ig:{ig} ?= {{ is_in_government = no }}")));
    t.extend(w.ig_clout_at_least.iter().map(|(ig, x)| format!("ig:{ig} ?= {{ ig_clout >= {} }}", num(*x))));
    t.extend(w.ig_clout_below.iter().map(|(ig, x)| format!("NOT = {{ ig:{ig} ?= {{ ig_clout >= {} }} }}", num(*x))));
    let price = |good: &str, cmp: &str, x: f64| {
        format!("market_capital.market = {{ mg:{good} = {{ market_goods_pricier {cmp} {} }} }}", num(x))
    };
    t.extend(w.price_above_base.iter().map(|(g, x)| price(g, ">=", *x)));
    if !w.any_price_above_base.is_empty() {
        let any: Vec<String> = w.any_price_above_base.iter().map(|(g, x)| price(g, ">=", *x)).collect();
        t.push(format!("OR = {{ {} }}", any.join(" ")));
    }
    t.extend(w.price_ratio_at_most.iter().map(|(g, r)| price(g, "<=", r - 1.0)));
    Ok(t)
}

/// Trigger for a rule's done check, when the game can evaluate it.
fn done_trigger(check: &Check, levels: &mut BTreeSet<String>) -> Option<String> {
    Some(match check {
        Check::LawActive { law } => format!("has_law_or_variant = law_type:{law}"),
        Check::LawInactive { law } => format!("NOT = {{ has_law_or_variant = law_type:{law} }}"),
        Check::WeeklyBalance { at_least } => format!("net_fixed_income >= {}", num(*at_least)),
        Check::Solvent => "scaled_debt < 1".into(),
        Check::BuildingLevels { building_type, state_id: None, at_least } => {
            levels.insert(building_type.clone());
            format!("{} >= {}", level_value(building_type), num(*at_least))
        }
        Check::BuildingLevels { .. } | Check::ProductionMethod { .. } => return None,
    })
}

/// Localization-safe text: no data-function brackets, formatting or quotes.
fn loc_text(s: &str) -> String {
    s.replace('"', "'").replace('[', "(").replace(']', ")").replace(['#', '$', '@'], "").replace('\n', "\\n")
}

fn block(out: &mut String, indent: &str, name: &str, lines: &[String]) {
    let _ = writeln!(out, "{indent}{name} = {{");
    for line in lines {
        let _ = writeln!(out, "{indent}\t{line}");
    }
    let _ = writeln!(out, "{indent}}}");
}

/// Mod files for `rules`. Countries filter to `OR = { c:TAG ?= root … }`.
pub fn generate(rules: &[Rule]) -> GeneratedMod {
    let mut out = GeneratedMod::default();
    let mut levels = BTreeSet::new();
    let mut jes = String::from(BOM);
    jes.push_str("# Generated by `vic3-cli mod generate` from crates/vic3-api/playbook/*.toml. Do not edit.\n\n");
    let mut monthly = String::new();
    let mut loc = String::new();

    for rule in rules {
        let mut needed = BTreeSet::new();
        let mut when = match when_triggers(&rule.when, &mut needed) {
            Ok(t) => t,
            Err(reason) => {
                out.skipped.push((rule.id.clone(), reason));
                continue;
            }
        };
        if !rule.countries.is_empty() {
            let tags: Vec<String> = rule.countries.iter().map(|c| format!("c:{c} ?= root")).collect();
            when.insert(0, format!("OR = {{ {} }}", tags.join(" ")));
        }
        let done = rule.done.as_ref().and_then(|c| done_trigger(c, &mut needed));
        levels.extend(needed);
        out.generated += 1;

        let name = je_name(rule);
        let done_var = format!("{name}_done");
        let icon = match rule.domain {
            Domain::Economy => "gfx/interface/icons/event_icons/event_industry.dds",
            Domain::Laws => "gfx/interface/icons/event_icons/event_portrait.dds",
            Domain::War => "gfx/interface/icons/event_icons/event_military.dds",
        };
        let _ = writeln!(jes, "# {}", rule.id);
        let _ = writeln!(jes, "{name} = {{");
        let _ = writeln!(jes, "\ticon = \"{icon}\"");
        let _ = writeln!(jes, "\tgroup = je_group_internal_affairs");
        let when_all = format!("AND = {{ {} }}", when.join(" "));
        match &done {
            Some(done) => {
                block(&mut jes, "\t", "complete", std::slice::from_ref(done));
                block(&mut jes, "\t", "on_complete", &[format!("set_variable = {done_var}")]);
                // Silently drop the entry once it no longer applies and was not followed.
                block(&mut jes, "\t", "invalid", &[format!("NOT = {{ {when_all} }}"), format!("NOT = {{ {done} }}")]);
            }
            None => block(&mut jes, "\t", "invalid", &[format!("NOT = {{ {when_all} }}")]),
        }
        if rule.priority == 1 {
            let _ = writeln!(jes, "\tshould_be_pinned_by_default_uninvolved_or_context = yes");
        }
        let _ = writeln!(jes, "\tweight = {}", if rule.priority == 1 { 700 } else { 500 });
        let _ = writeln!(jes, "}}\n");

        let mut limit = when.clone();
        limit.push(format!("NOT = {{ has_journal_entry = {name} }}"));
        limit.push(format!("NOT = {{ has_variable = {done_var} }}"));
        let _ = writeln!(monthly, "\t\tif = {{");
        block(&mut monthly, "\t\t\t", "limit", &limit);
        let _ = writeln!(monthly, "\t\t\tadd_journal_entry = {{ type = {name} }}");
        let _ = writeln!(monthly, "\t\t}}");

        let sources = if rule.sources.is_empty() {
            String::new()
        } else {
            format!("\n\nSources ({}): {}", rule.patch, rule.sources.join(", "))
        };
        let _ = writeln!(loc, " {name}:0 \"{}\"", loc_text(&format!("Playbook: {}", rule.title)));
        let _ = writeln!(
            loc,
            " {name}_reason:0 \"{}\"",
            loc_text(&format!("{} · {}\n\n{}\n\nHow it works: {}{sources}", rule.goal, rule.phase, rule.detail, rule.why))
        );
    }

    let mut actions = String::from(BOM);
    actions.push_str("# Generated by `vic3-cli mod generate`. Do not edit.\n\n");
    actions.push_str("on_monthly_pulse_country = {\n\ton_actions = { oiko_playbook_monthly }\n}\n\n");
    actions.push_str("oiko_playbook_monthly = {\n\ttrigger = {\n\t\tis_player = yes\n\t}\n\teffect = {\n");
    actions.push_str(&monthly);
    actions.push_str("\t}\n}\n");

    let mut values = String::from(BOM);
    values.push_str("# Generated by `vic3-cli mod generate`. Do not edit.\n\n");
    values.push_str(
        "# Weekly balance over weekly income.\noiko_pb_balance_share = {\n\tvalue = 0\n\tif = {\n\t\tlimit = { fixed_income > 0 }\n\t\tvalue = net_fixed_income\n\t\tdivide = fixed_income\n\t}\n}\n\n",
    );
    for b in &levels {
        let _ = writeln!(
            values,
            "{} = {{\n\tvalue = 0\n\tevery_scope_building = {{\n\t\tlimit = {{ is_building_type = {b} }}\n\t\tadd = this.level\n\t}}\n}}\n",
            level_value(b)
        );
    }

    out.files = vec![
        ("common/journal_entries/oiko_playbook.txt".into(), jes),
        ("common/on_actions/oiko_playbook_on_actions.txt".into(), actions),
        ("common/script_values/oiko_playbook_values.txt".into(), values),
        // Rules are written in English; the Polish file repeats them so a Polish game shows text, not keys.
        ("localization/english/oiko_playbook_l_english.yml".into(), format!("{BOM}l_english:\n{loc}")),
        ("localization/polish/oiko_playbook_l_polish.yml".into(), format!("{BOM}l_polish:\n{loc}")),
    ];
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(toml_text: &str) -> Rule {
        #[derive(serde::Deserialize)]
        struct One {
            rule: Vec<Rule>,
        }
        toml::from_str::<One>(toml_text).expect("rule").rule.remove(0)
    }

    #[test]
    fn rule_becomes_journal_entry_monthly_add_and_loc() {
        let r = rule(
            r#"
[[rule]]
id = "pru.test-rule"
countries = ["PRU", "NGF"]
domain = "laws"
priority = 1
goal = "reforms"
phase = "early"
title = "Keep \"Public\" [Schools]"
detail = "Detail."
why = "Because."
patch = "1.13"
done = { type = "law_active", law = "law_public_schools" }
[rule.when]
from_year = 1840
laws_missing = ["law_public_schools"]
levels_below = { building_paper_mill = 5.0 }
igs_not_in_government = ["ig_landowners"]
price_ratio_at_most = { iron = 1.1 }
"#,
        );
        let m = generate(std::slice::from_ref(&r));
        assert_eq!((m.generated, m.skipped.len()), (1, 0));
        let file = |p: &str| &m.files.iter().find(|(path, _)| path.ends_with(p)).expect(p).1;
        let jes = file("oiko_playbook.txt");
        assert!(jes.starts_with(BOM) && !jes[3..].starts_with(BOM));
        assert!(jes.contains("je_oiko_pb_pru_test_rule = {"));
        assert!(jes.contains("complete = {\n\t\thas_law_or_variant = law_type:law_public_schools"));
        assert!(jes.contains("should_be_pinned_by_default_uninvolved_or_context = yes"));
        let actions = file("oiko_playbook_on_actions.txt");
        for needle in [
            "OR = { c:PRU ?= root c:NGF ?= root }",
            "game_date >= 1840.1.1",
            "NOT = { has_law_or_variant = law_type:law_public_schools }",
            "oiko_lvl_building_paper_mill < 5",
            "ig:ig_landowners ?= { is_in_government = no }",
            "market_capital.market = { mg:iron = { market_goods_pricier <= 0.1 } }",
            "NOT = { has_variable = je_oiko_pb_pru_test_rule_done }",
            "add_journal_entry = { type = je_oiko_pb_pru_test_rule }",
        ] {
            assert!(actions.contains(needle), "{needle} missing:\n{actions}");
        }
        assert!(file("oiko_playbook_values.txt").contains("oiko_lvl_building_paper_mill = {"));
        let loc = file("oiko_playbook_l_english.yml");
        assert!(loc.contains("je_oiko_pb_pru_test_rule:0 \"Playbook: Keep 'Public' (Schools)\""), "{loc}");
    }

    #[test]
    fn committed_mod_files_match_the_playbook() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mod/oikonomia_advisor");
        for (path, text) in generate(crate::playbook::rules()).files {
            let on_disk = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
            assert!(on_disk == text, "{path} is stale: run `vic3-cli mod generate --out mod/oikonomia_advisor`");
        }
    }

    #[test]
    fn all_embedded_rules_generate_balanced_files() {
        let m = generate(crate::playbook::rules());
        assert!(m.generated >= 50, "only {} generated, skipped {:?}", m.generated, m.skipped);
        assert!(m.skipped.iter().all(|(_, why)| why.starts_with("the game has no trigger")));
        for (path, text) in &m.files {
            assert_eq!(text.matches('{').count(), text.matches('}').count(), "{path}");
            assert!(text.starts_with(BOM) && !text[3..].starts_with(BOM), "{path}");
        }
    }
}
