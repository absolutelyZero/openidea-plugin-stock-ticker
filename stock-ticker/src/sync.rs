//! Quote fetching from Tencent's public `qt.gtimg.cn` endpoint, with a task
//! budget, alerting and graceful degradation. Nothing here runs during
//! `render()`.

use crate::{
    model::{Quote, Snapshot},
    state::{now_secs, State},
};
use openidea_plugin::{
    Error, HttpRequest, NotificationAction, NotificationLevel, host,
};

const QUOTE_HOST: &str = "qt.gtimg.cn";
const SOURCE_LABEL: &str = "腾讯行情";
/// A non-forced refresh is skipped while the last attempt is this recent.
const THROTTLE_SECS: i64 = 55;

/// `market.code` secid → the `sh`/`sz`/`bj` code `qt.gtimg.cn` expects.
fn tencent_code(secid: &str) -> Option<String> {
    let (market, code) = secid.split_once('.')?;
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let prefix = match market {
        "1" => "sh",
        // Beijing listings share the 0. channel in the stored secid.
        "0" if code.starts_with('4') || code.starts_with('8') => "bj",
        "0" => "sz",
        _ => return None,
    };
    Some(format!("{prefix}{code}"))
}

fn build_codes(state: &State) -> Vec<String> {
    let mut ids: Vec<String> = state.watch.iter().map(|w| w.symbol.clone()).collect();
    let mut seen = std::collections::BTreeSet::new();
    ids.retain(|s| seen.insert(s.clone()));
    ids.iter().filter_map(|s| tencent_code(s)).collect()
}

fn build_url(codes: &[String]) -> String {
    format!("https://{QUOTE_HOST}/q={}", codes.join(","))
}

fn check_alerts(state: &State) {
    let Some(snapshot) = &state.snapshot else { return };
    for item in &state.watch {
        let Some(q) = snapshot.quotes.iter().find(|q| q.symbol == item.symbol) else { continue };
        let Some(price) = q.price else { continue };
        let hit = item.above.map(|v| price >= v).unwrap_or(false)
            || item.below.map(|v| price <= v).unwrap_or(false);
        if !hit || state.alert_fired(&item.symbol) {
            continue;
        }
        let name = item.alias.clone().unwrap_or_else(|| q.name.clone());
        let body = format!("{name} 现价 {price:.2}，已触及设定的预警阈值");
        let _ = host::notify_with_actions(
            NotificationLevel::Warning,
            &format!("{name} 触及预警"),
            &body,
            &[NotificationAction::new("打开面板", "reveal")],
        );
        state.mark_alert(&item.symbol);
    }
}

/// `force` bypasses the 55 s throttle (toolbar / shortcut / add-symbol).
/// All failures degrade: keep the cached snapshot, record the reason, and
/// return `Ok(())` so the command does not count as failed.
pub fn sync(state: &mut State, force: bool) -> Result<(), Error> {
    let now = now_secs();
    if !force && now.saturating_sub(state.meta.last_fetch_at) < THROTTLE_SECS {
        return Ok(());
    }
    let codes = build_codes(state);
    if codes.is_empty() {
        state.meta.last_error = Some("自选股为空".into());
        let _ = state.save_meta();
        crate::publish(state);
        return Ok(());
    }

    let task = host::begin_task("刷新行情", true);
    task.report(0, Some(3), Some("请求行情"));

    if task.is_cancelled() {
        return Ok(());
    }

    let request = HttpRequest {
        timeout_ms: Some(4000),
        ..HttpRequest::get(&build_url(&codes)).header("Accept", "text/plain")
    };

    let mut outcome: Option<Vec<Quote>> = None;
    let mut last_error: Option<String> = None;
    match host::fetch(&request) {
        Ok(response) if response.status == 200 => {
            match crate::quote::parse_tencent(&response.body) {
                Ok(quotes) => outcome = Some(quotes),
                Err(e) => last_error = Some(format!("腾讯行情解析失败：{e}")),
            }
        }
        Ok(response) => last_error = Some(format!("腾讯行情 HTTP {}", response.status)),
        Err(Error::Denied(cap)) => last_error = Some(format!("网络权限未授予（{cap}）")),
        Err(Error::Cancelled) => return Ok(()),
        Err(e) => last_error = Some(format!("腾讯行情：{e}")),
    }

    match outcome {
        Some(quotes) => {
            state.snapshot = Some(Snapshot {
                fetched_at: now,
                source: SOURCE_LABEL.to_string(),
                quotes,
            });
            state.meta.last_fetch_at = now;
            state.meta.failures = 0;
            state.meta.last_error = None;
            let _ = state.save_snapshot();
            let _ = state.save_meta();
            task.report(2, Some(3), Some("检查预警"));
            check_alerts(state);
        }
        None => {
            state.meta.failures = state.meta.failures.saturating_add(1);
            state.meta.last_error = last_error.or_else(|| Some("行情源不可达".into()));
            let _ = state.save_meta();
        }
    }
    task.report(3, Some(3), None);
    crate::publish(state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WatchItem;
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
            "v_sh000001=\"1~上证指数~000001~3620.00~3634.35~3611.54~277036290~0~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~~20261116113200~-14.35~-0.39~3623.12~3593.45~3620.00/277036290/421756737368~277036290~42175674~0.57~16.50\";"
        ))
    }

    fn mock() -> MockHost {
        MockHost::install().with_capabilities(&[
            "storage",
            "events",
            "net:qt.gtimg.cn",
        ])
    }

    #[test]
    fn builds_one_batched_request_for_the_watchlist() {
        let h = mock();
        let mut s = State::load();
        s.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: good() }));
        sync(&mut s, true).unwrap();
        let requests = h.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.starts_with("https://qt.gtimg.cn/q="), "url: {}", requests[0].url);
        assert!(requests[0].url.contains("sh600519"));
        assert!(!requests[0].url.contains("sh000001"), "indices are no longer fetched");
        assert_eq!(requests[0].timeout_ms, Some(4000));
        assert_eq!(s.snapshot.as_ref().unwrap().quotes.len(), 2);
        assert_eq!(s.snapshot.as_ref().unwrap().source, "腾讯行情");
        assert_eq!(s.meta.failures, 0);
    }

    #[test]
    fn bad_payload_keeps_the_previous_snapshot() {
        let h = mock();
        let mut s = State::load();
        s.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        s.snapshot = Some(Snapshot {
            fetched_at: 1,
            source: "旧缓存".into(),
            quotes: vec![],
        });
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: b"not a quote".to_vec() }));
        sync(&mut s, true).unwrap();
        assert_eq!(s.snapshot.as_ref().unwrap().source, "旧缓存");
        assert!(s.status_text().contains("解析失败"));
    }

    #[test]
    fn http_error_keeps_the_previous_snapshot_and_reports_status() {
        let h = mock();
        let mut s = State::load();
        s.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        h.on_fetch(|_| Ok(HttpResponse { status: 503, headers: vec![], body: vec![] }));
        sync(&mut s, true).unwrap();
        assert!(s.snapshot.is_none());
        assert!(s.status_text().contains("503"));
    }

    #[test]
    fn denied_network_degrades_without_failing() {
        let h = MockHost::install().with_capabilities(&["storage"]);
        let mut s = State::load();
        s.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        sync(&mut s, true).unwrap();
        assert!(s.status_text().contains("权限"));
        assert!(h.tasks().len() >= 1);
    }

    #[test]
    fn throttle_skips_a_second_call() {
        let h = mock();
        let mut s = State::load();
        s.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: good() }));
        sync(&mut s, true).unwrap();
        sync(&mut s, false).unwrap();
        assert_eq!(h.requests().len(), 1);
    }

    #[test]
    fn alert_fires_once_per_day() {
        let h = mock();
        let mut s = State::load();
        s.watch.push(WatchItem {
            symbol: "1.600519".into(),
            above: Some(1400.0),
            ..Default::default()
        });
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: good() }));
        sync(&mut s, true).unwrap();
        assert_eq!(h.notifications_with_actions().len(), 1);
        sync(&mut s, true).unwrap();
        assert_eq!(h.notifications_with_actions().len(), 1);
    }
}
