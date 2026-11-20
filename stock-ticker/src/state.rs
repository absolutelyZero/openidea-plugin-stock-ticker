//! Persisted state. The wasm instance is reclaimed after five idle minutes, so
//! `host::storage` is the only source of truth; instance memory is a cache.

use crate::model::{MinuteSeries, OrderMode, Snapshot, WatchItem};
use openidea_plugin::{Error, LogLevel, host, views::TableSort};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const WATCH: &str = "watch.v1";
const QUOTES: &str = "quotes.v1";
const SORT: &str = "sort.v1";
const ORDER_MODE: &str = "order-mode.v1";
const META: &str = "meta.v1";
const ALERT_PREFIX: &str = "alert.";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub last_fetch_at: i64,
    pub failures: u32,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct SortState {
    column: u32,
    descending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub precision: usize,
    pub arrow_style: bool,
    pub stale_after_secs: i64,
}

impl DisplayConfig {
    /// Used by `Default`, where no host is guaranteed to be installed.
    fn fallback() -> Self {
        Self {
            precision: 2,
            arrow_style: true,
            stale_after_secs: 180,
        }
    }

    /// Settings are read at render time; API 1.3 has no settings-changed event.
    pub fn read() -> Self {
        Self {
            precision: host::setting_number("precision")
                .map(|v| v.clamp(0.0, 6.0) as usize)
                .unwrap_or(2),
            arrow_style: host::setting_string("changeStyle").as_deref() != Some("sign"),
            stale_after_secs: host::setting_number("staleAfterSecs")
                .map(|v| v.clamp(30.0, 3600.0) as i64)
                .unwrap_or(180),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub watch: Vec<WatchItem>,
    pub snapshot: Option<Snapshot>,
    pub sort: TableSort,
    pub order_mode: OrderMode,
    pub meta: Meta,
    pub config: DisplayConfig,
    /// Per-symbol intraday cache for this instance only. Not persisted:
    /// minute data is short-lived and storage is the persisted source of truth.
    pub minute_cache: std::collections::HashMap<String, MinuteSeries>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            watch: Vec::new(),
            snapshot: None,
            sort: TableSort { column: 0, descending: false },
            order_mode: OrderMode::Column,
            meta: Meta::default(),
            config: DisplayConfig::fallback(),
            minute_cache: std::collections::HashMap::new(),
        }
    }
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn read<T: serde::de::DeserializeOwned>(key: &str, fallback: T) -> T {
    match host::storage::get(key) {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_else(|_| {
            host::log(LogLevel::Warn, &format!("{key} is corrupt; using defaults"));
            fallback
        }),
        _ => fallback,
    }
}

fn write<T: Serialize>(key: &str, value: &T) -> Result<(), Error> {
    let json = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    host::storage::set(key, &json)
}

impl State {
    pub fn load() -> Self {
        let sort: SortState = read(SORT, SortState { column: 0, descending: false });
        let mut state = Self {
            watch: read(WATCH, Vec::new()),
            snapshot: read(QUOTES, None),
            sort: TableSort { column: sort.column, descending: sort.descending },
            // Legacy data has no order-mode key: fall back to column sorting
            // so upgrades keep the previous visible behavior.
            order_mode: read(ORDER_MODE, OrderMode::Column),
            meta: read(META, Meta::default()),
            config: DisplayConfig::read(),
            minute_cache: std::collections::HashMap::new(),
        };
        if state.watch.len() > crate::model::MAX_WATCH {
            state.watch.truncate(crate::model::MAX_WATCH);
        }
        state
    }

    pub fn save_watch(&self) -> Result<(), Error> {
        write(WATCH, &self.watch)
    }

    pub fn save_snapshot(&self) -> Result<(), Error> {
        match &self.snapshot {
            Some(s) => write(QUOTES, s),
            None => host::storage::delete(QUOTES),
        }
    }

    pub fn save_sort(&self) -> Result<(), Error> {
        write(
            SORT,
            &SortState { column: self.sort.column, descending: self.sort.descending },
        )
    }

    pub fn save_order_mode(&self) -> Result<(), Error> {
        write(ORDER_MODE, &self.order_mode)
    }

    pub fn save_meta(&self) -> Result<(), Error> {
        write(META, &self.meta)
    }

    /// UTC day number, avoiding calendar arithmetic without a date library.
    pub fn alert_day(&self) -> i64 {
        now_secs() / 86_400
    }

    pub fn alert_fired(&self, symbol: &str) -> bool {
        matches!(host::storage::get(&format!("{ALERT_PREFIX}{symbol}")),
                 Ok(Some(day)) if day == self.alert_day().to_string())
    }

    pub fn mark_alert(&self, symbol: &str) {
        let _ = host::storage::set(
            &format!("{ALERT_PREFIX}{symbol}"),
            &self.alert_day().to_string(),
        );
    }

    pub fn refresh_config(&mut self) {
        self.config = DisplayConfig::read();
    }

    pub fn status_text(&self) -> String {
        let (time, stale, clock) = match &self.snapshot {
            Some(s) => {
                let age = now_secs().saturating_sub(s.fetched_at);
                let clock = s
                    .quotes
                    .iter()
                    .filter_map(|q| q.ts)
                    .max()
                    .map(fmt_clock)
                    .unwrap_or_default();
                (format_age(age), age > self.config.stale_after_secs, clock)
            }
            None => ("无数据".into(), true, String::new()),
        };
        let suffix = match &self.meta.last_error {
            Some(e) => format!(" · {e}"),
            None => String::new(),
        };
        let stale_mark = if stale { " · 陈旧" } else { "" };
        // The quote clock (from the data source) makes a refresh visible: if it
        // never advances, nothing is being fetched.
        let clock_part = if clock.is_empty() { String::new() } else { format!(" · {clock}") };
        format!(
            "{} 只 · {}{} · {}{}{}",
            self.watch.len(),
            self.snapshot.as_ref().map(|s| s.source.as_str()).unwrap_or("未拉取"),
            clock_part,
            time,
            stale_mark,
            suffix
        )
    }
}

/// Beijing-time clock for a quote timestamp (seconds since the epoch).
fn fmt_clock(ts: i64) -> String {
    let s = (ts + 8 * 3_600).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3_600, (s % 3_600) / 60, s % 60)
}

fn format_age(secs: i64) -> String {
    if secs < 60 {
        format!("{secs}s 前")
    } else if secs < 3600 {
        format!("{}m 前", secs / 60)
    } else {
        format!("{}h 前", secs / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn load_round_trips_all_keys() {
        let h = MockHost::install().with_capabilities(&["storage"]);
        let mut s = State::load();
        s.watch.push(WatchItem { symbol: "1.600519".into(), cost: Some(1500.0), ..Default::default() });
        s.sort = TableSort { column: 4, descending: true };
        s.order_mode = OrderMode::Custom;
        s.meta.failures = 2;
        s.snapshot = Some(Snapshot {
            fetched_at: 123,
            source: "primary".into(),
            quotes: vec![],
        });
        s.save_watch().unwrap();
        s.save_sort().unwrap();
        s.save_order_mode().unwrap();
        s.save_meta().unwrap();
        s.save_snapshot().unwrap();
        let restored = State::load();
        assert_eq!(restored.watch, s.watch);
        assert_eq!(restored.sort, s.sort);
        assert_eq!(restored.order_mode, OrderMode::Custom);
        assert_eq!(restored.meta.failures, 2);
        assert_eq!(restored.snapshot.unwrap().fetched_at, 123);
        assert_eq!(h.storage()["watch.v1"], serde_json::to_string(&s.watch).unwrap());
    }

    #[test]
    fn missing_order_mode_falls_back_to_column() {
        let _h = MockHost::install().with_capabilities(&["storage"]);
        // No order-mode.v1 key: legacy data must keep column behavior and not
        // lose watch/sort data.
        host::storage::set("watch.v1", r#"[{"symbol":"1.600519"}]"#).unwrap();
        let s = State::load();
        assert_eq!(s.order_mode, OrderMode::Column);
        assert_eq!(s.watch.len(), 1);
        assert_eq!(s.watch[0].symbol, "1.600519");
    }

    #[test]
    fn corrupt_storage_falls_back() {
        let _h = MockHost::install().with_capabilities(&["storage"]);
        host::storage::set("watch.v1", "not-json").unwrap();
        let s = State::load();
        assert!(s.watch.is_empty());
        assert!(s.snapshot.is_none());
    }

    #[test]
    fn settings_are_read_live() {
        let h = MockHost::install().with_capabilities(&["storage"]);
        h.set_setting("precision", "3");
        h.set_setting("changeStyle", "\"sign\"");
        let c = DisplayConfig::read();
        assert_eq!(c.precision, 3);
        assert!(!c.arrow_style);
    }

    #[test]
    fn alert_deduplicates_within_a_day() {
        let _h = MockHost::install().with_capabilities(&["storage"]);
        let s = State::load();
        assert!(!s.alert_fired("1.600519"));
        s.mark_alert("1.600519");
        assert!(s.alert_fired("1.600519"));
    }

    #[test]
    fn status_text_marks_stale_data() {
        let _h = MockHost::install().with_capabilities(&["storage"]);
        let mut s = State::load();
        assert!(s.status_text().contains("无数据"));
        s.snapshot = Some(Snapshot {
            fetched_at: now_secs() - 10_000,
            source: "primary".into(),
            quotes: vec![],
        });
        assert!(s.status_text().contains("陈旧"));
    }
}
