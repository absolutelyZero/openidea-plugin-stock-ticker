# Stock Ticker (OpenIDEA plugin)

A-share quotes in a single WebView side panel. API 1.3 (`engines.openidea = "^1.3"`).

Status: **M0–M5 implemented and validated**. 34 unit tests pass, and
`openidea-plugin build` / `check` both report `ok`. The folder is **linked**
into the local OpenIDEA plugins directory; open Settings › Plugins and Reload
to see the single 行情 tab (visual verification still needs a human). See
[`../DESIGN.md`](../DESIGN.md) for the full design and execution records.

> The view is a WebView page (`web/index.html`, `[[contributes.views]] web = …`),
> not a native table and not a `Body::Html` frame. The page owns the refresh
> timer and talks to the plugin through `openidea.postMessage` /
> `host::post_view_message`; the plugin fetches, formats and pushes snapshots.

## Features

- **4 columns only**: `代码 | 名称 | 现价 | 涨跌%` (plain text; direction is shown
  with `▲▼` / `+−`, not by color). The right-hand intraday chart uses the A-share
  convention (red = up, green = down) together with a `▲▼` text summary, and its
  X axis always covers the full 09:30–15:00 session.
- **行情** — a WebView tab: clickable headers re-sort, click selects a row,
  double-click opens its quote page, toolbar has refresh / add / remove.
- **Hover-linked intraday chart**: hovering (or keyboard-focusing) a row shows
  that stock's current-day minute chart in the right pane; switching rows
  cancels stale responses, and moving the mouse out of the list (or tabbing
  away) clears the pane back to its placeholder.
  Loading / no-data / failure states are displayed, including an actionable
  prompt when `net:web.ifzq.gtimg.cn` is not granted.
- **Drag to reorder**: drag any row to a new position to define your own
  watchlist order. The dragged order is persisted (`watch.v1`) and survives
  refresh, reload and IDE restarts. The two ordering modes are mutually
  exclusive: dragging switches to **custom order** (no sort arrow); clicking a
  header switches to **column sort**, which is only a transient view and never
  rewrites the stored watchlist order.
- **添加支持模糊搜索**：输入完整代码（`600519`）直接添加；输入中文名、代码片段或
  拼音首字母（`gzmt` → 贵州茅台）后，单个匹配直接添加，多个匹配弹出选择框。
  调用腾讯检索接口 `proxy.finance.qq.com/ifzqgtimg/appstock/smartbox/search/get`，
  只保留 A 股（`data.stock` 中 `GP-A`）。
  > `host::input` 仍是纯文本框（无 type-ahead），关键词需提交一次；这是宿主控件限制。
- Tencent `qt.gtimg.cn` batched quotes: one `q=sh600519,sz000001,…` request for
  the whole watchlist + indices, `~`-separated GBK text; `begin_task` + 4 s
  timeout; graceful degradation when `net:*` is revoked.
- Watchlist, last snapshot, sort order and alert de-duplication survive idle
  reclaim and IDE restarts via `host::storage`.
- **Refresh timer lives in the page**: the WebView runs `setInterval` (default
  **10 s**, configurable **3–300 s** in the toolbar) and posts a `refresh`
  message; the plugin fetches and pushes a snapshot back. This is why the panel
  refreshes while visible — the host only delivers `on-schedule` to **inactive**
  plugins (`ide-host/plugins/mod.rs` requires `status == Inactive`), so a
  schedule cannot drive a visible panel. Toolbar / `Ctrl+Alt+S` manual refresh,
  status bar item, price alerts (once per UTC day), settings page with a
  "测试连接" action. The footer shows the quote clock (`HH:MM:SS`).
  > A table→table `refresh_view` keeps row selection and scroll position —
    `TableBody` holds them in component-local state (`PluginView.tsx:465–467`)
    and React reuses the component. So faster refreshes do **not** break
    "选中行即删". The heartbeat is skipped while the search result list is up.
- Removing uses the currently selected table row plus a confirm dialog; no
  second picker when a row is selected.

## Layout

```
stock-ticker/
├─ plugin.toml             # contributions + capabilities
├─ Cargo.toml              # standalone crate, cdylib for wasm32-wasip2
├─ settings/general.json   # settings page schema
├─ web/index.html          # the panel: table, toolbar, refresh timer
├─ src/
│  ├─ lib.rs               # Plugin + Views, message protocol, snapshot push
│  ├─ model.rs             # pure domain model, normalize, ordered_rows
│  ├─ minute.rs            # intraday JSON parsing, fetch, error classification
│  ├─ quote.rs             # Tencent GBK quote parsing
│  ├─ search.rs            # fuzzy symbol/name/pinyin lookup (Tencent JSON)
│  ├─ state.rs             # storage + live settings
│  ├─ sync.rs              # fetch, alerts
└─ ../vendor/openidea-plugin/   # SDK vendored from tag v0.1.39 (API 1.3.0)
```

The SDK is vendored by path (the documented `--sdk` equivalent) so the build
does not depend on a git checkout of the whole IDE repository.

## Commands

```sh
cargo test                       # 55 native tests against the SDK mock host
openidea-plugin build            # -> dist/plugin.wasm, then check
openidea-plugin check            # validate manifest + component
openidea-plugin link .           # load this folder in the IDE (already linked)
openidea-plugin links            # list linked folders
openidea-plugin unlink .         # remove the link (rollback)
```

## Capabilities and why

The plugin requests only what its features need. All grants are optional except
the local storage one; revoking a network grant degrades gracefully instead of
breaking the view.

| Capability | Risk | Why this plugin needs it | If revoked |
| --- | --- | --- | --- |
| `storage` (required) | low | Persist the watchlist, last quote snapshot, sort order, refresh interval and alert de-duplication across IDE restarts and the 5-minute idle reclaim | plugin cannot load |
| `net:qt.gtimg.cn` (optional) | medium | Batched A-share quotes: one HTTPS request per refresh for the whole watchlist plus indices | no quotes; cached snapshot + an explicit error is shown |
| `net:proxy.finance.qq.com` (optional) | medium | Fuzzy lookup when adding by Chinese name, pinyin initials (`gzmt`) or a partial code | adding by full code still works; name search reports the missing grant |
| `net:web.ifzq.gtimg.cn` (optional, new in 0.4.0) | medium | Current-day minute series for the hover-linked intraday chart (one symbol at a time, cached for 60 s) | quotes and all list interactions keep working; the chart shows an actionable authorization prompt |

No `process:*`, `fs.*`, `secrets`, `project.write`, `ide.commands` or clipboard
access is used. All requests are read-only HTTPS with a 4 s timeout; no data
leaves the machine other than these quote/search/minute requests.

## Building from source

`dist/plugin.wasm` is **not** checked in — it is built from the Rust sources in
`src/` and the vendored SDK in `../vendor/openidea-plugin`:

```sh
cargo test                                   # 55 native tests against the SDK mock host
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
# or, equivalently, the official CLI (which then runs check):
openidea-plugin build
```

The published `.oiplugin` is produced from exactly these sources with
`openidea-plugin pack`; it contains no obfuscated or minified code.

## Network endpoints

| Purpose | Host | Capability |
|---|---|---|
| Batched quotes | `qt.gtimg.cn` | `net:qt.gtimg.cn` |
| Fuzzy symbol search | `proxy.finance.qq.com` | `net:proxy.finance.qq.com` |
| Intraday minute chart | `web.ifzq.gtimg.cn` | `net:web.ifzq.gtimg.cn` |

All three are **optional** capabilities. If the search or minute grant is
missing, the affected feature reports an actionable message instead of failing
silently. Quotes are `~`-separated GBK text decoded to UTF-8 on the plugin side;
the search and minute endpoints are UTF-8 JSON (`data.stock` / `data.<code>.data`).
The minute endpoint is an unofficial public interface with no availability
guarantee; failures never overwrite the quote list.

## Plugin icon

`icon.svg` (light theme) and `icon_dark.svg` (dark theme) are declared in
`plugin.toml`. Both are plain-geometry SVGs: a deep-blue rounded tile, four
rising volume bars, and a red upward price line with an end dot (red = up, the
A-share convention). Each is 925 B — well under the IDE's 16 KiB icon cap — and
uses only sanitizer allow-listed elements/attributes (`svg`, `rect`, `line`,
`path`, `circle`, presentation attributes), with no scripts, external
references, `url()`, gradients or `foreignObject`. `icon` requires plugin API
≥ 1.2; this plugin declares `^1.3`.

An unsigned local verification package was built and confirmed to contain both
SVGs. Version 0.2.0 is built and signed with the publisher key; marketplace
publication is tracked separately (see DESIGN.md §6.7).
