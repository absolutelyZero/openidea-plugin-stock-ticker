//! Stock Ticker — A-share quotes in a WebView panel.
//! Plugin API 1.3 (`engines.openidea = "^1.3"`).
//!
//! The panel is a WebView page (`[[contributes.views]] web = "web/index.html"`).
//! The page owns the refresh timer (`setInterval`, 3–300 s, default 10) and
//! talks to the plugin over `openidea.postMessage`; the plugin fetches quotes,
//! formats rows, and pushes a snapshot back with `host::post_view_message`.
//! The host only delivers `on-schedule` to inactive plugins, so a page timer is
//! the only way to refresh while the panel is visible.

mod model;
mod quote;
mod search;
mod state;
mod sync;

use model::{RenderOptions, Row, WatchItem};
use openidea_plugin::{
    Context, Error, Invocation, PickItem, Plugin, StatusItem, host, register_plugin,
    views::{TableSort, View, ViewEvent, Viewport, Views},
};
use state::State;

const VIEW_WATCHLIST: &str = "watchlist";
const DEFAULT_INTERVAL_SECS: u32 = 10;
/// Storage key for the page's refresh interval (the WebView has no storage).
const INTERVAL_KEY: &str = "web.intervalSec";

pub struct StockPlugin {
    state: State,
}

impl Default for StockPlugin {
    fn default() -> Self {
        Self { state: State::default() }
    }
}

pub(crate) fn render_options(state: &State) -> RenderOptions {
    RenderOptions {
        precision: state.config.precision,
        sort: state.sort,
        arrow_style: state.config.arrow_style,
        order_mode: state.order_mode,
    }
}

/// The watchlist rows in their current order.
pub(crate) fn current_rows(state: &State) -> Vec<Row> {
    model::ordered_rows(&state.watch, state.snapshot.as_ref(), render_options(state))
}

/// `(symbols, rows)` in display order: the sorted watchlist. The two vectors
/// share an index so a page row maps to its symbol for double-click / removal.
pub(crate) fn table_rows(state: &State) -> (Vec<String>, Vec<Vec<String>>) {
    current_rows(state)
        .into_iter()
        .map(|r| (r.symbol, r.cells))
        .unzip()
}

/// The browser URL for a symbol's quote page.
fn quote_url(symbol: &str) -> String {
    let (market, code) = symbol.split_once('.').unwrap_or(("1", symbol));
    let prefix = if market == "1" { "sh" } else { "sz" };
    format!("https://quote.eastmoney.com/{prefix}{code}.html")
}

fn interval_sec() -> u32 {
    host::storage::get(INTERVAL_KEY)
        .ok()
        .flatten()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(DEFAULT_INTERVAL_SECS)
        .clamp(3, 300)
}

/// Beijing-time clock for a quote timestamp.
fn clock_of(ts: i64) -> String {
    let secs = (ts + 8 * 3_600).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", secs / 3_600, (secs % 3_600) / 60, secs % 60)
}

/// Keep the status bar item in sync with the latest snapshot.
fn set_index_status(state: &State) {
    let text = if state.watch.is_empty() {
        "行情 —".to_string()
    } else {
        format!("{} 只", state.watch.len())
    };
    host::set_status_item(
        "index",
        &StatusItem {
            text,
            tooltip: Some(format!("点击刷新 · {}", state.status_text())),
            command: Some("refresh".into()),
        },
    );
}

/// One message shape for the page: formatted rows plus everything the header,
/// footer and timer need. The page only renders; it never fetches.
fn snapshot_json(state: &State) -> String {
    let (symbols, rows) = table_rows(state);
    let rows: Vec<serde_json::Value> = rows
        .iter()
        .zip(symbols.iter())
        .map(|(cells, symbol)| serde_json::json!({ "symbol": symbol, "cells": cells }))
        .collect();
    let (clock, age) = match &state.snapshot {
        Some(s) => {
            let clock = s
                .quotes
                .iter()
                .filter_map(|q| q.ts)
                .max()
                .map(clock_of)
                .unwrap_or_default();
            (clock, state::now_secs().saturating_sub(s.fetched_at))
        }
        None => (String::new(), -1),
    };
    serde_json::json!({
        "type": "snapshot",
        "columns": model::COLUMNS,
        "rows": rows,
        "config": {
            "intervalSec": interval_sec(),
            "orderMode": match state.order_mode {
                model::OrderMode::Custom => "custom",
                model::OrderMode::Column => "column",
            },
        },
        "source": state.snapshot.as_ref().map(|s| s.source.clone()).unwrap_or_default(),
        "clock": clock,
        "ageSec": age,
        "error": state.meta.last_error,
        "status": state.status_text(),
    })
    .to_string()
}

/// Push a snapshot to the page and refresh the status bar.
pub(crate) fn publish(state: &State) {
    let _ = host::post_view_message(VIEW_WATCHLIST, &snapshot_json(state));
    set_index_status(state);
}

/// Add an already-resolved secid to the watchlist, persist it, then fetch fresh
/// quotes.
fn add_symbol(state: &mut State, symbol: &str) -> Result<(), Error> {
    if state.watch.len() >= model::MAX_WATCH {
        return Err(Error::Invalid("自选股已达 200 条上限".into()));
    }
    if !state.watch.iter().any(|w| w.symbol == symbol) {
        state.watch.push(WatchItem { symbol: symbol.to_string(), ..Default::default() });
        let _ = state.save_watch();
    }
    sync::sync(state, true)?;
    Ok(())
}

/// Reorder `watch` to follow `order` (symbols in their new sequence). Symbols
/// not present in `order` keep their relative order and are appended; duplicate
/// or unknown symbols in `order` are ignored. Never produces duplicate rows.
fn reorder_watch(watch: &[WatchItem], order: &[String]) -> Vec<WatchItem> {
    use std::collections::HashSet;
    let mut seen: HashSet<&str> = HashSet::new();
    let mut result: Vec<WatchItem> = Vec::with_capacity(watch.len());
    for symbol in order {
        if !seen.insert(symbol.as_str()) {
            // skip duplicate entries (unknown symbols simply match nothing)
            continue;
        }
        if let Some(item) = watch.iter().find(|w| w.symbol == *symbol) {
            result.push(item.clone());
        }
    }
    for item in watch {
        if !seen.contains(item.symbol.as_str()) {
            result.push(item.clone());
        }
    }
    result
}

fn pick_symbol(state: &State, arg: Option<&str>, title: &str) -> Result<Option<String>, Error> {
    if let Some(a) = arg {
        let a = a.trim();
        if !a.is_empty() {
            return Ok(Some(a.to_string()));
        }
    }
    let items: Vec<PickItem> = state
        .watch
        .iter()
        .map(|w| PickItem::new(&w.symbol, w.alias.as_deref().unwrap_or(&w.symbol)))
        .collect();
    Ok(host::pick(title, &items)?)
}

impl Plugin for StockPlugin {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        self.state = State::load();
        publish(&self.state);
        Ok(())
    }

    fn run_command(
        &mut self,
        id: &str,
        _ctx: &Invocation,
        arg: Option<&str>,
    ) -> Result<Option<String>, Error> {
        self.state.refresh_config();
        match id {
            "refresh" => {
                sync::sync(&mut self.state, true)?;
                publish(&self.state);
                Ok(Some(self.state.status_text()))
            }
            "test-connection" => {
                let before = self.state.meta.failures;
                sync::sync(&mut self.state, true)?;
                if self.state.meta.failures > before {
                    Err(Error::Invalid(
                        self.state
                            .meta
                            .last_error
                            .clone()
                            .unwrap_or_else(|| "行情源不可达".into()),
                    ))
                } else {
                    Ok(Some("行情源连接正常".into()))
                }
            }
            "reveal" => {
                host::reveal_view(VIEW_WATCHLIST)?;
                Ok(None)
            }
            "add" => {
                let Some(text) = host::input("股票代码 / 中文名 / 拼音首字母", None)? else {
                    return Ok(None);
                };
                let keyword = text.trim();
                if keyword.is_empty() {
                    return Ok(None);
                }
                if let Ok(symbol) = model::normalize(keyword) {
                    add_symbol(&mut self.state, &symbol)?;
                    publish(&self.state);
                    return Ok(Some(format!("已添加 {symbol}")));
                }
                let found = search::search_symbols(keyword)?;
                match found.len() {
                    0 => Err(Error::Invalid(format!("未找到匹配 “{keyword}” 的股票"))),
                    1 => {
                        add_symbol(&mut self.state, &found[0].0)?;
                        publish(&self.state);
                        Ok(Some(format!("已添加 {}", found[0].0)))
                    }
                    _ => {
                        let items: Vec<PickItem> = found
                            .iter()
                            .map(|(s, n)| PickItem::new(s.as_str(), n.as_str()))
                            .collect();
                        match host::pick("选择股票", &items)? {
                            Some(s) => {
                                add_symbol(&mut self.state, &s)?;
                                publish(&self.state);
                                Ok(Some(format!("已添加 {s}")))
                            }
                            None => Ok(None),
                        }
                    }
                }
            }
            "open" => match pick_symbol(&self.state, arg, "打开行情")? {
                Some(s) => {
                    host::open_url(&quote_url(&s))?;
                    Ok(Some(format!("已打开 {s}")))
                }
                None => Ok(None),
            },
            "remove" => {
                let symbol = match pick_symbol(&self.state, arg, "移除自选股")? {
                    Some(s) => s,
                    None => return Ok(None),
                };
                let name = self
                    .state
                    .watch
                    .iter()
                    .find(|w| w.symbol == symbol)
                    .and_then(|w| w.alias.clone())
                    .unwrap_or_else(|| symbol.clone());
                if !host::confirm(
                    "移除自选股",
                    &format!("确定移除 {}（{}）？", name, model::code_of(&symbol)),
                )? {
                    return Ok(None);
                }
                self.state.watch.retain(|w| w.symbol != symbol);
                let _ = self.state.save_watch();
                publish(&self.state);
                Ok(Some(format!("已移除 {symbol}")))
            }
            _ => Err(format!("unknown command {id}").into()),
        }
    }
}

impl Views for StockPlugin {
    fn render(
        &mut self,
        _view: &str,
        _viewport: Option<Viewport>,
        _ctx: &Invocation,
    ) -> Result<View, Error> {
        // The page draws the panel; this body is only shown while it loads.
        Ok(View::message("正在加载行情…"))
    }

    fn handle(&mut self, _view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        let ViewEvent::Message(text) = event else { return Ok(()) };
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Ok(());
        };
        let kind = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match kind {
            "hello" | "refresh" => {
                self.state.refresh_config();
                let _ = sync::sync(&mut self.state, kind == "refresh");
                publish(&self.state);
            }
            "sort" => {
                let column = msg.get("column").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let descending = msg
                    .get("descending")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.state.sort = TableSort { column, descending };
                // Column sorting is a transient view: it never changes the
                // persisted watchlist order, only the display mode.
                self.state.order_mode = model::OrderMode::Column;
                let _ = self.state.save_sort();
                let _ = self.state.save_order_mode();
                publish(&self.state);
            }
            "reorder" => {
                let order: Vec<String> = msg
                    .get("order")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                if !order.is_empty() {
                    self.state.watch = reorder_watch(&self.state.watch, &order);
                    let _ = self.state.save_watch();
                    self.state.order_mode = model::OrderMode::Custom;
                    let _ = self.state.save_order_mode();
                }
                publish(&self.state);
            }
            "open" => {
                if let Some(symbol) = msg.get("symbol").and_then(|v| v.as_str()) {
                    let _ = host::open_url(&quote_url(symbol));
                }
            }
            "remove" => {
                let Some(symbol) = msg.get("symbol").and_then(|v| v.as_str()) else {
                    return Ok(());
                };
                let name = self
                    .state
                    .watch
                    .iter()
                    .find(|w| w.symbol == symbol)
                    .and_then(|w| w.alias.clone())
                    .unwrap_or_else(|| symbol.to_string());
                if host::confirm(
                    "移除自选股",
                    &format!("确定移除 {}（{}）？", name, model::code_of(symbol)),
                )? {
                    self.state.watch.retain(|w| w.symbol != symbol);
                    let _ = self.state.save_watch();
                }
                publish(&self.state);
            }
            "add" => {
                let _ = self.run_command("add", _ctx, None);
                publish(&self.state);
            }
            "prefs" => {
                if let Some(n) = msg.get("intervalSec").and_then(|v| v.as_u64()) {
                    let v = (n as u32).clamp(3, 300);
                    let _ = host::storage::set(INTERVAL_KEY, &v.to_string());
                }
                publish(&self.state);
            }
            _ => {}
        }
        Ok(())
    }
}

register_plugin!(StockPlugin, views);

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::GBK;
    use openidea_plugin::{HttpResponse, mock::MockHost};

    fn gbk(s: &str) -> Vec<u8> {
        let (bytes, _, _) = GBK.encode(s);
        bytes.into_owned()
    }

    fn good() -> Vec<u8> {
        gbk(concat!(
            "v_sh600519=\"1~贵州茅台~600519~1438.55~1435.26~1426.30~18370~10067~8302~",
            "1438.55~1~1438.54~1~1438.45~1~1438.44~1~1438.34~1~1439.00~3~1439.04~1~",
            "1439.20~1~1439.50~3~1439.55~1~~20261116113253~3.29~0.23~1445.60~1425.00~",
            "1438.55/18370/2640163153~18370~264016~0.15~21.89\";",
            "v_sh000001=\"1~上证指数~000001~3620.00~3634.35~3611.54~277036290~0~0~",
            "0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~~",
            "20261116113200~-14.35~-0.39~3623.12~3593.45~3620.00/277036290/421756737368~",
            "277036290~42175674~0.57~16.50\";"
        ))
    }

    fn mock() -> MockHost {
        MockHost::install().with_capabilities(&[
            "storage",
            "net:qt.gtimg.cn",
            "net:proxy.finance.qq.com",
        ])
    }

    fn plugin() -> (MockHost, StockPlugin) {
        let h = mock();
        let mut p = StockPlugin::default();
        p.activate(&h.context("absolutelyzero.stock-ticker")).unwrap();
        (h, p)
    }

    fn message(p: &mut StockPlugin, h: &MockHost, json: &str) {
        p.handle(VIEW_WATCHLIST, ViewEvent::Message(json.into()), &h.invocation())
            .unwrap();
    }

    fn last_snapshot(h: &MockHost) -> String {
        h.view_messages()
            .last()
            .map(|(_, m)| m.clone())
            .unwrap_or_default()
    }

    #[test]
    fn activation_publishes_a_snapshot_and_sets_the_status_item() {
        let (h, _p) = plugin();
        assert!(h.view_messages().iter().any(|(v, m)| v == VIEW_WATCHLIST && m.contains("\"type\":\"snapshot\"")));
        assert!(h.status_items().contains_key("index"));
    }

    #[test]
    fn refresh_message_fetches_and_pushes_rows() {
        let (h, mut p) = plugin();
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: good() }));
        p.state.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        message(&mut p, &h, r#"{"type":"refresh"}"#);
        assert!(!h.requests().is_empty());
        let snap = last_snapshot(&h);
        assert!(snap.contains("贵州茅台"), "snapshot: {snap}");
        assert!(snap.contains("1438.55"));
    }

    #[test]
    fn sort_message_updates_state_and_persists() {
        let (h, mut p) = plugin();
        message(&mut p, &h, r#"{"type":"sort","column":3,"descending":true}"#);
        assert_eq!(p.state.sort, TableSort { column: 3, descending: true });
        assert!(h.storage().contains_key("sort.v1"));
    }

    #[test]
    fn reorder_message_reorders_watch_and_persists_in_custom_mode() {
        let (h, mut p) = plugin();
        for s in ["1.600519", "0.000001", "0.300750"] {
            p.state.watch.push(WatchItem { symbol: s.into(), ..Default::default() });
        }
        message(
            &mut p,
            &h,
            r#"{"type":"reorder","order":["0.300750","1.600519","0.000001"]}"#,
        );
        let got: Vec<&str> = p.state.watch.iter().map(|w| w.symbol.as_str()).collect();
        assert_eq!(got, ["0.300750", "1.600519", "0.000001"]);
        assert_eq!(p.state.order_mode, model::OrderMode::Custom);
        assert_eq!(h.storage()["order-mode.v1"], "\"custom\"");
        let stored: Vec<serde_json::Value> =
            serde_json::from_str(&h.storage()["watch.v1"]).unwrap();
        let stored_symbols: Vec<&str> =
            stored.iter().map(|v| v["symbol"].as_str().unwrap()).collect();
        assert_eq!(stored_symbols, ["0.300750", "1.600519", "0.000001"]);
    }

    #[test]
    fn reorder_ignores_duplicates_unknown_and_appends_missing() {
        let (h, mut p) = plugin();
        for s in ["1.600519", "0.000001", "0.300750"] {
            p.state.watch.push(WatchItem { symbol: s.into(), ..Default::default() });
        }
        // order contains a duplicate and an unknown symbol, and omits 0.000001
        message(
            &mut p,
            &h,
            r#"{"type":"reorder","order":["0.300750","0.300750","9.999999","1.600519"]}"#,
        );
        let got: Vec<&str> = p.state.watch.iter().map(|w| w.symbol.as_str()).collect();
        // 0.300750 then 1.600519; the omitted 0.000001 is appended last; no duplicates
        assert_eq!(got, ["0.300750", "1.600519", "0.000001"]);
    }

    #[test]
    fn sort_message_switches_to_column_mode_without_changing_watch_order() {
        let (h, mut p) = plugin();
        for s in ["0.300750", "1.600519"] {
            p.state.watch.push(WatchItem { symbol: s.into(), ..Default::default() });
        }
        p.state.order_mode = model::OrderMode::Custom;
        let _ = p.state.save_order_mode();
        message(&mut p, &h, r#"{"type":"sort","column":2,"descending":true}"#);
        // underlying order untouched
        let got: Vec<&str> = p.state.watch.iter().map(|w| w.symbol.as_str()).collect();
        assert_eq!(got, ["0.300750", "1.600519"]);
        assert_eq!(p.state.order_mode, model::OrderMode::Column);
        assert_eq!(h.storage()["order-mode.v1"], "\"column\"");
    }

    #[test]
    fn remove_message_confirms_then_deletes() {
        let (h, mut p) = plugin();
        p.state.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        h.answer_next_prompt(Some(&["yes"]));
        message(&mut p, &h, r#"{"type":"remove","symbol":"1.600519"}"#);
        assert!(p.state.watch.is_empty());
        assert!(!h.storage()["watch.v1"].contains("1.600519"));
    }

    #[test]
    fn prefs_message_clamps_the_interval() {
        let (h, mut p) = plugin();
        message(&mut p, &h, r#"{"type":"prefs","intervalSec":9999}"#);
        assert_eq!(h.storage()["web.intervalSec"], "300");
        message(&mut p, &h, r#"{"type":"prefs","intervalSec":1}"#);
        assert_eq!(h.storage()["web.intervalSec"], "3");
    }

    #[test]
    fn hello_pushes_config_with_the_interval() {
        let (h, mut p) = plugin();
        message(&mut p, &h, r#"{"type":"hello"}"#);
        let snap = last_snapshot(&h);
        assert!(snap.contains("\"intervalSec\":10"), "snapshot: {snap}");
    }

    #[test]
    fn snapshot_carries_the_current_order_mode() {
        let (h, mut p) = plugin();
        message(&mut p, &h, r#"{"type":"hello"}"#);
        assert!(last_snapshot(&h).contains("\"orderMode\":\"column\""));
        message(
            &mut p,
            &h,
            r#"{"type":"reorder","order":["1.600519"]}"#,
        );
        assert!(last_snapshot(&h).contains("\"orderMode\":\"custom\""));
    }

    #[test]
    fn add_by_code_message_adds_without_search() {
        let (h, mut p) = plugin();
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: good() }));
        h.answer_next_prompt(Some(&["600519"]));
        message(&mut p, &h, r#"{"type":"add"}"#);
        assert!(p.state.watch.iter().any(|w| w.symbol == "1.600519"));
    }
}
