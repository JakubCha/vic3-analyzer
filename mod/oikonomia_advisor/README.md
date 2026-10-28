# Oikonomia Advisor (Victoria 3 mod, 1.13)

Player-only journal entries with live values computed by the game every tick. They are added on the first day of each month (so they also appear in campaigns started without the mod):

- **Budget Guard** – appears while the state loses money, disappears when the balance turns positive. Shows weekly balance, treasury, weeks until borrowing starts, debt relative to the credit limit and the military share of spending, with a deficit / danger / critical verdict and the levers that fix it.
- **Playbook: Prussia's Opening** (PRU) – checklist: military under 40% of spending, non-negative balance, paper and small arms near base price, 10 construction sector levels. Completes when the army is under 40% and the budget balances.
- **Playbook: Road to Germany** (PRU / NGF) – public schools, nationalism, Schleswig-Holstein solved, debt under 30% of credit, North German Federation, pan-nationalism. Completes when Germany is formed.

- **Playbook entries for every country** – one journal entry per analyzer playbook rule (general rules plus Prussia, Britain, France, Austria, Sardinia/Italy, USA), added while its conditions hold, with the mechanic explained and sources. Generated: run `vic3-cli mod generate --out mod/oikonomia_advisor` after editing `crates/vic3-api/playbook/*.toml` (a test fails when the files are stale). Rules whose conditions the game script cannot check (share of one budget line) stay analyzer-only.
- **Oikonomia: Budget Dashboard** – always-on entry with the live numbers; every week it writes one `OIKO1;country;date;balance;income;treasury;debt‰;military‰;weeks;gdp` line to `logs\debug.log` (works without debug mode), then one `OIKO2;country;date;state;unemployed;infrastructure balance` line per owned state and one `OIKO3;country;date;good;price vs base ‰` line per good at least 20% off base price. The analyzer's overlay and the MCP tool `live_status` read it, so they see in-game values without waiting for a save.
- **Budget alarm event** – a popup when the state is losing money and debt passes 85% of the credit limit (or in default), at most every six months.

- **Own journal group** – everything sits in the collapsible group "Oikonomia Advisor"; the Budget Guard and the dashboard show a debt / credit-limit bar and a budget verdict chosen by the game (customizable localization).
- **How to do it, why now** – every playbook entry lists the steps in the game's interface, links game terms to the game's own concept tooltips (credit limit, default, infrastructure, …) and shows the live values its rule checks (debt share, balance, building levels) against the rule's thresholds.
- **Three tips at a time** – at most three playbook entries are open at once (priority-1 tips always come in); each has a "Skip this tip" button that drops it for the campaign.
- **Chapters** – one five-item checklist per era for every country: Foundations (to 1850), Industry (to 1885), Modern State.
- **Construction Is Running Dry** – entry and top-bar alert when the state's construction queue has under two weeks of work.
- **Warnings at the moment of decision** – popups when a diplomatic play starts with a weak budget, when a law is started with high debt, and after a default.
- **Top-bar alerts** – idle construction, debt past half the credit limit, deficit paying the army.
- **Game rules** (chosen at game start) – help level (budget only / standard / everything) and warning popups on or off.
- **Cheat menu** (game rule, off by default; on blocks achievements) – journal entry with buttons: add ten weeks of spending, pay off the debt (ends a default), one-year boosts to research, construction and law enactment, loyalists, −10 infamy, +20 relations with every country.

Hand-written texts are in Polish and English; generated playbook texts are in English.

## Install

Copy this folder to `Documents\Paradox Interactive\Victoria 3\mod\oikonomia_advisor` and enable it in the launcher playset. Like any mod it changes the checksum: no Ironman achievements, and every multiplayer player needs it.

Errors, if any, appear in `Documents\Paradox Interactive\Victoria 3\logs\error.log` (search for `oiko`).

## Checking the mod without the game

`crates/vic3-api/tests/mod_lint.rs` checks every file of this mod against the game's own files: law, technology, building, goods, interest-group and country ids, concept links, icons, every localization key in English and Polish, and every cross-reference (buttons, script values, events, game rules, journal entries). It does not check trigger syntax; `error.log` still has the last word.

```
VIC3_GAME_DIR="M:/games/Victoria 3" cargo test -p vic3-api --test mod_lint
```

Polish button names in the steps come from `crates/vic3-api/playbook/ui_terms.toml`, built from the game's localization by `scripts/ui_terms.js`.
