//! Offline check of the Oikonomia Advisor mod against the game's own files: every
//! game id, icon, concept, localization key and cross-reference it uses must exist.
//! Catches typos before they reach `error.log`; it does not check trigger syntax.
//!
//! Needs the game: set `VIC3_GAME_DIR` to the folder that holds `game/`
//! (e.g. `M:/games/Victoria 3`). Without it the test only prints that it skipped.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn read_dir_text(dir: &Path, ext: &str) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(read_dir_text(&p, ext));
        } else if p.extension().is_some_and(|x| x == ext) {
            if let Ok(t) = std::fs::read_to_string(&p) {
                out.push((p, t.trim_start_matches('\u{feff}').to_string()));
            }
        }
    }
    out
}

/// Lines without comments (outside quotes).
fn code_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            let mut quoted = false;
            let mut end = l.len();
            for (i, c) in l.char_indices() {
                match c {
                    '"' => quoted = !quoted,
                    '#' if !quoted => {
                        end = i;
                        break;
                    }
                    _ => {}
                }
            }
            l[..end].to_string()
        })
        .collect()
}

/// `name = {` keys (block on the same line or not) at the given tab depth.
fn keys_at(text: &str, depth: usize) -> BTreeSet<String> {
    code_lines(text)
        .iter()
        .filter_map(|l| {
            let tabs = l.chars().take_while(|c| *c == '\t').count();
            let (key, rest) = l[tabs..].split_once('=')?;
            (tabs == depth && rest.trim_start().starts_with('{')).then(|| key.trim().to_string())
        })
        .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || "_.-".contains(c)))
        .collect()
}

fn top_keys(files: &[(PathBuf, String)]) -> BTreeSet<String> {
    files.iter().flat_map(|(_, t)| keys_at(t, 0)).collect()
}

/// Bare-word values of `key = value` pairs anywhere in `text`.
fn values_of(text: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    for l in code_lines(text) {
        let toks: Vec<&str> = l.split_whitespace().collect();
        for w in toks.windows(3) {
            if w[0] == key && ["=", ">=", "<=", "<", ">"].contains(&w[1]) && w[2] != "{" {
                out.push(w[2].trim_matches('"').to_string());
            }
        }
    }
    out
}

/// Words that follow `prefix` in `text` (e.g. `law_type:` → law ids).
fn after(text: &str, prefix: &str) -> Vec<String> {
    text.match_indices(prefix)
        .map(|(i, _)| text[i + prefix.len()..].chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect::<String>())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Localization keys of every `.yml` under `dir`.
fn loc_keys(dir: &Path) -> BTreeSet<String> {
    read_dir_text(dir, "yml")
        .iter()
        .flat_map(|(_, t)| t.lines().filter_map(|l| l.strip_prefix(' ')?.split_once(':').map(|(k, _)| k.to_string())).collect::<Vec<_>>())
        .collect()
}

#[test]
fn mod_uses_only_ids_and_keys_that_exist() {
    let Some(game_root) = std::env::var_os("VIC3_GAME_DIR").map(PathBuf::from) else {
        eprintln!("skipped: set VIC3_GAME_DIR to the Victoria 3 folder to lint the mod against the game files");
        return;
    };
    let game = game_root.join("game");
    assert!(game.join("common").is_dir(), "VIC3_GAME_DIR has no game/common: {}", game.display());
    let modroot = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mod/oikonomia_advisor");
    let common = |p: &str| read_dir_text(&game.join("common").join(p), "txt");
    let mine = |p: &str| read_dir_text(&modroot.join("common").join(p), "txt");

    let laws = top_keys(&common("laws"));
    let techs = top_keys(&common("technology/technologies"));
    let buildings = top_keys(&common("buildings"));
    let goods = top_keys(&common("goods"));
    let concepts: BTreeSet<String> = common("game_concepts").iter().flat_map(|(_, t)| keys_at(t, 0)).collect();
    let igs = top_keys(&common("interest_groups"));
    let tags = top_keys(&common("country_definitions"));
    let vanilla_groups = top_keys(&common("journal_entry_groups"));

    let script: Vec<(PathBuf, String)> = read_dir_text(&modroot.join("common"), "txt").into_iter().chain(read_dir_text(&modroot.join("events"), "txt")).collect();
    let all_script: String = script.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
    let yml: String = read_dir_text(&modroot.join("localization"), "yml").iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
    let text = format!("{all_script}\n{yml}");
    let mut errors: Vec<String> = Vec::new();
    let mut need = |what: &str, id: &str, ok: bool| {
        if !ok {
            errors.push(format!("{what} {id}"));
        }
    };

    // Game ids.
    for id in after(&all_script, "law_type:") {
        need("law", &id, laws.contains(&id));
    }
    for id in values_of(&all_script, "has_technology_researched") {
        need("technology", &id, techs.contains(&id));
    }
    for id in values_of(&all_script, "is_building_type").into_iter().chain(values_of(&all_script, "building")) {
        need("building", &id, buildings.contains(&id));
    }
    for id in after(&all_script, "mg:") {
        need("goods", &id, goods.contains(&id));
    }
    for id in after(&all_script, "ig:") {
        need("interest group", &id, igs.contains(&id));
    }
    for id in after(&all_script, "c:") {
        need("country tag", &id, tags.contains(&id));
    }
    for id in after(&yml, "Concept('").into_iter().chain(after(&yml, "[concept_").into_iter().map(|c| format!("concept_{c}"))) {
        need("concept", &id, concepts.contains(&id));
    }
    for path in after(&text, "gfx/") {
        let full = format!("gfx/{path}");
        let file = text.split(&full).nth(1).and_then(|rest| rest.split(['"', ' ', '\n', ')']).next()).unwrap_or_default();
        let rel = format!("{full}{file}");
        need("icon", &rel, game.join(&rel).is_file());
    }

    // Definitions in the mod.
    let defined = |dir: &str| top_keys(&mine(dir));
    let jes = defined("journal_entries");
    let buttons = defined("scripted_buttons");
    let values = defined("script_values");
    let effects = defined("scripted_effects");
    let custom = defined("customizable_localization");
    let bars = defined("scripted_progress_bars");
    let groups = defined("journal_entry_groups");
    let rules = defined("game_rules");
    let alerts = defined("alert_types");
    let modifiers = defined("static_modifiers");
    let settings: BTreeSet<String> = mine("game_rules").iter().flat_map(|(_, t)| keys_at(t, 1)).collect();
    let events: BTreeSet<String> = read_dir_text(&modroot.join("events"), "txt").iter().flat_map(|(_, t)| keys_at(t, 0)).collect();

    for id in values_of(&all_script, "scripted_button") {
        need("scripted button", &id, buttons.contains(&id));
    }
    for id in after(&text, "ScriptValue('") {
        need("script value", &id, values.contains(&id));
    }
    for id in after(&yml, "GetCustom('") {
        need("custom localization", &id, custom.contains(&id));
    }
    for id in values_of(&all_script, "has_game_rule") {
        need("game rule setting", &id, settings.contains(&id));
    }
    for id in values_of(&all_script, "id").into_iter().filter(|i| i.starts_with("oiko.")).chain(after(&all_script, "\t\toiko.").into_iter().map(|n| format!("oiko.{n}"))) {
        need("event", &id, events.contains(&id));
    }
    for id in values_of(&all_script, "has_journal_entry").into_iter().chain(values_of(&all_script, "type").into_iter().filter(|t| t.starts_with("je_"))) {
        need("journal entry", &id, jes.contains(&id));
    }
    for id in values_of(&all_script, "scripted_progress_bar") {
        need("progress bar", &id, bars.contains(&id));
    }
    for id in values_of(&all_script, "group") {
        need("journal group", &id, groups.contains(&id) || vanilla_groups.contains(&id));
    }
    for id in values_of(&all_script, "has_modifier").into_iter().chain(values_of(&all_script, "name").into_iter().filter(|n| n.starts_with("oiko_cheat_") && !n.ends_with("_button"))) {
        need("static modifier", &id, modifiers.contains(&id));
    }
    // Our own names used as values or comparisons must be script values or effects.
    let used: BTreeSet<String> = code_lines(&all_script)
        .iter()
        .flat_map(|l| l.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .filter(|w| w.starts_with("oiko_") && w.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .collect();
    let saved: BTreeSet<String> = values_of(&all_script, "save_temporary_scope_as").into_iter().chain(values_of(&all_script, "save_scope_as")).collect();
    for id in after(&text, "sCountry('") {
        need("saved scope", &id, saved.contains(&id));
    }
    let variables: BTreeSet<String> = values_of(&all_script, "set_variable").into_iter().chain(values_of(&all_script, "has_variable")).chain(values_of(&all_script, "name")).chain(saved.iter().cloned()).collect();
    let on_actions = defined("on_actions");
    let loc_values: BTreeSet<String> = values_of(&all_script, "localization_key").into_iter().collect();
    for id in used {
        let known = [&values, &effects, &variables, &jes, &buttons, &custom, &bars, &rules, &settings, &alerts, &modifiers, &groups, &on_actions, &loc_values]
            .iter()
            .any(|set| set.contains(&id));
        need("oiko_ name (script value, effect, variable or definition)", &id, known);
    }

    // Localization: every key the scripts name, in both languages.
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for je in &jes {
        keys.insert(je.clone());
        keys.insert(format!("{je}_reason"));
    }
    for g in &groups {
        keys.insert(g.clone());
    }
    for r in &rules {
        keys.insert(format!("rule_{r}"));
    }
    for s in &settings {
        keys.insert(format!("setting_{s}"));
        keys.insert(format!("setting_{s}_desc"));
    }
    for a in &alerts {
        keys.insert(format!("alert_{a}_name"));
        keys.insert(format!("alert_{a}_desc"));
        // The alert settings list (error.log: "Alert is missing localization key-value").
        keys.insert(format!("{a}_setting_name"));
    }
    keys.extend(modifiers.iter().cloned());
    for k in ["name", "desc", "title", "flavor", "status_desc", "localization_key"] {
        keys.extend(values_of(&all_script, k).into_iter().filter(|v| !v.starts_with("oiko_cheat_research") || v.ends_with("_button")).filter(|v| v.contains('_') || v.contains('.')));
    }
    // `name =` of add_modifier / set_bar_progress are definitions, not text.
    keys.retain(|k| !bars.contains(k) || k.ends_with("_desc") || k == "oiko_debt_progress_bar");
    let langs: BTreeMap<&str, BTreeSet<String>> = ["english", "polish"].into_iter().map(|l| (l, loc_keys(&modroot.join("localization").join(l)))).collect();
    for (lang, have) in &langs {
        for k in &keys {
            if !have.contains(k) && !variables.contains(k) {
                errors.push(format!("{lang} localization {k}"));
            }
        }
    }

    errors.sort();
    errors.dedup();
    assert!(errors.is_empty(), "{} problems:\n{}", errors.len(), errors.join("\n"));
}
