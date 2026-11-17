//! Tencent `qt.gtimg.cn` response parser. The endpoint answers one
//! `v_<market><code>="…"` statement per symbol in a `~`-separated,
//! GBK-encoded body. Everything is optional: a suspended symbol still yields a
//! row, and a malformed body never overwrites a good snapshot.

use crate::model::Quote;
use encoding_rs::GBK;

/// Field positions confirmed against a live response on 2026-11-16.
const F_NAME: usize = 1;
const F_CODE: usize = 2;
const F_PRICE: usize = 3;
const F_PREV_CLOSE: usize = 4;
const F_OPEN: usize = 5;
const F_VOLUME: usize = 6;
const F_TIME: usize = 30;
const F_CHANGE: usize = 31;
const F_CHANGE_PCT: usize = 32;
const F_HIGH: usize = 33;
const F_LOW: usize = 34;
const F_TURNOVER_WAN: usize = 37;
const MIN_FIELDS: usize = 35;

fn num(field: Option<&str>) -> Option<f64> {
    let field = field?.trim();
    if field.is_empty() || field == "-" {
        return None;
    }
    field.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `sh`/`sz`/`bj` → the `market.code` secid the rest of the plugin uses.
fn secid(prefix: &str, code: &str) -> Option<String> {
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let market = match prefix {
        "sh" => "1",
        "sz" => "0",
        // East Money routes Beijing listings through the 0. channel.
        "bj" => "0",
        _ => return None,
    };
    Some(format!("{market}.{code}"))
}

/// `yyyyMMddHHmmss` in Beijing time → UTC epoch seconds.
fn ts_of(field: Option<&str>) -> Option<i64> {
    let raw = field?.trim();
    if raw.len() != 14 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let y: i64 = raw[0..4].parse().ok()?;
    let mo: i64 = raw[4..6].parse().ok()?;
    let d: i64 = raw[6..8].parse().ok()?;
    let h: i64 = raw[8..10].parse().ok()?;
    let mi: i64 = raw[10..12].parse().ok()?;
    let s: i64 = raw[12..14].parse().ok()?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3_600 + mi * 60 + s - 8 * 3_600)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse a response body. Returns an error when the frame is unusable; the
/// caller then keeps its previous snapshot.
pub fn parse_tencent(bytes: &[u8]) -> Result<Vec<Quote>, String> {
    let (text, _, _) = GBK.decode(bytes);
    let mut quotes = Vec::new();
    for statement in text.split(';') {
        let statement = statement.trim();
        let Some((key, value)) = statement.split_once('=') else { continue };
        let Some(prefix) = key.trim().strip_prefix("v_").and_then(|k| k.get(0..2)) else {
            continue;
        };
        let fields: Vec<&str> = value.trim().trim_matches('"').split('~').collect();
        if fields.len() < MIN_FIELDS {
            continue;
        }
        let Some(symbol) = secid(prefix, fields[F_CODE].trim()) else { continue };
        quotes.push(Quote {
            symbol,
            name: fields[F_NAME].trim().to_string(),
            price: num(fields.get(F_PRICE).copied()),
            prev_close: num(fields.get(F_PREV_CLOSE).copied()),
            open: num(fields.get(F_OPEN).copied()),
            high: num(fields.get(F_HIGH).copied()),
            low: num(fields.get(F_LOW).copied()),
            change: num(fields.get(F_CHANGE).copied()),
            change_pct: num(fields.get(F_CHANGE_PCT).copied()),
            volume: num(fields.get(F_VOLUME).copied()),
            turnover: num(fields.get(F_TURNOVER_WAN).copied()).map(|v| v * 10_000.0),
            ts: ts_of(fields.get(F_TIME).copied()),
        });
    }
    if quotes.is_empty() {
        return Err("no usable quotes in the Tencent response".into());
    }
    Ok(quotes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gbk(s: &str) -> Vec<u8> {
        let (bytes, _, _) = GBK.encode(s);
        bytes.into_owned()
    }

    /// Shape captured from the live endpoint on 2026-11-16 (fields kept in
    /// their original positions).
    fn body() -> Vec<u8> {
        gbk(concat!(
            "v_sh600519=\"1~贵州茅台~600519~1438.55~1435.26~1426.30~18370~10067~8302~",
            "1438.55~1~1438.54~1~1438.45~1~1438.44~1~1438.34~1~1439.00~3~1439.04~1~",
            "1439.20~1~1439.50~3~1439.55~1~~20261116113253~3.29~0.23~1445.60~1425.00~",
            "1438.55/18370/2640163153~18370~264016~0.15~21.89\";",
            "v_sz000001=\"51~平安银行~000001~12.16~12.07~12.06~1141378~596211~545167~",
            "12.16~1357~12.15~463~12.14~2918~12.13~1305~12.12~1582~12.17~707~12.18~3164~",
            "12.19~1947~12.20~4254~12.21~2292~~20261116113306~0.09~0.75~12.23~12.05~",
            "12.16/1141378/1387037682~1141378~138704~0.59~5.43\";"
        ))
    }

    #[test]
    fn parses_a_live_shape_response() {
        let q = parse_tencent(&body()).unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].symbol, "1.600519");
        assert_eq!(q[0].name, "贵州茅台");
        assert_eq!(q[0].price, Some(1438.55));
        assert_eq!(q[0].prev_close, Some(1435.26));
        assert_eq!(q[0].open, Some(1426.30));
        assert_eq!(q[0].change, Some(3.29));
        assert_eq!(q[0].change_pct, Some(0.23));
        assert_eq!(q[0].high, Some(1445.60));
        assert_eq!(q[0].low, Some(1425.00));
        assert_eq!(q[0].volume, Some(18_370.0));
        assert_eq!(q[0].turnover, Some(2_640_160_000.0));
        assert!(q[0].ts.is_some());
        assert_eq!(q[1].symbol, "0.000001");
        assert_eq!(q[1].name, "平安银行");
    }

    #[test]
    fn suspended_symbol_still_yields_a_row() {
        let b = gbk(concat!(
            "v_sh600519=\"1~停牌股~600519~0.00~10.00~0.00~0~0~0~0.00~0~0.00~0~0.00~0~",
            "0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~~20261116150000~0.00~0.00~",
            "0.00~0.00~0~0~0~0~0\";"
        ));
        let q = parse_tencent(&b).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].price, Some(0.0));
        assert_eq!(q[0].name, "停牌股");
    }

    #[test]
    fn index_rows_map_back_to_secids() {
        let b = gbk("v_sh000001=\"1~上证指数~000001~3620.00~3634.35~3611.54~277036290~0~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~0.00~0~~20261116113200~-14.35~-0.39~3623.12~3593.45~3620.00/277036290/421756737368~277036290~42175674~0.57~16.50~~3623.12~3593.45~0.82~569675.08~634958.76~0.00~-1~-1~1.24~0~3605.81~~~~~~42175673.7368~0.0000~0~ ~ZS~-8.79~-1.11~~~~4258.86~3574.27~-0.50~-2.97~-7.82~4865879337250~~11.04~-12.43~4865879337250~~~-9.28~-0.03~~CNY~0~~0.00~0~\";");
        let q = parse_tencent(&b).unwrap();
        assert_eq!(q[0].symbol, "1.000001");
        assert_eq!(q[0].name, "上证指数");
        assert_eq!(q[0].change_pct, Some(-0.39));
    }

    #[test]
    fn rejects_unusable_bodies() {
        for body in [
            b"not a quote".as_slice(),
            b"v_sh600519=\"1~x\"".as_slice(),
            b"".as_slice(),
        ] {
            assert!(parse_tencent(body).is_err(), "should reject");
        }
    }

    #[test]
    fn timestamp_conversion_is_sane() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert!(ts_of(Some("20261116113253")).is_some());
        assert!(ts_of(Some("20261316113253")).is_none());
        assert!(ts_of(Some("bad")).is_none());
        assert!(ts_of(None).is_none());
    }
}
