//! Stock Ticker — A-share quotes in an OpenIDEA side tool window.
//! Plugin API 1.3 (`engines.openidea = "^1.3"`).
//!
//! A single tool window backed by a native [`Body::sortable_table`] (no sandboxed
//! HTML frame). Native views re-render through [`host::refresh_view`], which is
//! the reliable, documented path — the earlier HTML-frame approach failed to
//! reload in some IDE builds, so clicks and auto-refresh never reached the UI.
//! The toolbar provides refresh / add / open / remove; double-clicking a row
//! opens its quote page, and header clicks re-sort. A fuzzy-search add switches
//! the same view into an in-panel result list instead of opening a second
//! dialog — API 1.3 has no type-ahead input, so the keyword is committed first.
//! See DESIGN.md.

mod model;
mod quote;
mod search;
mod state;
mod sync;

use model::{RenderOptions, Row, WatchItem};
use openidea_plugin::{
    Context, Error, Event, EventKind, Invocation, LogLevel, PickItem, Plugin, StatusItem, host,
    register_plugin,
    views::{Body, Button, ListItem, Table, View, ViewEvent, Viewport, Views},
};
use state::State;

const VIEW_WATCHLIST: &str = "watchlist";
/// Extra refresh opportunity while the user is active. The IDE's shortest legal
/// `on-schedule` is 60 s, so editor/file events act as a throttled heartbeat.
const HEARTBEAT_SECS: i64 = 15;

pub struct StockPlugin {
    state: State,
    /// The table row the user last selected (absolute, including pinned index
    /// rows). Used by the toolbar "移除" button so deleting does not prompt for
    /// a second pick. `None` until the first selection.
    selected: Option<u32>,
    /// When `Some`, the single view shows these fuzzy-search matches as an
    /// in-panel list instead of the quote table. `activate` on a row adds it.
    search_results: Option<Vec<(String, String)>>,
    /// The keyword that produced `search_results`, shown in the status line.
    search_keyword: String,
    /// Wall-clock of the last *attempted* refresh (instance-local, not persisted).
    /// Records attempts rather than successes so a failing source cannot turn
    /// every IDE event into a retry storm.
    last_heartbeat_fetch: i64,
}

impl Default for StockPlugin {
    fn default() -> Self {
        Self {
            state: State::default(),
            selected: None,
            search_results: None,
            search_keyword: String::new(),
            last_heartbeat_fetch: 0,
        }
    }
}

pub(crate) fn render_options(state: &State) -> RenderOptions {
    RenderOptions {
        precision: state.config.precision,
        sort: state.sort,
        arrow_style: state.config.arrow_style,
    }
}

/// The watchlist rows in their current order.
pub(crate) fn current_rows(state: &State) -> Vec<Row> {
    model::ordered_rows(&state.watch, state.snapshot.as_ref(), render_options(state))
}

fn indices(state: &State) -> Vec<Row> {
    model::index_rows(
        state.snapshot.as_ref(),
        state.config.precision,
        state.config.arrow_style,
    )
}

/// `(symbols, rows)` in display order: pinned index rows (if enabled), then the
/// sorted watchlist. The two vectors share an index so a table row maps to its
/// symbol for double-click / activation.
pub(crate) fn table_rows(state: &State) -> (Vec<String>, Vec<Vec<String>>) {
    let mut symbols = Vec::new();
    let mut rows = Vec::new();
    if state.config.show_indices {
        for r in indices(state) {
            symbols.push(r.symbol);
            rows.push(r.cells);
        }
    }
    for r in current_rows(state) {
        symbols.push(r.symbol);
        rows.push(r.cells);
    }
    (symbols, rows)
}

/// The browser URL for a symbol's quote page.
fn quote_url(symbol: &str) -> String {
    let (market, code) = symbol.split_once('.').unwrap_or(("1", symbol));
    let prefix = if market == "0" { "sz" } else { "sh" };
    format!("https://quote.eastmoney.com/{prefix}{code}.html")
}

/// Re-render the view with the latest data. Native views refresh through a
/// host-side `refresh_view`; unlike the old HTML frame, this does not depend on
/// an in-frame message bridge that some IDE builds leave disconnected.
pub(crate) fn publish(state: &State) {
    let _ = host::refresh_view(VIEW_WATCHLIST);
    set_index_status(state);
}

/// Keep the status bar item in sync with the latest snapshot.
fn set_index_status(state: &State) {
    let text = indices(state)
        .first()
        .map(|r| format!("{} {}", r.cells[0], r.cells[2]))
        .unwrap_or_else(|| "行情 —".to_string());
    host::set_status_item(
        "index",
        &StatusItem {
            text,
            tooltip: Some(format!("点击刷新 · {}", state.status_text())),
            command: Some("refresh".into()),
        },
    );
}

/// Result mode shows only navigation actions — the quote actions would act on
/// table rows that are not currently visible.
/// All icon names are verified against the IDE's built-in icon set
/// (`src/ui/openidea` + icon usages in `v0.1.39` source).
fn toolbar(in_results: bool) -> Vec<Button> {
    if in_results {
        vec![
            Button::new("back", "返回").icon("arrow-left"),
            Button::new("add", "重新搜索").icon("search"),
        ]
    } else {
        vec![
            Button::new("refresh", "刷新").icon("refresh"),
            Button::new("add", "添加").icon("add"),
            Button::new("open", "打开").icon("external-link"),
            Button::new("remove", "移除").icon("trash"),
        ]
    }
}

fn with_toolbar(mut view: View, in_results: bool) -> View {
    view.toolbar = toolbar(in_results);
    view
}

/// Add an already-resolved secid to the watchlist, persist it, then fetch fresh
/// quotes. Shared by the direct-code `add` command and the result-list click.
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

impl StockPlugin {
    /// Leave the in-panel result list and return to the quote table.
    fn clear_results(&mut self) {
        self.search_results = None;
        self.search_keyword.clear();
    }

    /// Force a fetch and re-render. The attempt timestamp is set *before* the
    /// request so both success and throttled/failed paths share one throttle.
    fn refresh_now(&mut self) {
        self.state.refresh_config();
        self.last_heartbeat_fetch = state::now_secs();
        let _ = sync::sync(&mut self.state, true);
        publish(&self.state);
    }
}

impl Plugin for StockPlugin {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        self.state = State::load();
        if let Err(e) = host::subscribe(EventKind::Scheduled) {
            host::log(LogLevel::Warn, &format!("subscribe(Scheduled) failed: {e}"));
        }
        // Activity heartbeat: these events are declared in `activation`, so they
        // also re-activate the plugin after an idle reclaim.
        if let Err(e) = host::subscribe(EventKind::EditorFocused) {
            host::log(LogLevel::Warn, &format!("subscribe(EditorFocused) failed: {e}"));
        }
        if let Err(e) = host::subscribe(EventKind::FileSaved) {
            host::log(LogLevel::Warn, &format!("subscribe(FileSaved) failed: {e}"));
        }
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
                self.last_heartbeat_fetch = state::now_secs();
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
                // A full code resolves immediately. Names, pinyin initials and
                // partial codes are searched and shown as an in-panel result
                // list (the IDE has no type-ahead input, so the keyword must be
                // committed once before matches can be computed).
                if let Ok(symbol) = model::normalize(keyword) {
                    self.clear_results();
                    add_symbol(&mut self.state, &symbol)?;
                    return Ok(Some(format!("已添加 {symbol}")));
                }
                let found = search::search_symbols(keyword)?;
                if found.is_empty() {
                    return Err(Error::Invalid(format!("未找到匹配 “{keyword}” 的股票")));
                }
                let n = found.len();
                self.search_results = Some(found);
                self.search_keyword = keyword.to_string();
                publish(&self.state);
                Ok(Some(format!("找到 {n} 个匹配，请在面板中双击或回车添加")))
            }
            "open" => {
                let symbol = pick_symbol(&self.state, arg, "打开行情")?;
                match symbol {
                    Some(s) => {
                        host::open_url(&quote_url(&s))?;
                        Ok(Some(format!("已打开 {s}")))
                    }
                    None => Ok(None),
                }
            }
            "remove" => {
                // Act on the row the user just selected; only fall back to a picker
                // when nothing is selected (e.g. run from the command palette).
                let selected_symbol = self.selected.and_then(|row| {
                    let (symbols, _) = table_rows(&self.state);
                    symbols
                        .get(row as usize)
                        .filter(|s| self.state.watch.iter().any(|w| w.symbol == **s))
                        .cloned()
                });
                let symbol = match selected_symbol {
                    Some(s) => s,
                    None => match pick_symbol(&self.state, arg, "移除自选股")? {
                        Some(s) => s,
                        None => return Ok(None),
                    },
                };
                let name = self
                    .state
                    .watch
                    .iter()
                    .find(|w| w.symbol == symbol)
                    .and_then(|w| w.alias.clone())
                    .unwrap_or_else(|| symbol.clone());
                if !host::confirm("移除自选股", &format!("确定移除 {}（{}）？", name, model::code_of(&symbol)))? {
                    return Ok(None);
                }
                self.state.watch.retain(|w| w.symbol != symbol);
                self.selected = None;
                let _ = self.state.save_watch();
                publish(&self.state);
                Ok(Some(format!("已移除 {symbol}")))
            }
            _ => Err(format!("unknown command {id}").into()),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        // The 60 s schedule is the only real timer a 1.3 plugin has; it stays
        // the guaranteed background floor.
        if events.iter().any(|e| matches!(e, Event::Scheduled)) {
            host::log(LogLevel::Info, "stock-ticker: scheduled refresh tick");
            self.refresh_now();
            return;
        }
        // While the user is active, editor/file events give extra chances to
        // refresh, throttled to at most one fetch per HEARTBEAT_SECS. Skip while
        // the in-panel search result list is up so picking a stock is never
        // interrupted by a re-render.
        let heartbeat = events
            .iter()
            .any(|e| matches!(e, Event::EditorFocused(_) | Event::FileSaved(_)));
        if !heartbeat || self.search_results.is_some() {
            return;
        }
        if state::now_secs().saturating_sub(self.last_heartbeat_fetch) >= HEARTBEAT_SECS {
            self.refresh_now();
        }
    }
}

fn pick_symbol(state: &State, arg: Option<&str>, title: &str) -> Result<Option<String>, Error> {
    if let Some(a) = arg {
        let a = a.trim();
        if !a.is_empty() {
            // Accept either a bare symbol or a table row index.
            if let Ok(row) = a.parse::<usize>() {
                let (symbols, _) = table_rows(state);
                if let Some(s) = symbols.get(row) {
                    return Ok(Some(s.clone()));
                }
            } else {
                return Ok(Some(a.to_string()));
            }
        }
    }
    let items: Vec<PickItem> = state
        .watch
        .iter()
        .map(|w| PickItem::new(&w.symbol, w.alias.as_deref().unwrap_or(&w.symbol)))
        .collect();
    Ok(host::pick(title, &items)?)
}

impl Views for StockPlugin {
    fn render(
        &mut self,
        view: &str,
        _viewport: Option<Viewport>,
        _ctx: &Invocation,
    ) -> Result<View, Error> {
        self.state.refresh_config();
        let in_results = self.search_results.is_some();
        let body = match view {
            VIEW_WATCHLIST if in_results => {
                let items: Vec<ListItem> = self
                    .search_results
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|(symbol, name)| {
                        let (market, code) = symbol.split_once('.').unwrap_or(("?", symbol));
                        let board = match market { "1" => "沪", "0" => "深", m => m };
                        ListItem::new(symbol.as_str(), &format!("{name} · {code}"))
                            .detail(&format!("{board} · 双击或回车添加"))
                            .icon("add")
                    })
                    .collect();
                Body::List(items)
            }
            VIEW_WATCHLIST => {
                let (_, rows) = table_rows(&self.state);
                let table = Table {
                    columns: model::COLUMNS.iter().map(|c| c.to_string()).collect(),
                    total_rows: rows.len() as u32,
                    first_row: 0,
                    rows,
                    editable: false,
                };
                Body::sortable_table(&table, Some(self.state.sort))
            }
            other => Body::Message(format!("unknown view {other}")),
        };
        let status = if in_results {
            format!(
                "“{}” 匹配 {} 只 · 双击/回车添加，或点「返回」",
                self.search_keyword,
                self.search_results.as_ref().map(Vec::len).unwrap_or(0)
            )
        } else {
            self.state.status_text()
        };
        Ok(with_toolbar(View::new(body).status(&status), in_results))
    }

    fn handle(&mut self, _view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        self.state.refresh_config();
        if let ViewEvent::Button(id) = &event {
            if id == "back" {
                self.clear_results();
                publish(&self.state);
                return Ok(());
            }
            return self.run_command(id, _ctx, None).map(|_| ());
        }
        // In result mode, double-click / Enter on a list row adds that symbol.
        if let ViewEvent::Activate(key) = &event {
            if self.search_results.is_some() {
                let symbol = key.clone();
                self.clear_results();
                add_symbol(&mut self.state, &symbol)?;
                publish(&self.state);
                return Ok(());
            }
        }
        // A single click (`select`) is deliberately ignored in result mode: the
        // IDE also sends `select` while moving with arrow keys, which would add
        // stocks the user was only browsing past.
        if self.search_results.is_some() && matches!(&event, ViewEvent::Select(_)) {
            return Ok(());
        }
        // Remember the chosen row so the toolbar "移除" can act on it directly.
        if let Some(row) = event.table_select() {
            self.selected = Some(row);
            return Ok(());
        }
        // Header click: re-sort and re-render.
        if let Some(sort) = event.sort() {
            self.state.sort = sort;
            let _ = self.state.save_sort();
            publish(&self.state);
            return Ok(());
        }
        // Double-click / Enter on a row: open its quote page.
        if let Some(row) = event.table_activate() {
            let (symbols, _) = table_rows(&self.state);
            if let Some(symbol) = symbols.get(row as usize) {
                let _ = host::open_url(&quote_url(symbol));
            }
            return Ok(());
        }
        // `table-select` is handled above; every other event (Cell / InsertRow /
        // DeleteRows / Message) is irrelevant to a read-only quote list and is
        // safely ignored.
        Ok(())
    }
}

register_plugin!(StockPlugin, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::{HttpResponse, mock::MockHost, views::TableSort};

    const GOOD: &str = r#"{"data":{"diff":[
      {"f12":"600519","f13":1,"f14":"贵州茅台","f2":1520.0,"f3":1.33,"f4":20.0,"f18":1500.0},
      {"f12":"000001","f13":1,"f14":"上证指数","f2":3100.5,"f3":0.82,"f18":3075.0}
    ]}}"#;

    fn mock() -> MockHost {
        MockHost::install().with_capabilities(&[
            "storage",
            "events",
            "net:push2.eastmoney.com",
            "net:push2delay.eastmoney.com",
            "net:searchapi.eastmoney.com",
        ])
    }

    fn plugin_with_data() -> (MockHost, StockPlugin) {
        let h = mock();
        h.on_fetch(|_| {
            Ok(HttpResponse { status: 200, headers: vec![], body: GOOD.as_bytes().to_vec() })
        });
        let mut p = StockPlugin::default();
        p.activate(&h.context("acme.stock-ticker")).unwrap();
        h.answer_next_prompt(Some(&["600519"]));
        p.run_command("add", &h.invocation(), None).unwrap();
        (h, p)
    }

    #[test]
    fn activation_subscribes_and_sets_status_item() {
        let h = mock();
        let mut p = StockPlugin::default();
        p.activate(&h.context("acme.stock-ticker")).unwrap();
        assert!(h.subscriptions().contains(&EventKind::Scheduled));
        assert!(h.status_items().contains_key("index"));
    }

    #[test]
    fn refresh_populates_the_native_table_and_toolbar() {
        let (h, mut p) = plugin_with_data();
        assert_eq!(p.state.snapshot.as_ref().unwrap().quotes.len(), 2);
        assert!(!h.requests().is_empty());
        let watch = p.render(VIEW_WATCHLIST, None, &h.invocation()).unwrap();
        match &watch.body {
            Body::Other { kind, .. } => assert_eq!(kind, "table"),
            other => panic!("expected a sortable table body, got {other:?}"),
        }
        assert_eq!(watch.toolbar.len(), 4);
        assert!(h.refreshed_views().contains(&VIEW_WATCHLIST.to_string()));
    }

    #[test]
    fn native_table_carries_data_and_refreshes_on_change() {
        let (h, mut p) = plugin_with_data();
        let view = p.render(VIEW_WATCHLIST, None, &h.invocation()).unwrap();
        let Body::Other { kind, data } = view.body else { panic!() };
        assert_eq!(kind, "table");
        // Data is present in the rendered JSON, so the frame is never blank.
        assert!(data.contains("贵州茅台"));
        assert!(data.contains("1520.00"));
        assert!(data.contains("涨跌%"));
        // A data change re-renders the view (host-side `refresh_view`); it does
        // not depend on an in-frame message bridge.
        assert!(h.refreshed_views().contains(&VIEW_WATCHLIST.to_string()));
    }

    #[test]
    fn arrow_style_setting_toggles_the_direction_arrow_in_cells() {
        let (h, mut p) = plugin_with_data();
        let before = match p.render(VIEW_WATCHLIST, None, &h.invocation()).unwrap().body {
            Body::Other { data, .. } => data,
            _ => panic!(),
        };
        assert!(before.contains("+1.33% ▲"), "arrow shown when enabled: {before}");
        h.set_setting("changeStyle", "\"sign\"");
        let after = match p.render(VIEW_WATCHLIST, None, &h.invocation()).unwrap().body {
            Body::Other { data, .. } => data,
            _ => panic!(),
        };
        assert!(after.contains("+1.33%") && !after.contains('▲'), "arrow hidden when sign-only: {after}");
    }

    #[test]
    fn empty_watchlist_renders_an_empty_table() {
        let h = mock();
        let mut p = StockPlugin::default();
        let view = p.render(VIEW_WATCHLIST, None, &h.invocation()).unwrap();
        let Body::Other { data, .. } = view.body else { panic!() };
        assert!(data.contains(r#""totalRows":0"#));
        assert!(h.requests().is_empty());
    }

    #[test]
    fn table_sort_event_is_applied_and_refreshes() {
        let (h, mut p) = plugin_with_data();
        p.handle(
            VIEW_WATCHLIST,
            ViewEvent::Other {
                kind: "sort".into(),
                data: r#"{"column":3,"descending":true}"#.into(),
            },
            &h.invocation(),
        )
        .unwrap();
        assert_eq!(p.state.sort, TableSort { column: 3, descending: true });
        assert!(h.storage().contains_key("sort.v1"));
        assert!(h.refreshed_views().contains(&VIEW_WATCHLIST.to_string()));
    }

    #[test]
    fn double_click_activates_the_row_and_opens_its_quote() {
        let (h, mut p) = plugin_with_data();
        // Row 0 is the pinned index (1.000001, from the GOOD payload), before the
        // single watch item (1.600519).
        p.handle(
            VIEW_WATCHLIST,
            ViewEvent::Other {
                kind: "table-activate".into(),
                data: r#"{"row":0}"#.into(),
            },
            &h.invocation(),
        )
        .unwrap();
        let urls = h.opened_urls();
        assert!(urls.iter().any(|u| u.contains("sh000001.html")), "urls: {urls:?}");
    }

    #[test]
    fn scheduled_tick_refreshes_the_view() {
        let (h, mut p) = plugin_with_data();
        let before = h.refreshed_views().len();
        p.handle_events(&[Event::Scheduled], &h.invocation());
        assert!(h.refreshed_views().len() > before, "scheduled tick must refresh the view");
    }

    #[test]
    fn scheduled_tick_force_fetches_and_renders() {
        let (h, mut p) = plugin_with_data();
        let before = h.requests().len();
        p.handle_events(&[Event::Scheduled], &h.invocation());
        // Auto-refresh forces a fetch on every tick (no throttle) so it always
        // pulls fresh data; the render must still succeed.
        assert!(h.requests().len() > before, "auto-refresh must fetch on every tick");
        assert!(h.refreshed_views().contains(&VIEW_WATCHLIST.to_string()));
        assert!(p.render(VIEW_WATCHLIST, None, &h.invocation()).is_ok());
    }

    #[test]
    fn activation_subscribes_to_the_activity_heartbeat() {
        let h = mock();
        let mut p = StockPlugin::default();
        p.activate(&h.context("acme.stock-ticker")).unwrap();
        let subs = h.subscriptions();
        assert!(subs.contains(&EventKind::Scheduled));
        assert!(subs.contains(&EventKind::EditorFocused));
        assert!(subs.contains(&EventKind::FileSaved));
    }

    #[test]
    fn an_active_editor_event_refreshes_when_data_is_stale() {
        let (h, mut p) = plugin_with_data();
        let before = h.requests().len();
        p.handle_events(&[Event::EditorFocused(None)], &h.invocation());
        assert!(h.requests().len() > before, "heartbeat must fetch when stale");
        assert!(h.refreshed_views().contains(&VIEW_WATCHLIST.to_string()));
    }

    #[test]
    fn heartbeat_is_throttled_to_one_fetch_per_15_seconds() {
        let (h, mut p) = plugin_with_data();
        p.handle_events(&[Event::EditorFocused(None)], &h.invocation());
        let after_first = h.requests().len();
        p.handle_events(&[Event::EditorFocused(None)], &h.invocation());
        p.handle_events(&[Event::FileSaved("main.rs".into())], &h.invocation());
        assert_eq!(h.requests().len(), after_first, "repeated events must not refetch");
    }

    #[test]
    fn heartbeat_does_nothing_when_the_last_fetch_is_recent() {
        let (h, mut p) = plugin_with_data();
        p.last_heartbeat_fetch = state::now_secs();
        let before = h.requests().len();
        p.handle_events(&[Event::EditorFocused(None)], &h.invocation());
        assert_eq!(h.requests().len(), before);
    }

    #[test]
    fn heartbeat_does_not_interrupt_the_search_result_list() {
        let (h, mut p) = plugin_in_result_mode();
        p.last_heartbeat_fetch = 0; // deliberately stale
        let before = h.requests().len();
        p.handle_events(&[Event::EditorFocused(None)], &h.invocation());
        assert_eq!(h.requests().len(), before, "no fetch while picking a stock");
        assert!(p.search_results.is_some());
    }

    #[test]
    fn missing_capabilities_still_activate_and_render() {
        let h = MockHost::install();
        let mut p = StockPlugin::default();
        assert!(p.activate(&h.context("acme.stock-ticker")).is_ok());
        assert!(p.render(VIEW_WATCHLIST, None, &h.invocation()).is_ok());
    }

    #[test]
    fn corrupt_storage_does_not_fail_activation() {
        let h = mock();
        host::storage::set("watch.v1", "not-json").unwrap();
        let mut p = StockPlugin::default();
        assert!(p.activate(&h.context("acme.stock-ticker")).is_ok());
        assert!(p.state.watch.is_empty());
    }

    #[test]
    fn duplicate_symbol_is_not_added_twice() {
        let (h, mut p) = plugin_with_data();
        h.answer_next_prompt(Some(&["600519"]));
        p.run_command("add", &h.invocation(), None).unwrap();
        assert_eq!(p.state.watch.len(), 1);
    }

    #[test]
    fn add_by_code_resolves_directly_without_search() {
        let (h, mut p) = plugin_with_data();
        // plugin_with_data already added 600519; add 000001 by code.
        h.answer_next_prompt(Some(&["000001"]));
        p.run_command("add", &h.invocation(), None).unwrap();
        assert_eq!(p.state.watch.len(), 2);
        assert!(p.state.watch.iter().any(|w| w.symbol == "0.000001"));
    }

    /// Drives `add` with a Chinese name so the plugin is in in-panel result mode.
    fn plugin_in_result_mode() -> (MockHost, StockPlugin) {
        let h = mock();
        h.on_fetch(|req| {
            if req.url.contains("suggest/get") {
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![],
                    body: r#"{"QuotationCodeTable":{"Data":[
                        {"Name":"贵州茅台","Classify":"AStock","QuoteID":"1.600519"},
                        {"Name":"贵州燃气","Classify":"AStock","QuoteID":"1.600199"}
                    ],"Status":0,"TotalCount":2}}"#
                        .as_bytes()
                        .to_vec(),
                })
            } else {
                Ok(HttpResponse { status: 200, headers: vec![], body: GOOD.as_bytes().to_vec() })
            }
        });
        let mut p = StockPlugin::default();
        p.activate(&h.context("acme.stock-ticker")).unwrap();
        h.answer_next_prompt(Some(&["茅台"])); // host::input text
        p.run_command("add", &h.invocation(), None).unwrap();
        (h, p)
    }

    #[test]
    fn add_by_name_switches_to_an_in_panel_result_list() {
        let (h, mut p) = plugin_in_result_mode();
        // Nothing is added yet — the user chooses from the panel.
        assert!(p.state.watch.is_empty());
        let view = p.render(VIEW_WATCHLIST, None, &h.invocation()).unwrap();
        match &view.body {
            Body::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].key, "1.600519");
                assert!(items[0].label.contains("贵州茅台"));
            }
            other => panic!("expected an in-panel result list, got {other:?}"),
        }
        assert_eq!(view.toolbar.len(), 2, "result mode shows only back + re-search");
        assert!(view.status.as_deref().unwrap_or("").contains("匹配 2 只"));
        assert!(h.refreshed_views().contains(&VIEW_WATCHLIST.to_string()));
    }

    #[test]
    fn add_by_pinyin_initials_shows_a_filtered_result_list() {
        let h = mock();
        h.on_fetch(|req| {
            if req.url.contains("suggest/get") {
                assert!(req.url.contains("input=dftx"), "url: {}", req.url);
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![],
                    body: r#"{"QuotationCodeTable":{"Data":[
                        {"Name":"东方通信","Classify":"AStock","QuoteID":"1.600776"},
                        {"Name":"Definium Therapeutics","Classify":"UsStock","QuoteID":"105.DFTX"}
                    ],"Status":0,"TotalCount":2}}"#
                        .as_bytes()
                        .to_vec(),
                })
            } else {
                Ok(HttpResponse { status: 200, headers: vec![], body: GOOD.as_bytes().to_vec() })
            }
        });
        let mut p = StockPlugin::default();
        p.activate(&h.context("acme.stock-ticker")).unwrap();
        h.answer_next_prompt(Some(&["dftx"]));
        p.run_command("add", &h.invocation(), None).unwrap();
        let view = p.render(VIEW_WATCHLIST, None, &h.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a result list") };
        assert_eq!(items.len(), 1, "the US stock must be filtered out");
        assert_eq!(items[0].key, "1.600776");
        assert!(p.state.watch.is_empty());
    }

    #[test]
    fn activating_a_result_adds_the_symbol_and_returns_to_table() {
        let (h, mut p) = plugin_in_result_mode();
        p.handle(VIEW_WATCHLIST, ViewEvent::Activate("1.600519".into()), &h.invocation()).unwrap();
        assert_eq!(p.state.watch.len(), 1);
        assert_eq!(p.state.watch[0].symbol, "1.600519");
        let view = p.render(VIEW_WATCHLIST, None, &h.invocation()).unwrap();
        assert!(matches!(view.body, Body::Other { ref kind, .. } if kind == "table"));
        assert_eq!(view.toolbar.len(), 4, "back to the normal quote toolbar");
    }

    #[test]
    fn back_button_discards_results_without_adding() {
        let (h, mut p) = plugin_in_result_mode();
        p.handle(VIEW_WATCHLIST, ViewEvent::Button("back".into()), &h.invocation()).unwrap();
        assert!(p.state.watch.is_empty());
        let view = p.render(VIEW_WATCHLIST, None, &h.invocation()).unwrap();
        assert!(matches!(view.body, Body::Other { ref kind, .. } if kind == "table"));
    }

    #[test]
    fn single_click_in_result_mode_does_not_add_by_accident() {
        let (h, mut p) = plugin_in_result_mode();
        // The IDE also emits Select while the user moves with arrow keys.
        p.handle(VIEW_WATCHLIST, ViewEvent::Select("1.600519".into()), &h.invocation()).unwrap();
        assert!(p.state.watch.is_empty());
        assert!(p.search_results.is_some());
    }

    #[test]
    fn add_by_name_with_no_match_is_rejected() {
        let h = mock();
        h.on_fetch(|req| {
            if req.url.contains("suggest/get") {
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![],
                    body: r#"{"QuotationCodeTable":{"Data":null,"Status":0,"TotalCount":0}}"#.as_bytes().to_vec(),
                })
            } else {
                Ok(HttpResponse { status: 200, headers: vec![], body: GOOD.as_bytes().to_vec() })
            }
        });
        let mut p = StockPlugin::default();
        p.activate(&h.context("acme.stock-ticker")).unwrap();
        h.answer_next_prompt(Some(&["不存在的股票xyz"]));
        let err = p.run_command("add", &h.invocation(), None).unwrap_err();
        assert!(format!("{err}").contains("未找到匹配"));
    }

    #[test]
    fn removing_a_selected_row_confirms_then_deletes() {
        let (h, mut p) = plugin_with_data();
        // The fixture only carries 上证指数, so indices contributes one row and the
        // single watch item sits at table row 1.
        p.handle(
            VIEW_WATCHLIST,
            ViewEvent::Other { kind: "table-select".into(), data: r#"{"row":1}"#.into() },
            &h.invocation(),
        )
        .unwrap();
        h.answer_next_prompt(Some(&["yes"])); // host::confirm -> true
        p.run_command("remove", &h.invocation(), None).unwrap();
        assert!(p.state.watch.is_empty());
    }

    #[test]
    fn removing_a_selected_row_is_cancelled_when_confirm_says_no() {
        let (h, mut p) = plugin_with_data();
        p.handle(
            VIEW_WATCHLIST,
            ViewEvent::Other { kind: "table-select".into(), data: r#"{"row":1}"#.into() },
            &h.invocation(),
        )
        .unwrap();
        h.answer_next_prompt(None); // host::confirm -> false
        p.run_command("remove", &h.invocation(), None).unwrap();
        assert_eq!(p.state.watch.len(), 1);
    }
}
