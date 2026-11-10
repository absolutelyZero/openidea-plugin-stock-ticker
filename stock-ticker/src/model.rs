//! Domain model and pure presentation helpers. No host calls here, so every
//! function can be unit-tested on the native target.

use openidea_plugin::views::TableSort;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const COLUMNS: [&str; 4] = ["代码", "名称", "现价", "涨跌%"];
pub const MAX_WATCH: usize = 200;

/// Indices requested alongside the watchlist. Explicit secids avoid the
/// ambiguity of `000001` (SSE composite vs. SZSE Ping An Bank).
pub const INDEX_SECIDS: [&str; 3] = ["1.000001", "0.399001", "0.399006"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WatchItem {
    pub symbol: String,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub qty: Option<f64>,
    #[serde(default)]
    pub above: Option<f64>,
    #[serde(default)]
    pub below: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub price: Option<f64>,
    pub prev_close: Option<f64>,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub change: Option<f64>,
    pub change_pct: Option<f64>,
    pub volume: Option<f64>,
    pub turnover: Option<f64>,
    pub ts: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub fetched_at: i64,
    pub source: String,
    pub quotes: Vec<Quote>,
}

/// One rendered row for the native `sortable_table`. Plain text only — no
/// per-cell color (the IDE owns cell styling). `symbol` maps a table row back
/// to a watch item so clicks/activations can open the right quote page.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Row {
    pub symbol: String,
    pub cells: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    pub precision: usize,
    pub sort: TableSort,
    pub arrow_style: bool,
}

/// Accept `1.600519`, `sh600519`, `600519`. Market is inferred from the first
/// digit when omitted; index secids must be spelled out (see INDEX_SECIDS).
pub fn normalize(input: &str) -> Result<String, String> {
    let s = input.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err("股票代码不能为空".into());
    }
    if let Some((market, code)) = s.split_once('.') {
        if (market == "1" || market == "0")
            && code.len() == 6
            && code.bytes().all(|b| b.is_ascii_digit())
        {
            return Ok(format!("{market}.{code}"));
        }
    }
    let (market, code) = match (s.strip_prefix("sh"), s.strip_prefix("sz")) {
        (Some(rest), _) => ("1", rest),
        (_, Some(rest)) => ("0", rest),
        _ => ("", s.as_str()),
    };
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("无法识别的股票代码：{input}（示例 600519 / sh600519）"));
    }
    let market = if !market.is_empty() {
        market
    } else if code.starts_with('6') {
        "1"
    } else if code.starts_with('0') || code.starts_with('3') {
        "0"
    } else {
        return Err(format!("无法推断市场：{input}，请用 sh/sz 前缀"));
    };
    Ok(format!("{market}.{code}"))
}

pub fn code_of(symbol: &str) -> &str {
    symbol.rsplit('.').next().unwrap_or(symbol)
}

fn fixed(value: Option<f64>, precision: usize) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{:.*}", precision, v),
        _ => "—".to_string(),
    }
}

fn signed(value: Option<f64>, precision: usize, suffix: &str, arrow_style: bool) -> String {
    match value {
        Some(v) if v.is_finite() => {
            let sign = if v > 0.0 { "+" } else { "" };
            let mark = if !arrow_style {
                ""
            } else if v > 0.0 {
                " ▲"
            } else if v < 0.0 {
                " ▼"
            } else {
                ""
            };
            format!("{sign}{:.*}{suffix}{mark}", precision, v)
        }
        _ => "—".to_string(),
    }
}

fn numeric(cell: &str) -> Option<f64> {
    let cleaned: String = cell
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .collect();
    cleaned.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Missing values always sort last, in both directions.
fn cmp_numeric(a: &str, b: &str, descending: bool) -> Ordering {
    match (numeric(a), numeric(b)) {
        (Some(x), Some(y)) => {
            let ord = x.partial_cmp(&y).unwrap_or(Ordering::Equal);
            if descending { ord.reverse() } else { ord }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Pure: watchlist + last snapshot -> sorted, formatted rows.
pub fn ordered_rows(
    watch: &[WatchItem],
    snapshot: Option<&Snapshot>,
    opts: RenderOptions,
) -> Vec<Row> {
    let quotes: std::collections::HashMap<&str, &Quote> = snapshot
        .map(|s| {
            s.quotes
                .iter()
                .map(|q| (q.symbol.as_str(), q))
                .collect()
        })
        .unwrap_or_default();

    let mut rows: Vec<Row> = watch
        .iter()
        .map(|item| {
            let q = quotes.get(item.symbol.as_str()).copied();
            let price = q.and_then(|x| x.price);
            let change_pct = q.and_then(|x| x.change_pct);
            let name = item
                .alias
                .clone()
                .or_else(|| q.map(|x| x.name.clone()).filter(|n| !n.is_empty()))
                .unwrap_or_else(|| item.symbol.clone());

            Row {
                symbol: item.symbol.clone(),
                cells: vec![
                    code_of(&item.symbol).to_string(),
                    name,
                    fixed(price, opts.precision),
                    signed(change_pct, opts.precision, "%", opts.arrow_style),
                ],
            }
        })
        .collect();

    let col = (opts.sort.column as usize).min(COLUMNS.len() - 1);
    let descending = opts.sort.descending;
    rows.sort_by(|a, b| {
        let ord = if col <= 1 {
            let o = a.cells[col].cmp(&b.cells[col]);
            if descending { o.reverse() } else { o }
        } else {
            cmp_numeric(&a.cells[col], &b.cells[col], descending)
        };
        ord.then_with(|| a.symbol.cmp(&b.symbol))
    });
    rows
}

pub fn index_rows(
    snapshot: Option<&Snapshot>,
    precision: usize,
    arrow_style: bool,
) -> Vec<Row> {
    let Some(s) = snapshot else { return Vec::new() };
    INDEX_SECIDS
        .iter()
        .filter_map(|sid| s.quotes.iter().find(|q| &q.symbol == sid))
        .map(|q| Row {
            symbol: q.symbol.clone(),
            cells: vec![
                code_of(&q.symbol).to_string(),
                q.name.clone(),
                fixed(q.price, precision),
                signed(q.change_pct, precision, "%", arrow_style),
            ],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sort(column: u32, descending: bool) -> TableSort {
        TableSort { column, descending }
    }

    #[test]
    fn normalize_accepts_common_forms() {
        assert_eq!(normalize("600519").unwrap(), "1.600519");
        assert_eq!(normalize("sh600519").unwrap(), "1.600519");
        assert_eq!(normalize(" 1.600519 ").unwrap(), "1.600519");
        assert_eq!(normalize("sz000001").unwrap(), "0.000001");
        assert_eq!(normalize("300750").unwrap(), "0.300750");
        assert!(normalize("").is_err());
        assert!(normalize("AAPL").is_err());
        assert!(normalize("2.600519").is_err());
    }

    fn quote(symbol: &str, price: f64, pct: f64) -> Quote {
        Quote {
            symbol: symbol.into(),
            name: "贵州茅台".into(),
            price: Some(price),
            prev_close: Some(1500.0),
            change: Some(price - 1500.0),
            change_pct: Some(pct),
            ..Quote::default()
        }
    }

    #[test]
    fn renders_four_columns_with_the_change_arrow() {
        let watch = vec![WatchItem { symbol: "1.600519".into(), ..WatchItem::default() }];
        let snap = Snapshot {
            quotes: vec![quote("1.600519", 1520.0, 1.33)],
            ..Snapshot::default()
        };
        let rows = ordered_rows(
            &watch,
            Some(&snap),
            RenderOptions { precision: 2, sort: sort(0, false), arrow_style: true },
        );
        assert_eq!(COLUMNS, ["代码", "名称", "现价", "涨跌%"]);
        assert_eq!(rows[0].cells.len(), 4);
        assert_eq!(rows[0].cells[2], "1520.00");
        assert_eq!(rows[0].cells[3], "+1.33% ▲");
    }

    #[test]
    fn falling_quotes_are_marked_down() {
        let watch = vec![WatchItem { symbol: "1.600519".into(), ..WatchItem::default() }];
        let snap = Snapshot {
            quotes: vec![quote("1.600519", 1480.0, -1.33)],
            ..Snapshot::default()
        };
        let rows = ordered_rows(
            &watch,
            Some(&snap),
            RenderOptions { precision: 2, sort: sort(0, false), arrow_style: true },
        );
        assert!(rows[0].cells[3].contains('▼'));
    }

    #[test]
    fn no_quote_renders_placeholders_without_panicking() {
        let watch = vec![WatchItem { symbol: "1.600519".into(), ..WatchItem::default() }];
        let rows = ordered_rows(
            &watch,
            None,
            RenderOptions { precision: 2, sort: sort(2, true), arrow_style: false },
        );
        assert_eq!(rows[0].cells[2], "—");
    }

    #[test]
    fn missing_values_sort_last_in_both_directions() {
        let watch: Vec<WatchItem> = ["1.600519", "0.000001", "0.300750"]
            .iter()
            .map(|s| WatchItem { symbol: (*s).into(), ..WatchItem::default() })
            .collect();
        let snap = Snapshot {
            quotes: vec![quote("1.600519", 100.0, 1.0), quote("0.300750", 50.0, -1.0)],
            ..Snapshot::default()
        };
        for desc in [false, true] {
            let rows = ordered_rows(
                &watch,
                Some(&snap),
                RenderOptions { precision: 2, sort: sort(2, desc), arrow_style: true },
            );
            assert_eq!(rows.last().unwrap().symbol, "0.000001");
        }
    }

    #[test]
    fn arrow_style_can_be_turned_off() {
        let watch = vec![WatchItem { symbol: "1.600519".into(), ..WatchItem::default() }];
        let snap = Snapshot { quotes: vec![quote("1.600519", 1520.0, 1.33)], ..Snapshot::default() };
        let rows = ordered_rows(
            &watch,
            Some(&snap),
            RenderOptions { precision: 2, sort: sort(0, false), arrow_style: false },
        );
        assert_eq!(rows[0].cells[3], "+1.33%");
    }
}
