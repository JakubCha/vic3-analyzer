//! Live link to a running game: the Oikonomia Advisor mod writes one `OIKO1;…`
//! line per week to `logs/debug.log` (its `oiko_log_status` scripted effect).
//! This reads the newest one, so the overlay and agents see in-game values
//! without waiting for an autosave.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MARKER: &str = "OIKO1;";
/// Per owned state, logged right after its week's `OIKO1` line.
const STATE_MARKER: &str = "OIKO2;";
/// Per dear or cheap good on the home market, after the state lines.
const GOODS_MARKER: &str = "OIKO3;";
/// Enough of the log's tail for several weeks of lines (with state and goods lines) even in a noisy log.
const TAIL_BYTES: u64 = 2 * 1024 * 1024;

/// One owned state that week.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveState {
    /// Localized state name.
    pub name: String,
    pub unemployed: f64,
    /// Infrastructure minus its use; negative = short.
    pub infrastructure_balance: f64,
}

/// One good at least 20% off its base price on the home market that week.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveGood {
    /// Localized goods name.
    pub name: String,
    /// Price over base minus 1 (0.3 = 30% dearer, −0.25 = 25% cheaper).
    pub price_vs_base: f64,
}

/// One weekly line from the mod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveStatus {
    /// Country tag or name as the game printed it.
    pub country: String,
    /// In-game date as the game printed it (localized text).
    pub date: String,
    /// Wall-clock time of the log line (`HH:MM:SS`).
    pub logged_at: String,
    pub weekly_balance: f64,
    pub weekly_income: f64,
    pub treasury: f64,
    /// Debt over credit limit (1 = default).
    pub debt_share: f64,
    /// Military share of spending.
    pub military_share: f64,
    pub weeks_to_debt: f64,
    pub gdp: f64,
    /// Owned states, from the `OIKO2` lines of the same week (newest line only; not kept in history).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<LiveState>,
    /// Dear or cheap goods, from the `OIKO3` lines of the same week.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub goods: Vec<LiveGood>,
}

/// `[name, a, b]` after `marker` when the line is for `status`'s country and date.
fn detail_fields<'a>(line: &'a str, marker: &str, status: &LiveStatus) -> Option<Vec<&'a str>> {
    let start = line.find(marker)?;
    let fields: Vec<&str> = line[start + marker.len()..].trim_end().split(';').map(str::trim).collect();
    (fields.len() >= 4 && fields[0] == status.country && fields[1] == status.date).then(|| fields[2..].to_vec())
}

/// Adds an `OIKO2`/`OIKO3` line to `status`; a repeated name (the mod also logs monthly) replaces the earlier one.
fn attach(status: &mut LiveStatus, line: &str) {
    if let Some([name, unemployed, infra]) = detail_fields(line, STATE_MARKER, status).as_deref() {
        if let (Some(unemployed), Some(infrastructure_balance)) = (int(unemployed), int(infra)) {
            status.states.retain(|s| s.name != *name);
            status.states.push(LiveState { name: name.to_string(), unemployed, infrastructure_balance });
        }
    } else if let Some([name, delta]) = detail_fields(line, GOODS_MARKER, status).as_deref() {
        if let Some(delta) = int(delta) {
            status.goods.retain(|g| g.name != *name);
            status.goods.push(LiveGood { name: name.to_string(), price_vs_base: delta / 1000.0 });
        }
    }
}

/// Integer printed by the game; drops thousands separators of any locale.
fn int(field: &str) -> Option<f64> {
    let negative = field.trim_start().starts_with(['-', '\u{2212}']);
    let digits: String = field.chars().filter(char::is_ascii_digit).collect();
    let n: f64 = digits.parse().ok()?;
    Some(if negative { -n } else { n })
}

/// Parses one log line, `None` when it is not a mod line.
pub fn parse_line(line: &str) -> Option<LiveStatus> {
    let start = line.find(MARKER)?;
    let fields: Vec<&str> = line[start + MARKER.len()..].trim_end().split(';').collect();
    let [country, date, balance, income, treasury, debt, military, weeks, gdp] = fields[..] else {
        return None;
    };
    Some(LiveStatus {
        country: country.trim().to_string(),
        date: date.trim().to_string(),
        logged_at: line.strip_prefix('[').and_then(|l| l.split(']').next()).unwrap_or_default().to_string(),
        weekly_balance: int(balance)?,
        weekly_income: int(income)?,
        treasury: int(treasury)?,
        debt_share: int(debt)? / 1000.0,
        military_share: int(military)? / 1000.0,
        weeks_to_debt: int(weeks)?,
        gdp: int(gdp)?,
        states: Vec::new(),
        goods: Vec::new(),
    })
}

/// Month names the game prints in dates (English and Polish, Polish in the
/// genitive), matched by prefix.
const MONTHS: [&[&str]; 12] = [
    &["jan", "stycz"],
    &["feb", "lut"],
    &["mar"],
    &["apr", "kwie"],
    &["may", "maj"],
    &["jun", "czerw"],
    &["jul", "lip"],
    &["aug", "sierp"],
    &["sep", "wrze"],
    &["oct", "paźdz", "pazdz"],
    &["nov", "listop"],
    &["dec", "grud"],
];

/// `(year, month, day)` from a localized date such as `March 1, 1836` or `1 marca 1836`.
pub fn parse_game_date(text: &str) -> Option<(i16, u8, u8)> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let year = words.iter().find(|w| w.len() == 4).and_then(|w| w.parse().ok())?;
    let day = words.iter().find(|w| w.len() <= 2).and_then(|w| w.parse().ok()).filter(|d| (1..=31).contains(d))?;
    let month = words.iter().find_map(|w| MONTHS.iter().position(|names| names.iter().any(|n| w.starts_with(n))))?;
    Some((year, month as u8 + 1, day))
}

/// Merge `new` lines into the JSON-lines store at `store` (one per country and
/// in-game date) and return the whole history, oldest first. Lines whose date
/// cannot be read are kept out of the store.
pub fn merge_history(store: &Path, new: &[LiveStatus]) -> std::io::Result<Vec<LiveStatus>> {
    let mut all: Vec<LiveStatus> = std::fs::read_to_string(store)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let key = |s: &LiveStatus| parse_game_date(&s.date).map(|d| (s.country.clone(), d));
    let mut seen: std::collections::BTreeSet<_> = all.iter().filter_map(key).collect();
    let before = all.len();
    for line in new {
        if let Some(k) = key(line) {
            if seen.insert(k) {
                // Charts need the national numbers only; per-state and goods lines would grow the store every week.
                all.push(LiveStatus { states: Vec::new(), goods: Vec::new(), ..line.clone() });
            }
        }
    }
    all.sort_by_key(|s| parse_game_date(&s.date));
    if all.len() != before {
        if let Some(dir) = store.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text: String = all.iter().filter_map(|s| serde_json::to_string(s).ok()).map(|l| l + "\n").collect();
        std::fs::write(store, text)?;
    }
    Ok(all)
}

/// Mod lines from `debug.log` and the game's rotated copies (`debug.1.log` …), oldest file first.
pub fn read_all_logs(save_dirs: &[PathBuf]) -> Vec<LiveStatus> {
    let Some(log) = log_path(save_dirs) else { return Vec::new() };
    let dir = log.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut lines = Vec::new();
    for n in (1..=9).rev() {
        if let Ok(mut older) = read_history(&dir.join(format!("debug.{n}.log"))) {
            lines.append(&mut older);
        }
    }
    lines.extend(read_history(&log).unwrap_or_default());
    lines
}

/// Every mod line in `text`, oldest first.
pub fn history(text: &str) -> Vec<LiveStatus> {
    let mut out: Vec<LiveStatus> = Vec::new();
    for line in text.lines() {
        if let Some(status) = parse_line(line) {
            // The monthly call repeats the week's line (with its own details): the newer one wins.
            if out.last().is_some_and(|last| last.country == status.country && last.date == status.date) {
                out.pop();
            }
            out.push(status);
        } else if let Some(last) = out.last_mut() {
            attach(last, line);
        }
    }
    out
}

/// `<game documents>/logs/debug.log` next to the first save folder that has one.
pub fn log_path(save_dirs: &[PathBuf]) -> Option<PathBuf> {
    save_dirs
        .iter()
        .filter_map(|dir| dir.parent())
        .map(|docs| docs.join("logs").join("debug.log"))
        .find(|path| path.is_file())
}

/// Lines from the tail of the log at `path`, oldest first.
pub fn read_history(path: &Path) -> std::io::Result<Vec<LiveStatus>> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(history(&String::from_utf8_lossy(&bytes)))
}

/// Newest line in the game log beside `save_dirs`; `None` without a log or line.
pub fn latest(save_dirs: &[PathBuf]) -> Option<LiveStatus> {
    read_history(&log_path(save_dirs)?).ok()?.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "[21:02:59][jomini_effect_impl.cpp:454]: common/scripted_effects/oiko_effects.txt:5: OIKO1;PRU;1 marca 1836;-7\u{a0}231;57\u{a0}845;486\u{a0}803;0;618;67;9\u{a0}720\u{a0}602";

    #[test]
    fn parses_a_localized_line() {
        let s = parse_line(LINE).expect("line");
        assert_eq!(s.country, "PRU");
        assert_eq!(s.date, "1 marca 1836");
        assert_eq!(s.logged_at, "21:02:59");
        assert_eq!(s.weekly_balance, -7231.0);
        assert_eq!(s.treasury, 486_803.0);
        assert_eq!(s.military_share, 0.618);
        assert_eq!(s.gdp, 9_720_602.0);
        assert!(parse_line("[21:03:00][x.cpp:1]: Peelite Party Created").is_none());
        assert_eq!(int("1,234").unwrap(), 1234.0);
        assert_eq!(int("\u{2212}5").unwrap(), -5.0);
    }

    #[test]
    fn state_and_goods_lines_attach_to_their_week() {
        let prefix = "[21:02:59][jomini_effect_impl.cpp:454]: common/scripted_effects/oiko_effects.txt:9: ";
        let text = [
            LINE.to_string(),
            format!("{prefix}OIKO2;PRU;1 marca 1836;Brandenburgia;12\u{a0}400;-3"),
            format!("{prefix}OIKO2;PRU;1 marca 1836;Śląsk;0;5"),
            format!("{prefix}OIKO3;PRU;1 marca 1836;Drewno;710"),
            format!("{prefix}OIKO3;PRU;1 marca 1836;Tkaniny;\u{2212}250"),
            // The monthly call repeats the week with its own details: the newer set wins.
            LINE.to_string(),
            format!("{prefix}OIKO2;PRU;1 marca 1836;Brandenburgia;12\u{a0}400;-3"),
            format!("{prefix}OIKO2;PRU;1 marca 1836;Śląsk;100;5"),
            format!("{prefix}OIKO2;PRU;1 marca 1836;Śląsk;100;5"),
            format!("{prefix}OIKO3;PRU;1 marca 1836;Drewno;710"),
            format!("{prefix}OIKO3;PRU;1 marca 1836;Tkaniny;\u{2212}250"),
            // Another week's or country's detail line is not attached.
            format!("{prefix}OIKO2;AUS;1 marca 1836;Czechy;1;1"),
        ]
        .join("\n");
        let all = history(&text);
        assert_eq!(all.len(), 1);
        let week = &all[0];
        assert_eq!(week.states.len(), 2);
        let brandenburg = week.states.iter().find(|s| s.name == "Brandenburgia").expect("state");
        assert_eq!((brandenburg.unemployed, brandenburg.infrastructure_balance), (12_400.0, -3.0));
        assert_eq!(week.states.iter().find(|s| s.name == "Śląsk").map(|s| s.unemployed), Some(100.0));
        assert_eq!(week.goods.iter().map(|g| (g.name.as_str(), g.price_vs_base)).collect::<Vec<_>>(), [("Drewno", 0.71), ("Tkaniny", -0.25)]);
        // History keeps the national numbers only.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/tmp/oiko-detail-{}", std::process::id()));
        let stored = merge_history(&root.join("h.jsonl"), &all).unwrap();
        assert!(stored[0].states.is_empty() && stored[0].goods.is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn localized_dates_parse_and_history_merges_once_per_date() {
        assert_eq!(parse_game_date("1 marca 1836"), Some((1836, 3, 1)));
        assert_eq!(parse_game_date("February 27, 1836"), Some((1836, 2, 27)));
        assert_eq!(parse_game_date("12 października 1850"), Some((1850, 10, 12)));
        assert_eq!(parse_game_date("sometime"), None);

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/tmp/oiko-hist-{}", std::process::id()));
        let store = root.join("live_history.jsonl");
        let march = parse_line(LINE).unwrap();
        let mut april = march.clone();
        april.date = "1 kwietnia 1836".into();
        assert_eq!(merge_history(&store, &[april.clone(), march.clone()]).unwrap().len(), 2);
        let all = merge_history(&store, &[march.clone()]).unwrap();
        assert_eq!(all.iter().map(|s| s.date.as_str()).collect::<Vec<_>>(), ["1 marca 1836", "1 kwietnia 1836"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn reads_the_newest_line_from_the_log_beside_the_saves() {
        // Under the workspace target dir, not the system temp dir.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/tmp/oiko-live-{}", std::process::id()));
        let saves = root.join("save games");
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::create_dir_all(root.join("logs")).unwrap();
        let older = LINE.replace("-7\u{a0}231", "-9000");
        std::fs::write(root.join("logs/debug.log"), format!("noise\n{older}\nmore noise\n{LINE}\n")).unwrap();
        let latest = latest(&[saves]).expect("latest");
        assert_eq!(latest.weekly_balance, -7231.0);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
