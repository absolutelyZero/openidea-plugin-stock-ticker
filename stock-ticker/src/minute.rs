//! Intraday minute data from Tencent's public `web.ifzq.gtimg.cn` endpoint.
//! The WebView cannot fetch directly: every call here runs inside the plugin
//! behind a task budget, with the same 4 s timeout convention as quotes.

use crate::{
    model::{self, MinutePoint, MinuteSeries},
    state::{now_secs, State},
};
use openidea_plugin::{Error, HttpRequest, host};

pub const MINUTE_HOST: &str = "web.ifzq.gtimg.cn";
pub const MINUTE_SOURCE: &str = "腾讯分时";
/// How long a successful result stays fresh in the plugin's instance cache.
const MINUTE_TTL_SECS: i64 = 60;
/// Even a forced refresh must wait this long, so hovering cannot flood tasks.
const MINUTE_MIN_INTERVAL_SECS: i64 = 5;

#[derive(Debug)]
pub enum MinuteError {
    Denied(String),
    Http(u16),
    Empty,
    Parse(String),
    InvalidSymbol,
    Cancelled,
}

impl MinuteError {
    pub fn status(&self) -> &'static str {
        match self {
            MinuteError::Denied(_) => "denied",
            MinuteError::Http(_) => "http_error",
            MinuteError::Empty => "empty",
            MinuteError::Parse(_) => "parse_error",
            MinuteError::InvalidSymbol => "invalid_symbol",
            MinuteError::Cancelled => "cancelled",
        }
    }

    pub fn message(&self) -> String {
        match self {
            MinuteError::Denied(cap) => {
                format!("分时数据需要网络权限，请在插件权限中授权 {cap}")
            }
            MinuteError::Http(code) => format!("腾讯分时 HTTP {code}"),
            MinuteError::Empty => "暂无分时数据（非交易日或未开盘）".into(),
            MinuteError::Parse(e) => format!("腾讯分时解析失败：{e}"),
            MinuteError::InvalidSymbol => "无法识别的股票代码".into(),
            MinuteError::Cancelled => "已取消".into(),
        }
    }

    pub fn grant(&self) -> Option<&str> {
        match self {
            MinuteError::Denied(cap) => Some(cap),
            _ => None,
        }
    }
}

/// `https://web.ifzq.gtimg.cn/appstock/app/minute/query?code=sh600519`
pub fn minute_url(code: &str) -> String {
    format!("https://{MINUTE_HOST}/appstock/app/minute/query?code={code}")
}

/// Beijing calendar date as `YYYYMMDD`, used to invalidate cached series
/// across trading days without trusting the instance clock's timezone.
fn beijing_date(epoch_secs: i64) -> String {
    let days = (epoch_secs + 8 * 3_600).div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

/// Parse a Tencent minute response. Each sample is
/// `"HHMM price cumulative_volume cumulative_amount"`. Malformed samples are
/// skipped; an entirely empty series is reported as `Empty` rather than a
/// parse failure so callers can distinguish "no trading data" from a broken
/// upstream schema.
///
/// The live payload nests the series one level deeper than the path suggests:
///
/// ```text
/// data.<code>.data = { "date": "20261120", "data": ["0930 1450.89 235 34095914.67", ...] }
/// ```
///
/// while some mirrors/proxies flatten it to `data.<code>.data = [...]` with a
/// sibling `date`. Both shapes are accepted so a cosmetic upstream change
/// cannot silently degrade into "no trading data".
pub fn parse_minute(bytes: &[u8], symbol: &str) -> Result<MinuteSeries, MinuteError> {
    let code = model::tencent_code(symbol).ok_or(MinuteError::InvalidSymbol)?;
    let root: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| MinuteError::Parse(e.to_string()))?;
    let entry = root
        .get("data")
        .and_then(|d| d.get(&code))
        .ok_or(MinuteError::Empty)?;

    // Nested (live) shape first, then the flattened shape.
    let inner = entry.get("data");
    let (date_field, samples_field) = match inner {
        Some(v) if v.is_object() => (v.get("date"), v.get("data")),
        _ => (entry.get("date"), inner),
    };
    let date = date_field.and_then(|v| v.as_str()).unwrap_or("").to_string();
    let samples = samples_field.and_then(|v| v.as_array()).ok_or(MinuteError::Empty)?;

    let mut points = Vec::new();
    for sample in samples {
        let Some(text) = sample.as_str() else { continue };
        let mut fields = text.split_whitespace();
        let (Some(time), Some(price), Some(volume)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if time.len() != 4 || !time.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let (Ok(price), Ok(volume)) = (price.parse::<f64>(), volume.parse::<f64>()) else {
            continue;
        };
        if !price.is_finite() || !volume.is_finite() || price <= 0.0 {
            continue;
        }
        points.push(MinutePoint {
            time: time.to_string(),
            price,
            volume,
        });
    }
    if points.is_empty() {
        return Err(MinuteError::Empty);
    }

    // The quote array embedded in the minute response carries the previous
    // close at index 4, matching the batch quote parser in `quote.rs`.
    let prev_close = entry
        .get("qt")
        .and_then(|qt| qt.get(&code))
        .and_then(|q| q.get(4))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0);

    Ok(MinuteSeries {
        symbol: symbol.to_string(),
        date,
        prev_close,
        points,
        fetched_at: now_secs(),
        source: MINUTE_SOURCE.to_string(),
    })
}

/// Return a fresh series, using the instance cache when it is still valid.
/// Failures never enter the cache, so a transient outage can be retried.
pub fn fetch_minute(state: &State, symbol: &str, force: bool) -> Result<MinuteSeries, MinuteError> {
    let now = now_secs();
    let today = beijing_date(now);
    if let Some(cached) = state.minute_cache.get(symbol) {
        let age = now.saturating_sub(cached.fetched_at);
        let interval = if force { MINUTE_MIN_INTERVAL_SECS } else { MINUTE_TTL_SECS };
        if cached.date == today && age < interval {
            return Ok(cached.clone());
        }
    }

    let code = model::tencent_code(symbol).ok_or(MinuteError::InvalidSymbol)?;
    let task = host::begin_task("加载分时图", true);
    task.report(0, Some(2), Some("请求分时数据"));
    if task.is_cancelled() {
        return Err(MinuteError::Cancelled);
    }

    let request = HttpRequest {
        timeout_ms: Some(4000),
        ..HttpRequest::get(&minute_url(&code)).header("Accept", "application/json")
    };
    let result = match host::fetch(&request) {
        Ok(response) if response.status == 200 => {
            let mut series = parse_minute(&response.body, symbol)?;
            if series.prev_close.is_none() {
                series.prev_close = state
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.quotes.iter().find(|q| q.symbol == symbol))
                    .and_then(|q| q.prev_close)
                    .filter(|v| v.is_finite() && *v > 0.0);
            }
            series.fetched_at = now_secs();
            Ok(series)
        }
        Ok(response) => Err(MinuteError::Http(response.status)),
        Err(Error::Denied(cap)) => Err(MinuteError::Denied(cap.to_string())),
        Err(Error::Cancelled) => Err(MinuteError::Cancelled),
        Err(e) => Err(MinuteError::Parse(format!("腾讯分时：{e}"))),
    };
    task.report(2, Some(2), None);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Quote, Snapshot, WatchItem};
    use openidea_plugin::{HttpResponse, mock::MockHost};

    /// The live shape: `data.<code>.data = { date, data: [...] }`.
    fn sample() -> Vec<u8> {
        r#"{"code":0,"msg":"","data":{"sh600519":{"data":{"data":[
            "0930 1438.55 18370 26401631.50",
            "0931 1438.10 18420 26500000.00",
            "bad sample",
            "0932 nope 10 20"
        ],"date":"20261120"},"qt":{"sh600519":["1","贵州茅台","600519","1438.10","1435.26"]},"mx_price":1}}}"#
            .as_bytes()
            .to_vec()
    }

    /// The flattened shape used by some mirrors: array + sibling `date`.
    fn flat_sample() -> Vec<u8> {
        r#"{"code":0,"data":{"sh600519":{"date":"20261120","data":[
            "0930 1438.55 18370 26401631.50",
            "0931 1438.10 18420 26500000.00"
        ]}}}"#
            .as_bytes()
            .to_vec()
    }

    fn mock() -> MockHost {
        MockHost::install().with_capabilities(&[
            "storage",
            "events",
            "net:web.ifzq.gtimg.cn",
        ])
    }

    fn state() -> State {
        let mut s = State::default();
        s.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        s
    }

    #[test]
    fn parses_live_shape_and_skips_bad_points() {
        let series = parse_minute(&sample(), "1.600519").unwrap();
        assert_eq!(series.date, "20261120");
        assert_eq!(series.prev_close, Some(1435.26));
        assert_eq!(series.points.len(), 2);
        assert_eq!(series.points[0].time, "0930");
        assert_eq!(series.points[0].price, 1438.55);
        assert_eq!(series.points[1].volume, 18420.0);
        assert_eq!(series.source, MINUTE_SOURCE);
    }

    #[test]
    fn accepts_both_nested_and_flattened_payloads() {
        let nested = parse_minute(&sample(), "1.600519").unwrap();
        let flat = parse_minute(&flat_sample(), "1.600519").unwrap();
        assert_eq!(nested.date, "20261120");
        assert_eq!(flat.date, "20261120");
        assert_eq!(nested.points.len(), 2);
        assert_eq!(flat.points.len(), 2);
        assert_eq!(nested.points[0].price, flat.points[0].price);
    }

    /// Regression guard for the 0.4.0 bug: the parser read `data.<code>.data`
    /// as an array, missed the extra nesting level and reported every trading
    /// day as "no intraday data". The fixture is a trimmed capture of the
    /// real endpoint taken during the afternoon session.
    #[test]
    fn parses_captured_live_payload() {
        let raw = include_bytes!("../tests/fixtures/minute_live_sh600519.json");
        let series = parse_minute(raw, "1.600519").unwrap();
        assert_eq!(series.date, "20261120");
        assert_eq!(series.points.len(), 6);
        assert_eq!(series.points[0].time, "0930");
        assert_eq!(series.points[0].price, 1450.89);
        assert_eq!(series.points[0].volume, 235.0);
        assert_eq!(series.points.last().unwrap().time, "1312");
        assert_eq!(series.prev_close, Some(1460.21));
    }

    #[test]
    fn empty_and_malformed_payloads_have_distinct_errors() {
        let empty = br#"{"code":0,"data":{"sh600519":{"data":{"date":"20261120","data":[]}}}}"#;
        assert_eq!(parse_minute(empty, "1.600519").unwrap_err().status(), "empty");
        let missing = br#"{"code":0,"data":{}}"#;
        assert_eq!(parse_minute(missing, "1.600519").unwrap_err().status(), "empty");
        let unreadable = br#"{"code":0,"data":{"sh600519":{"data":{"date":"20261120"}}}}"#;
        assert_eq!(parse_minute(unreadable, "1.600519").unwrap_err().status(), "empty");
        assert_eq!(parse_minute(b"not json", "1.600519").unwrap_err().status(), "parse_error");
        assert_eq!(parse_minute(&sample(), "9.999999").unwrap_err().status(), "invalid_symbol");
    }

    #[test]
    fn secid_conversion_covers_sh_sz_and_bj() {
        assert_eq!(model::tencent_code("1.600519").as_deref(), Some("sh600519"));
        assert_eq!(model::tencent_code("0.000001").as_deref(), Some("sz000001"));
        assert_eq!(model::tencent_code("0.430047").as_deref(), Some("bj430047"));
        assert_eq!(model::tencent_code("2.600519"), None);
    }

    #[test]
    fn fetch_hits_expected_url_timeout_and_task() {
        let h = mock();
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: sample() }));
        let s = state();
        let series = fetch_minute(&s, "1.600519", true).unwrap();
        assert_eq!(series.points.len(), 2);
        let requests = h.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url,
            "https://web.ifzq.gtimg.cn/appstock/app/minute/query?code=sh600519"
        );
        assert_eq!(requests[0].timeout_ms, Some(4000));
        assert!(!h.tasks().is_empty());
    }

    #[test]
    fn errors_are_classified_and_denied_is_actionable() {
        let h = mock();
        h.on_fetch(|_| Ok(HttpResponse { status: 503, headers: vec![], body: vec![] }));
        let err = fetch_minute(&state(), "1.600519", true).unwrap_err();
        assert_eq!(err.status(), "http_error");
        assert!(err.message().contains("503"));

        let h2 = MockHost::install().with_capabilities(&["storage", "events"]);
        h2.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: sample() }));
        let err2 = fetch_minute(&state(), "1.600519", true).unwrap_err();
        assert_eq!(err2.status(), "denied");
        assert_eq!(err2.grant(), Some("net:web.ifzq.gtimg.cn"));
        assert!(err2.message().contains("net:web.ifzq.gtimg.cn"));
    }

    #[test]
    fn cache_hit_and_force_min_interval_skip_requests() {
        let h = mock();
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: sample() }));
        let mut s = state();
        let first = fetch_minute(&s, "1.600519", true).unwrap();
        s.minute_cache.insert("1.600519".into(), first);
        let _ = fetch_minute(&s, "1.600519", false).unwrap();
        let _ = fetch_minute(&s, "1.600519", true).unwrap();
        assert_eq!(h.requests().len(), 1, "cache and force interval suppress fetches");
    }

    #[test]
    fn stale_day_and_failed_fetch_do_not_use_cache() {
        let mut s = state();
        let mut stale = parse_minute(&sample(), "1.600519").unwrap();
        stale.date = "20200101".into();
        s.minute_cache.insert("1.600519".into(), stale);

        let h = mock();
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: sample() }));
        let fresh = fetch_minute(&s, "1.600519", false).unwrap();
        assert_eq!(fresh.date, "20261120");
        assert_eq!(h.requests().len(), 1);

        let h2 = mock();
        h2.on_fetch(|_| Ok(HttpResponse { status: 500, headers: vec![], body: vec![] }));
        let s2 = state();
        assert!(fetch_minute(&s2, "1.600519", true).is_err());
        assert!(s2.minute_cache.is_empty(), "failures are never cached");
    }

    #[test]
    fn prev_close_falls_back_to_the_quote_snapshot() {
        let mut s = state();
        s.snapshot = Some(Snapshot {
            fetched_at: 1,
            source: "test".into(),
            quotes: vec![Quote {
                symbol: "1.600519".into(),
                prev_close: Some(1435.26),
                ..Default::default()
            }],
        });
        let body = br#"{"data":{"sh600519":{"date":"20261120","data":["0930 1438.55 1 1"]}}}"#;
        let h = mock();
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: body.to_vec() }));
        assert_eq!(fetch_minute(&s, "1.600519", true).unwrap().prev_close, Some(1435.26));
    }
}
