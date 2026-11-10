//! Quote fetching with a backup source, task budget, alerting and graceful
//! degradation. Nothing here runs during `render()`.

use crate::{
    model::{Quote, Snapshot, INDEX_SECIDS},
    state::{now_secs, State},
};
use openidea_plugin::{
    Error, HttpRequest, NotificationAction, NotificationLevel, host,
};

const FIELDS: &str = "f2,f3,f4,f5,f6,f12,f13,f14,f15,f16,f17,f18,f124";

fn build_secids(state: &State) -> Vec<String> {
    let mut ids: Vec<String> = state.watch.iter().map(|w| w.symbol.clone()).collect();
    if state.config.show_indices {
        ids.extend(INDEX_SECIDS.iter().map(|s| (*s).to_string()));
    }
    let mut seen = std::collections::BTreeSet::new();
    ids.retain(|s| seen.insert(s.clone()));
    ids
}

fn build_url(host_name: &str, secids: &[String]) -> String {
    format!(
        "https://{host_name}/api/qt/ulist.np/get?fltt=2&invt=2&secids={}&fields={FIELDS}",
        secids.join(",")
    )
}

fn hosts(state: &State) -> [&'static str; 2] {
    if state.config.source == "delayed" {
        ["push2delay.eastmoney.com", "push2.eastmoney.com"]
    } else {
        ["push2.eastmoney.com", "push2delay.eastmoney.com"]
    }
}

fn label(host_name: &str) -> &'static str {
    if host_name.starts_with("push2delay") { "东财备源" } else { "东财主源" }
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
    if !force && now.saturating_sub(state.meta.last_fetch_at) < 55 {
        return Ok(());
    }
    let secids = build_secids(state);
    if secids.is_empty() {
        state.meta.last_error = Some("自选股为空".into());
        let _ = state.save_meta();
        crate::publish(state);
        return Ok(());
    }

    let task = host::begin_task("刷新行情", true);
    task.report(0, Some(3), Some("请求行情"));

    let mut outcome: Option<(&'static str, Vec<Quote>)> = None;
    let mut last_error: Option<String> = None;
    for host_name in hosts(state) {
        if task.is_cancelled() {
            return Ok(());
        }
        let request = HttpRequest {
            timeout_ms: Some(4000),
            ..HttpRequest::get(&build_url(host_name, &secids))
                .header("Accept", "application/json")
        };
        match host::fetch(&request) {
            Ok(response) if response.status == 200 => {
                match crate::quote::parse_eastmoney(&response.body) {
                    Ok(quotes) => {
                        outcome = Some((host_name, quotes));
                        break;
                    }
                    Err(e) => last_error = Some(format!("{} 解析失败：{e}", label(host_name))),
                }
            }
            Ok(response) => {
                last_error = Some(format!("{} HTTP {}", label(host_name), response.status))
            }
            Err(Error::Denied(cap)) => {
                last_error = Some(format!("网络权限未授予（{cap}）"));
            }
            Err(Error::Cancelled) => return Ok(()),
            Err(e) => last_error = Some(format!("{}：{e}", label(host_name))),
        }
    }

    match outcome {
        Some((host_name, quotes)) => {
            state.snapshot = Some(Snapshot {
                fetched_at: now,
                source: label(host_name).to_string(),
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
    use openidea_plugin::{HttpResponse, mock::MockHost};

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
        ])
    }

    #[test]
    fn builds_one_batched_request_with_indices() {
        let h = mock();
        let mut s = State::load();
        s.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: GOOD.as_bytes().to_vec() }));
        sync(&mut s, true).unwrap();
        let requests = h.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.starts_with("https://push2.eastmoney.com/"));
        assert!(requests[0].url.contains("1.600519"));
        assert!(requests[0].url.contains("1.000001"));
        assert_eq!(requests[0].timeout_ms, Some(4000));
        assert_eq!(s.snapshot.as_ref().unwrap().quotes.len(), 2);
        assert_eq!(s.meta.failures, 0);
    }

    #[test]
    fn falls_back_to_the_secondary_host() {
        let h = mock();
        let mut s = State::load();
        s.watch.push(WatchItem { symbol: "1.600519".into(), ..Default::default() });
        h.on_fetch(|req| {
            if req.url.contains("push2delay") {
                Ok(HttpResponse { status: 200, headers: vec![], body: GOOD.as_bytes().to_vec() })
            } else {
                Ok(HttpResponse { status: 503, headers: vec![], body: vec![] })
            }
        });
        sync(&mut s, true).unwrap();
        assert_eq!(h.requests().len(), 2);
        assert_eq!(s.snapshot.as_ref().unwrap().source, "东财备源");
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
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: b"not json".to_vec() }));
        sync(&mut s, true).unwrap();
        assert_eq!(s.snapshot.as_ref().unwrap().source, "旧缓存");
        assert!(s.status_text().contains("解析失败"));
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
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: GOOD.as_bytes().to_vec() }));
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
            above: Some(1500.0),
            ..Default::default()
        });
        h.on_fetch(|_| Ok(HttpResponse { status: 200, headers: vec![], body: GOOD.as_bytes().to_vec() }));
        sync(&mut s, true).unwrap();
        assert_eq!(h.notifications_with_actions().len(), 1);
        sync(&mut s, true).unwrap();
        assert_eq!(h.notifications_with_actions().len(), 1);
    }
}
