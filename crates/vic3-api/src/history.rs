//! Charts over time. The save carries the game's own sampled histories (GDP,
//! standard of living, literacy, prestige, population by strata, radicals and
//! loyalists, from 1836); the mod's weekly log lines add budget history. Lines
//! are thinned with LTTB and drawn as SVG with `plotters` once per request.

use plotters::prelude::*;
use serde::Serialize;
use vic3_load::{CountryTrends, TrendSeries, Vic3Date};
use vic3save::PdsDate;

use crate::live::{parse_game_date, LiveStatus};

/// Points kept per line: plenty for a ~700 px wide chart.
const MAX_POINTS: usize = 600;
const MONTH_START: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
/// Line colours: the app's accent first, then distinct hues.
const PALETTE: [RGBColor; 4] = [RGBColor(42, 74, 106), RGBColor(192, 96, 32), RGBColor(47, 107, 79), RGBColor(150, 60, 120)];

/// Day number on the game's 365-day calendar (no leap years).
pub fn day_number(year: i16, month: u8, day: u8) -> i32 {
    i32::from(year) * 365 + MONTH_START[usize::from(month.clamp(1, 12)) - 1] + i32::from(day) - 1
}

fn year_of(day: f64) -> f64 {
    day / 365.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Money,
    Count,
    Share,
    Plain,
}

/// One line: `(day number, value)` points, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Line {
    pub label: String,
    pub points: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Chart {
    pub key: String,
    pub title: String,
    pub unit: Unit,
    pub lines: Vec<Line>,
}

/// What the UI gets: the chart's SVG plus first/last values per line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderedChart {
    pub key: String,
    pub title: String,
    pub unit: Unit,
    pub svg: String,
    pub lines: Vec<LineSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LineSummary {
    pub label: String,
    pub first: Option<f64>,
    pub last: Option<f64>,
}

fn series_line(label: &str, trend: Option<&TrendSeries>) -> Option<Line> {
    let trend = trend?;
    let (date, values): (Vic3Date, Vec<f64>) = trend.chronological()?;
    if values.is_empty() {
        return None;
    }
    let last = f64::from(day_number(date.year(), date.month(), date.day()));
    let step = trend.interval_days();
    let n = values.len();
    let points = values
        .into_iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .map(|(i, v)| (last - (n - 1 - i) as f64 * step, v))
        .collect();
    Some(Line { label: label.into(), points })
}

/// Histories the save keeps for a country (see `WorldCountry::trends`).
pub fn country_charts(country: &CountryTrends) -> Vec<Chart> {
    let pops = country.pop_statistics.as_ref();
    let chart = |key: &str, title: &str, unit, lines: Vec<Option<Line>>| {
        let lines: Vec<Line> = lines.into_iter().flatten().collect();
        (!lines.is_empty()).then(|| Chart { key: key.into(), title: title.into(), unit, lines })
    };
    [
        chart("gdp", "GDP", Unit::Money, vec![series_line("GDP", country.gdp.as_ref())]),
        chart("sol", "Average standard of living", Unit::Plain, vec![series_line("Standard of living", country.avgsoltrend.as_ref())]),
        chart("literacy", "Literacy", Unit::Share, vec![series_line("Literacy", country.literacy.as_ref())]),
        chart("prestige", "Prestige", Unit::Plain, vec![series_line("Prestige", country.prestige.as_ref())]),
        chart(
            "population",
            "Population by strata",
            Unit::Count,
            vec![
                series_line("Lower", pops.and_then(|p| p.trend_population_lower_strata.as_ref())),
                series_line("Middle", pops.and_then(|p| p.trend_population_middle_strata.as_ref())),
                series_line("Upper", pops.and_then(|p| p.trend_population_upper_strata.as_ref())),
            ],
        ),
        chart(
            "politics",
            "Radicals and loyalists",
            Unit::Count,
            vec![
                series_line("Radicals", pops.and_then(|p| p.trend_radicals.as_ref())),
                series_line("Loyalists", pops.and_then(|p| p.trend_loyalists.as_ref())),
            ],
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Budget charts from the mod's weekly lines for `country` (tag or name as logged).
pub fn budget_charts(lines: &[LiveStatus], country: &str) -> Vec<Chart> {
    let mut rows: Vec<(f64, &LiveStatus)> = lines
        .iter()
        .filter(|s| s.country == country)
        .filter_map(|s| parse_game_date(&s.date).map(|(y, m, d)| (f64::from(day_number(y, m, d)), s)))
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    if rows.len() < 2 {
        return Vec::new();
    }
    let line = |label: &str, f: fn(&LiveStatus) -> f64| Line {
        label: label.into(),
        points: rows.iter().map(|(day, s)| (*day, f(s))).collect(),
    };
    vec![
        Chart {
            key: "budget".into(),
            title: "Weekly income and balance".into(),
            unit: Unit::Money,
            lines: vec![line("Income", |s| s.weekly_income), line("Balance", |s| s.weekly_balance)],
        },
        Chart { key: "treasury".into(), title: "Treasury".into(), unit: Unit::Money, lines: vec![line("Treasury", |s| s.treasury)] },
        Chart {
            key: "debt_military".into(),
            title: "Debt (of credit limit) and military (of spending)".into(),
            unit: Unit::Share,
            lines: vec![line("Debt", |s| s.debt_share), line("Military", |s| s.military_share)],
        },
    ]
}

/// Largest-Triangle-Three-Buckets: `max` points that keep the line's shape.
pub fn lttb(points: &[(f64, f64)], max: usize) -> Vec<(f64, f64)> {
    if points.len() <= max || max < 3 {
        return points.to_vec();
    }
    let bucket = (points.len() - 2) as f64 / (max - 2) as f64;
    let mut out = Vec::with_capacity(max);
    out.push(points[0]);
    let mut a = 0;
    for i in 0..max - 2 {
        let start = (i as f64 * bucket) as usize + 1;
        let end = (((i + 1) as f64 * bucket) as usize + 1).min(points.len() - 1);
        let next_end = (((i + 2) as f64 * bucket) as usize + 1).min(points.len());
        let next = &points[end..next_end.max(end + 1)];
        let (nx, ny) = next.iter().fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
        let (nx, ny) = (nx / next.len() as f64, ny / next.len() as f64);
        let (ax, ay) = points[a];
        let best = (start..end.max(start + 1))
            .max_by(|&p, &q| {
                let area = |k: usize| ((ax - nx) * (points[k].1 - ay) - (ax - points[k].0) * (ny - ay)).abs();
                area(p).total_cmp(&area(q))
            })
            .unwrap_or(start);
        out.push(points[best]);
        a = best;
    }
    out.push(points[points.len() - 1]);
    out
}

/// Axis label for a value.
pub fn format_value(unit: Unit, v: f64) -> String {
    let a = v.abs();
    match unit {
        Unit::Share => format!("{:.0}%", v * 100.0),
        _ if a >= 1e9 => format!("{:.1}B", v / 1e9),
        _ if a >= 1e6 => format!("{:.1}M", v / 1e6),
        _ if a >= 1e3 => format!("{:.1}K", v / 1e3),
        Unit::Plain if a < 100.0 => format!("{v:.1}"),
        _ => format!("{v:.0}"),
    }
}

/// SVG for `chart`, `width`×`height` px.
pub fn render_svg(chart: &Chart, width: u32, height: u32) -> String {
    let lines: Vec<Line> = chart.lines.iter().map(|l| Line { label: l.label.clone(), points: lttb(&l.points, MAX_POINTS) }).collect();
    let all = lines.iter().flat_map(|l| l.points.iter());
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &(x, y) in all {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    if x0 > x1 {
        return String::new();
    }
    // Years on the x axis; pad flat or single-point ranges.
    let (x0, x1) = (year_of(x0), year_of(x1).max(year_of(x0) + 0.1));
    let pad = ((y1 - y0) * 0.08).max(y1.abs().max(1.0) * 0.01);
    let (y0, y1) = (if y0 >= 0.0 { (y0 - pad).max(0.0) } else { y0 - pad }, y1 + pad);

    let mut svg = String::new();
    {
        let root = SVGBackend::with_string(&mut svg, (width, height)).into_drawing_area();
        let built = ChartBuilder::on(&root)
            .margin(8)
            .x_label_area_size(22)
            .y_label_area_size(52)
            .build_cartesian_2d(x0..x1, y0..y1);
        let Ok(mut ctx) = built else { return String::new() };
        let unit = chart.unit;
        let span = x1 - x0;
        let _ = ctx
            .configure_mesh()
            .light_line_style(RGBColor(232, 236, 240))
            .bold_line_style(RGBColor(214, 221, 228))
            .axis_style(RGBColor(160, 170, 180))
            .x_labels(6)
            .y_labels(5)
            .x_label_formatter(&|x| if span < 3.0 { format!("{x:.1}") } else { format!("{x:.0}") })
            .y_label_formatter(&|y| format_value(unit, *y))
            .label_style(("sans-serif", 12).into_font().color(&RGBColor(90, 102, 117)))
            .draw();
        if y0 < 0.0 && y1 > 0.0 {
            let _ = ctx.draw_series(std::iter::once(PathElement::new(vec![(x0, 0.0), (x1, 0.0)], RGBColor(160, 170, 180))));
        }
        for (i, line) in lines.iter().enumerate() {
            let color = PALETTE[i % PALETTE.len()];
            let points = line.points.iter().map(|&(x, y)| (year_of(x), y));
            if lines.len() == 1 {
                // A single series gets a soft fill under the line.
                let _ = ctx.draw_series(AreaSeries::new(points.clone(), y0.max(0.0).min(y1), color.mix(0.12)));
            }
            let _ = ctx.draw_series(LineSeries::new(points, color.stroke_width(2)));
        }
        let _ = root.present();
    }
    svg
}

/// Render `charts` with first/last values per line.
pub fn render_all(charts: &[Chart]) -> Vec<RenderedChart> {
    charts
        .iter()
        .map(|c| RenderedChart {
            key: c.key.clone(),
            title: c.title.clone(),
            unit: c.unit,
            svg: render_svg(c, 720, 240),
            lines: c
                .lines
                .iter()
                .map(|l| LineSummary { label: l.label.clone(), first: l.points.first().map(|p| p.1), last: l.points.last().map(|p| p.1) })
                .collect(),
        })
        .collect()
}

/// A few points per line for agents: `[year, value]` pairs, evenly thinned.
pub fn summarize(charts: &[Chart], points: usize) -> serde_json::Value {
    let charts: Vec<serde_json::Value> = charts
        .iter()
        .map(|c| {
            let lines: Vec<serde_json::Value> = c
                .lines
                .iter()
                .map(|l| {
                    let thin = lttb(&l.points, points);
                    let pts: Vec<[f64; 2]> = thin.iter().map(|&(x, y)| [(year_of(x) * 100.0).round() / 100.0, y]).collect();
                    serde_json::json!({ "label": l.label, "points": pts })
                })
                .collect();
            serde_json::json!({ "key": c.key, "title": c.title, "unit": c.unit, "lines": lines })
        })
        .collect();
    serde_json::json!({ "charts": charts })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trend(rate: f64, date: &str, values: Vec<f64>) -> TrendSeries {
        let text = format!(
            "sample_rate={rate} count=5214 channels={{ 0={{ date={date} index={} values={{ {} }} }} }}",
            values.len(),
            values.iter().map(f64::to_string).collect::<Vec<_>>().join(" ")
        );
        jomini::text::de::from_utf8_slice(text.as_bytes()).expect("trend")
    }

    #[test]
    fn weekly_trend_ends_on_its_date_and_renders() {
        // GDP is sampled every 7 days (sample_rate 28 quarter days).
        let country = CountryTrends { gdp: Some(trend(28.0, "1836.2.24", vec![1.0e6, 1.1e6, 1.2e6])), ..CountryTrends::default() };
        let charts = country_charts(&country);
        assert_eq!(charts.len(), 1);
        let points = &charts[0].lines[0].points;
        assert_eq!(points.last().unwrap().0, f64::from(day_number(1836, 2, 24)));
        assert_eq!(points[1].0 - points[0].0, 7.0);
        let svg = render_svg(&charts[0], 720, 240);
        assert!(svg.starts_with("<svg") && svg.contains("<polyline"), "{}", &svg[..svg.len().min(200)]);
        assert!(svg.contains("1.1M") || svg.contains("1.2M"), "money axis labels");
    }

    /// Writes `target/tmp/charts.html` with a century of synthetic data to eyeball the look.
    #[test]
    #[ignore = "writes a preview file"]
    fn preview_charts() {
        let wave = |base: f64, growth: f64| -> Vec<f64> {
            (0..5000).map(|i| base * (1.0 + growth).powf(f64::from(i) / 52.0) * (1.0 + 0.05 * (f64::from(i) / 40.0).sin())).collect()
        };
        let trends = CountryTrends {
            gdp: Some(trend(28.0, "1935.12.1", wave(9.7e6, 0.03))),
            literacy: Some(trend(28.0, "1935.12.1", (0..5000).map(|i| 0.46 + 0.5 * f64::from(i) / 5000.0).collect())),
            ..CountryTrends::default()
        };
        let mut charts = country_charts(&trends);
        charts.push(Chart {
            key: "budget".into(),
            title: "Weekly income and balance".into(),
            unit: Unit::Money,
            lines: vec![
                Line { label: "Income".into(), points: (0..400).map(|i| (f64::from(day_number(1836, 1, 1) + i * 7), 57_000.0 + 80.0 * f64::from(i))).collect() },
                Line { label: "Balance".into(), points: (0..400).map(|i| (f64::from(day_number(1836, 1, 1) + i * 7), -7_000.0 + 30.0 * f64::from(i))).collect() },
            ],
        });
        let html: String = render_all(&charts).iter().map(|c| format!("<h3>{}</h3><div style='max-width:720px'>{}</div>", c.title, c.svg)).collect();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/charts.html");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(out, format!("<body style='font-family:sans-serif'>{html}</body>")).unwrap();
    }

    #[test]
    fn lttb_keeps_ends_and_peaks() {
        let points: Vec<(f64, f64)> = (0..10_000).map(|i| (f64::from(i), if i == 5_000 { 100.0 } else { 0.0 })).collect();
        let thin = lttb(&points, 300);
        assert_eq!(thin.len(), 300);
        assert_eq!(thin[0], points[0]);
        assert_eq!(*thin.last().unwrap(), points[9_999]);
        assert!(thin.iter().any(|p| p.1 == 100.0), "the spike survives");
    }

    #[test]
    fn budget_charts_need_two_dated_lines() {
        let line = crate::live::parse_line(
            "[1:2:3][x]: OIKO1;PRU;1 marca 1836;-7231;57845;486803;0;618;67;9720602",
        )
        .unwrap();
        let mut later = line.clone();
        later.date = "8 marca 1836".into();
        later.treasury = 479_572.0;
        assert!(budget_charts(std::slice::from_ref(&line), "PRU").is_empty());
        let charts = budget_charts(&[later, line], "PRU");
        assert_eq!(charts.len(), 3);
        assert_eq!(charts[1].lines[0].points.iter().map(|p| p.1).collect::<Vec<_>>(), [486_803.0, 479_572.0]);
    }
}
