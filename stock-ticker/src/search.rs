//! Fuzzy stock search via Tencent's JSON smartbox endpoint. Accepts a code
//! fragment, a Chinese name or pinyin initials (e.g. `gzmt` -> 贵州茅台) and
//! returns `(symbol, name)` candidates for the add command.
//!
//! `/s3/` on `smartbox.gtimg.cn` is the lighter text endpoint, but it only
//! returns ETFs for broad Chinese words like 银行 (and nothing for `t=gp-a`),
//! so name search uses the JSON endpoint whose `data.stock` lists up to 50
//! instruments with a `GP-A` type tag.

use openidea_plugin::{Error, HttpRequest, host};

pub const SEARCH_HOST: &str = "proxy.finance.qq.com";

/// `(symbol, name)` pairs matching `keyword`. The symbol is the `market.code`
/// secid the quote fetch and `model::normalize` use.
pub fn search_symbols(keyword: &str) -> Result<Vec<(String, String)>, Error> {
    let kw = keyword.trim();
    if kw.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "https://{SEARCH_HOST}/ifzqgtimg/appstock/smartbox/search/get?q={}",
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
    let Some(stocks) = root
        .get("data")
        .and_then(|d| d.get("stock"))
        .and_then(|s| s.as_array())
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in stocks {
        let Some(fields) = item.as_array() else { continue };
        let Some(market) = fields.first().and_then(|v| v.as_str()) else { continue };
        let Some(code) = fields.get(1).and_then(|v| v.as_str()) else { continue };
        let Some(name) = fields.get(2).and_then(|v| v.as_str()) else { continue };
        // `[4]` is the instrument type: `GP-A` for A-shares, `GP` for HK/US
        // listings, `ETF` for funds.
        let Some(kind) = fields.get(4).and_then(|v| v.as_str()) else { continue };
        if kind != "GP-A" {
            continue;
        }
        let market = match market {
            "sh" => "1",
            "sz" | "bj" => "0",
            _ => continue,
        };
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        out.push((format!("{market}.{code}"), name.to_string()));
    }
    Ok(out)
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

    // Shape captured from the live endpoint on 2026-11-16 (a few of the 50
    // `stock` rows, including the HK/US/ETF rows the parser must drop).
    const SEARCH: &str = r#"{"code":0,"msg":"","data":{"sector":[["pt","01801780","银行","","BK-HY-1"]],"stock":[
        ["sh","601988","中国银行","","GP-A"],
        ["hk","03988","中国银行","","GP"],
        ["sz","002142","宁波银行","","GP-A"],
        ["us","BACHY.PS","中国银行","","GP"],
        ["sh","512800","银行ETF华宝","yhetfhb","ETF"]
    ]}}"#;

    fn mock() -> MockHost {
        MockHost::install().with_capabilities(&["net:proxy.finance.qq.com"])
    }

    #[test]
    fn parses_live_shape_and_keeps_only_a_shares() {
        let r = parse_search(SEARCH.as_bytes()).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], ("1.601988".into(), "中国银行".into()));
        assert_eq!(r[1], ("0.002142".into(), "宁波银行".into()));
        assert!(!r.iter().any(|(s, _)| s.contains("BACHY")));
    }

    #[test]
    fn no_match_and_malformed_bodies_are_empty() {
        for body in [
            "not json",
            "{}",
            r#"{"data":{}}"#,
            r#"{"data":{"stock":[]}}"#,
            r#"{"code":1,"msg":"error"}"#,
        ] {
            assert!(parse_search(body.as_bytes()).unwrap().is_empty(), "should be empty: {body}");
        }
    }

    #[test]
    fn query_hits_the_json_endpoint() {
        let h = mock();
        h.on_fetch(|req| {
            assert!(
                req.url.starts_with(
                    "https://proxy.finance.qq.com/ifzqgtimg/appstock/smartbox/search/get?q="
                ),
                "url: {}",
                req.url
            );
            assert!(req.url.contains("%E9%93%B6%E8%A1%8C"), "keyword must be urlencoded: {}", req.url);
            Ok(HttpResponse { status: 200, headers: vec![], body: SEARCH.as_bytes().to_vec() })
        });
        let got = search_symbols("银行").unwrap();
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn pinyin_initials_are_forwarded_unchanged() {
        let h = mock();
        h.on_fetch(|req| {
            assert!(req.url.contains("q=gzmt"), "url: {}", req.url);
            Ok(HttpResponse { status: 200, headers: vec![], body: SEARCH.as_bytes().to_vec() })
        });
        assert_eq!(search_symbols("gzmt").unwrap().len(), 2);
    }

    #[test]
    fn missing_grant_gives_an_actionable_error() {
        let _h = MockHost::install().with_capabilities(&["storage"]);
        let err = search_symbols("银行").unwrap_err();
        assert!(format!("{err}").contains("net:proxy.finance.qq.com"), "err: {err}");
    }
}
