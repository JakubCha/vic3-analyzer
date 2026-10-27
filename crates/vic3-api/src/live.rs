//! Live link to a running game: the Oikonomia Advisor mod writes one `OIKO1;…`
//! line per week to `logs/debug.log` (its `oiko_log_status` scripted effect).
//! This reads the newest one, so the overlay and agents see in-game values
//! without waiting for an autosave.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MARKER: &str = "OIKO1;";
/// Enough of the log's tail for several weeks of lines even in a noisy log.
const TAIL_BYTES: u64 = 512 * 1024;

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
                all.push(line.clone());
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
    text.lines().filter_map(parse_line).collect()
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
