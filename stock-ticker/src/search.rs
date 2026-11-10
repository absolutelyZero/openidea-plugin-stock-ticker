//! Fuzzy stock search via East Money's public suggestion API. Accepts a code
//! fragment, a Chinese name or pinyin initials (e.g. `dftx` -> 东方通信) and
//! returns `(symbol, name)` candidates for the add command.
//!
//! The old implementation used `push2.eastmoney.com/api/qt/clist/get`, which is
//! a *quotes list* endpoint: it silently ignores `keyword` and returns the first
//! page of whatever `fs` selects, so searches produced unrelated stocks. The
//! suggestion endpoint is the one that actually does name/pinyin/code matching.

use openidea_plugin::{Error, HttpRequest, host};

pub const SEARCH_HOST: &str = "searchapi.eastmoney.com";
const SEARCH_COUNT: u32 = 20;

/// `(symbol, name)` pairs matching `keyword`. The symbol is East Money's
/// `QuoteID` (`1.600519` / `0.000001`), the same secid format the quote fetch
/// and `model::normalize` use.
pub fn search_symbols(keyword: &str) -> Result<Vec<(String, String)>, Error> {
    let kw = keyword.trim();
    if kw.is_empty() {
        return Ok(Vec::new());
    }
    // `type=14` is the all-market suggestion set; no token is required.
    let url = format!(
        "https://{SEARCH_HOST}/api/suggest/get?input={}&type=14&count={SEARCH_COUNT}",
        urlencode(kw)
    );
    let request = HttpRequest {
        timeout_ms: Some(4000),
        ..HttpRequest::get(&url).header("Accept", "application/json")
    };
    let response = host::fetch(&request).map_err(|e| match e {
        // Make a missing optional grant actionable instead of a generic failure.
        Error::Denied(_) => Error::Invalid(format!(
            "股票搜索需要网络权限，请在插件权限中授权 net:{SEARCH_HOST}"
        )),
        other => Error::Invalid(format!("搜索行情源不可达：{other}")),
    })?;
    if response.status != 200 {
        return Err(Error::Invalid(format!("搜索行情源返回 HTTP {}", response.status)));
    }
    parse_search(&response.body)
}

fn parse_search(bytes: &[u8]) -> Result<Vec<(String, String)>, Error> {
    // Unparseable or empty bodies mean "no matches" rather than a hard error,
    // so the add command can answer with a friendly "未找到匹配".
    let root: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(data) = root
        .get("QuotationCodeTable")
        .and_then(|t| t.get("Data"))
        .and_then(|d| d.as_array())
    else {
        // A no-match response carries `"Data": null`.
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in data {
        // The same endpoint also returns US/HK stocks, funds and indices for the
        // same keyword (e.g. input `dftx` also matches a NASDAQ ticker).
        if v.get("Classify").and_then(|x| x.as_str()) != Some("AStock") {
            continue;
        }
        let Some(quote_id) = v.get("QuoteID").and_then(|x| x.as_str()) else { continue };
        if !valid_a_share_quote_id(quote_id) {
            continue;
        }
        let name = v.get("Name").and_then(|x| x.as_str()).unwrap_or("").to_string();
        out.push((quote_id.to_string(), name));
    }
    Ok(out)
}

/// QuoteIDs for A-shares are `1.600519` / `0.000001`; reject everything else so
/// the watchlist never receives an unusable secid.
fn valid_a_share_quote_id(id: &str) -> bool {
    let Some((market, code)) = id.split_once('.') else { return false };
    (market == "1" || market == "0") && code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::{HttpResponse, mock::MockHost};

    // Shape captured from the live endpoint on 2026-11-10 (fields trimmed to
    // the ones the parser reads). Includes a US stock to prove filtering.
    const SUGGEST: &str = r#"{"QuotationCodeTable":{"Data":[
        {"Code":"600278","Name":"东方创业","PinYin":"DFCY","Classify":"AStock","QuoteID":"1.600278"},
        {"Code":"600776","Name":"东方通信","PinYin":"DFTX","Classify":"AStock","QuoteID":"1.600776"},
        {"Code":"000001","Name":"平安银行","PinYin":"PAYH","Classify":"AStock","QuoteID":"0.000001"},
        {"Code":"DFTX","Name":"Definium Therapeutics Inc","Classify":"UsStock","QuoteID":"105.DFTX"},
        {"Code":"000001","Name":"上证指数","Classify":"Index","QuoteID":"1.000001"}
    ],"Status":0,"Message":"成功","TotalCount":5}}"#;

    fn mock() -> MockHost {
        MockHost::install().with_capabilities(&["net:searchapi.eastmoney.com"])
    }

    #[test]
    fn parses_live_shape_and_keeps_only_a_shares() {
        let r = parse_search(SUGGEST.as_bytes()).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], ("1.600278".into(), "东方创业".into()));
        assert_eq!(r[1], ("1.600776".into(), "东方通信".into()));
        assert_eq!(r[2], ("0.000001".into(), "平安银行".into()));
        // US stock (105.DFTX) and index (Classify=Index) must be filtered out.
        assert!(!r.iter().any(|(s, _)| s == "105.DFTX"));
    }

    #[test]
    fn no_match_null_data_and_malformed_bodies_are_empty() {
        for body in [
            "not json",
            "{}",
            r#"{"QuotationCodeTable":{"Data":null,"Status":0,"TotalCount":0}}"#,
            r#"{"QuotationCodeTable":{"Data":[]}}"#,
        ] {
            assert!(parse_search(body.as_bytes()).unwrap().is_empty(), "should be empty: {body}");
        }
    }

    #[test]
    fn quote_ids_must_be_six_digit_a_share_secids() {
        assert!(valid_a_share_quote_id("1.600519"));
        assert!(valid_a_share_quote_id("0.000001"));
        assert!(!valid_a_share_quote_id("105.DFTX"));
        assert!(!valid_a_share_quote_id("600519"));
        assert!(!valid_a_share_quote_id("1.60051"));
        assert!(!valid_a_share_quote_id("2.600519"));
    }

    #[test]
    fn chinese_name_query_hits_the_suggest_endpoint() {
        let h = mock();
        h.on_fetch(|req| {
            assert!(req.url.starts_with("https://searchapi.eastmoney.com/api/suggest/get?"), "url: {}", req.url);
            assert!(req.url.contains("input=%E4%B8%9C%E6%96%B9"), "keyword must be urlencoded: {}", req.url);
            assert!(req.url.contains("clist/get") == false, "must NOT be the clist endpoint");
            Ok(HttpResponse { status: 200, headers: vec![], body: SUGGEST.as_bytes().to_vec() })
        });
        let got = search_symbols("东方").unwrap();
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn pinyin_initials_are_forwarded_unchanged() {
        let h = mock();
        h.on_fetch(|req| {
            assert!(req.url.contains("input=dftx"), "url: {}", req.url);
            Ok(HttpResponse { status: 200, headers: vec![], body: SUGGEST.as_bytes().to_vec() })
        });
        assert_eq!(search_symbols("dftx").unwrap().len(), 3);
    }

    #[test]
    fn missing_grant_gives_an_actionable_error() {
        let _h = MockHost::install().with_capabilities(&["storage"]);
        let err = search_symbols("东方").unwrap_err();
        assert!(format!("{err}").contains("net:searchapi.eastmoney.com"), "err: {err}");
    }
}
