//! East Money `ulist.np` response parser. Everything is optional: the public
//! endpoint sometimes returns `"-"` strings, nulls or a `diff` object instead
//! of an array. Malformed input must never overwrite a good cached snapshot.

use crate::model::Quote;
use serde_json::Value;

fn num(value: &Value, key: &str) -> Option<f64> {
    match value.get(key) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn int(value: &Value, key: &str) -> Option<i64> {
    value
        .get(key)
        .and_then(|v| v.as_i64().or_else(|| num(value, key).map(|f| f as i64)))
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn symbol_of(value: &Value) -> String {
    let code = text(value, "f12");
    match int(value, "f13") {
        Some(1) => format!("1.{code}"),
        Some(0) => format!("0.{code}"),
        _ => code,
    }
}

/// Parse a response body. Returns an error when the frame is unusable; callers
/// then keep their previous snapshot and try the backup source.
pub fn parse_eastmoney(bytes: &[u8]) -> Result<Vec<Quote>, String> {
    let root: Value = serde_json::from_slice(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    let data = root.get("data").ok_or_else(|| "missing data".to_string())?;
    if data.is_null() {
        return Err("data is null".into());
    }
    let diff = data.get("diff").ok_or_else(|| "missing data.diff".to_string())?;

    let items: Vec<&Value> = match diff {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by_key(|k| k.parse::<usize>().unwrap_or(usize::MAX));
            keys.into_iter().filter_map(|k| map.get(k)).collect()
        }
        _ => return Err("data.diff is neither an array nor an object".into()),
    };

    let quotes: Vec<Quote> = items
        .into_iter()
        .map(|v| Quote {
            symbol: symbol_of(v),
            name: text(v, "f14"),
            price: num(v, "f2"),
            prev_close: num(v, "f18"),
            open: num(v, "f17"),
            high: num(v, "f15"),
            low: num(v, "f16"),
            change: num(v, "f4"),
            change_pct: num(v, "f3"),
            volume: num(v, "f5"),
            turnover: num(v, "f6"),
            ts: int(v, "f124"),
        })
        .filter(|q| !q.symbol.is_empty())
        .collect();

    if quotes.is_empty() {
        return Err("no usable quotes in data.diff".into());
    }
    Ok(quotes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "rc": 0, "rt": 10,
      "data": { "total": 2, "diff": [
        {"f2":1500.0,"f3":1.23,"f4":18.2,"f5":12345,"f6":987654,
         "f12":"600519","f13":1,"f14":"贵州茅台","f15":1510.0,
         "f16":1488.0,"f17":1495.0,"f18":1481.8,"f124":1730000000},
        {"f2":"-","f3":null,"f4":"-","f12":"000001","f13":0,
         "f14":"平安银行","f18":"12.30","f124":1730000001}
      ]}
    }"#;

    #[test]
    fn parses_a_normal_response() {
        let q = parse_eastmoney(FIXTURE.as_bytes()).unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].symbol, "1.600519");
        assert_eq!(q[0].name, "贵州茅台");
        assert_eq!(q[0].price, Some(1500.0));
        assert_eq!(q[0].change_pct, Some(1.23));
        assert_eq!(q[0].prev_close, Some(1481.8));
        assert_eq!(q[0].ts, Some(1730000000));
    }

    #[test]
    fn dash_and_null_become_none() {
        let q = parse_eastmoney(FIXTURE.as_bytes()).unwrap();
        assert_eq!(q[1].symbol, "0.000001");
        assert_eq!(q[1].price, None);
        assert_eq!(q[1].change_pct, None);
        assert_eq!(q[1].prev_close, Some(12.30));
    }

    #[test]
    fn accepts_diff_as_an_object() {
        let body = r#"{"data":{"diff":{"1":{"f12":"600519","f13":1,"f14":"贵州茅台","f2":1500.0},"0":{"f12":"000001","f13":0,"f14":"平安银行","f2":12.3}}}}"#;
        let q = parse_eastmoney(body.as_bytes()).unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].symbol, "0.000001");
        assert_eq!(q[1].symbol, "1.600519");
    }

    #[test]
    fn rejects_unusable_frames() {
        for body in ["not json", "{}", r#"{"data":null}"#, r#"{"data":{"diff":[]}}"#] {
            assert!(parse_eastmoney(body.as_bytes()).is_err(), "should reject {body}");
        }
    }

    #[test]
    fn tolerates_missing_market_field() {
        let body = r#"{"data":{"diff":[{"f12":"600519","f14":"贵州茅台","f2":1500.0}]}}"#;
        let q = parse_eastmoney(body.as_bytes()).unwrap();
        assert_eq!(q[0].symbol, "600519");
    }

    /// Snapshot of a real `push2.eastmoney.com` response (2026-11-09).
    /// `fltt=2` returns true decimals — there is no /100 scaling.
    #[test]
    fn matches_the_live_endpoint_contract() {
        const LIVE: &str = include_str!("../tests/fixtures/eastmoney-live.json");
        let q = parse_eastmoney(LIVE.as_bytes()).unwrap();
        assert_eq!(q.len(), 3);
        assert_eq!(q[0].symbol, "1.600519");
        assert_eq!(q[0].price, Some(1442.17));
        assert_eq!(q[0].change_pct, Some(-0.75));
        assert_eq!(q[0].prev_close, Some(1453.0));
        // `000001` is both Ping An Bank and the SSE composite: f13 separates them.
        assert_eq!(q[1].symbol, "0.000001");
        assert_eq!(q[1].name, "平安银行");
        assert_eq!(q[2].symbol, "1.000001");
        assert_eq!(q[2].name, "上证指数");
        assert!(q.iter().all(|x| x.ts.is_some()));
    }
}
