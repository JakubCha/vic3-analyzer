# Oikonomia Advisor (Victoria 3 mod, 1.13)

Player-only journal entries with live values computed by the game every tick. They are added on the first day of each month (so they also appear in campaigns started without the mod):

- **Budget Guard** – appears while the state loses money, disappears when the balance turns positive. Shows weekly balance, treasury, weeks until borrowing starts, debt relative to the credit limit and the military share of spending, with a deficit / danger / critical verdict and the levers that fix it.
- **Playbook: Prussia's Opening** (PRU) – checklist: military under 40% of spending, non-negative balance, paper and small arms near base price, 10 construction sector levels. Completes when the army is under 40% and the budget balances.
- **Playbook: Road to Germany** (PRU / NGF) – public schools, nationalism, Schleswig-Holstein solved, debt under 30% of credit, North German Federation, pan-nationalism. Completes when Germany is formed.

- **Playbook entries for every country** – one journal entry per analyzer playbook rule (general rules plus Prussia, Britain, France, Austria, Sardinia/Italy, USA), added while its conditions hold, with the mechanic explained and sources. Generated: run `vic3-cli mod generate --out mod/oikonomia_advisor` after editing `crates/vic3-api/playbook/*.toml` (a test fails when the files are stale). Rules whose conditions the game script cannot check (share of one budget line) stay analyzer-only.
- **Oikonomia: Budget Dashboard** – always-on entry with the live numbers; every week it writes one `OIKO1;country;date;balance;income;treasury;debt‰;military‰;weeks;gdp` line to `logs\debug.log` (works without debug mode), then one `OIKO2;country;date;state;unemployed;infrastructure balance` line per owned state and one `OIKO3;country;date;good;price vs base ‰` line per good at least 20% off base price. The analyzer's overlay and the MCP tool `live_status` read it, so they see in-game values without waiting for a save.
- **Budget alarm event** – a popup when the state is losing money and debt passes 85% of the credit limit (or in default), at most every six months.

Hand-written texts are in Polish and English; generated playbook texts are in English.

## Install

Copy this folder to `Documents\Paradox Interactive\Victoria 3\mod\oikonomia_advisor` and enable it in the launcher playset. Like any mod it changes the checksum: no Ironman achievements, and every multiplayer player needs it.

Errors, if any, appear in `Documents\Paradox Interactive\Victoria 3\logs\error.log` (search for `oiko`).
