# Stock Ticker · A 股行情侧边栏

在 OpenIDEA 右侧边栏盯盘：自选股报价一眼看完，鼠标移到任意一行就能看到它当天的分时图。

- **四列自选行情** — `代码 | 名称 | 现价 | 涨跌%`。涨跌方向用 `▲▼` / `±` 文本表达，
  不靠颜色区分，色弱也能读。
- **悬停看分时** — 鼠标移入（或键盘聚焦）某一行，右侧即显示该股当日 09:30–15:00
  的走势：昨收虚线、最新价标记与数值摘要；移出列表即收起，切股不会残留上一只的图。
- **加股票不用记代码** — 输入 `600519`、`贵州茅台`、`茅台` 或拼音首字母 `gzmt`
  都能找到；命中多个时弹选择框。只收录 A 股。
- **拖拽排序** — 顺手拖成你的习惯顺序，刷新、重载、重启 IDE 都记得；点表头排序只是
  临时视图，不会改写你存下的顺序。
- **面板内自动刷新** — 默认 10 秒（可设 3–300 秒），另有工具栏按钮、`Ctrl+Alt+S`
  与状态栏入口；底部显示行情时间。
- **能力申请克制** — 只要自选存储与 3 个腾讯行情域名。任何一项网络授权被拒都只是
  降级提示（无行情 / 无法按名搜索 / 图表提示授权），面板不会坏掉。

> 数据来自腾讯财经公开接口（行情 / 检索 / 分时）。分时为非官方公开接口，不保证
> 长期可用；请求失败只提示错误并保留已有行情，不会清空你的列表。

## 技术说明

- 视图是一个 WebView 页面（`web/index.html`，`[[contributes.views]] web = …`），
  不是原生表格，也不是 `Body::Html`。刷新计时器在页面内，插件负责取数、格式化并
  推送快照；两者通过 `openidea.postMessage` / `host::post_view_message` 通信。
- API 1.3（`engines.openidea = "^1.3"`）；Rust 编译到 `wasm32-wasip2`，SDK 按路径
  vendor 在 `../vendor/openidea-plugin`，构建不依赖 IDE 仓库。

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

These two files are what the marketplace and Settings › Plugins show. The
**side rail** uses a separate icon: `[[contributes.views]].icon`, here
`view.svg`. It is the same motif redrawn for a 24 px box (three bars instead of
four, heavier strokes, transparent margin) so it stays readable at rail size.
The view icon takes no `icon-dark` — the IDE rejects that key — so it must read
correctly on both light and dark rails, which is why it carries its own tile.

An unsigned local verification package was built and confirmed to contain both
plugin SVGs plus `view.svg`. Version 0.4.5 is built and signed with the
publisher key; marketplace publication is tracked separately (see DESIGN.md
§6.7).

## Development status

**M0–M5 implemented and validated.** `cargo test` passes (55 tests) and
`openidea-plugin build` / `check` both report `ok`. The folder is **linked**
into the local OpenIDEA plugins directory — open Settings › Plugins and Reload
to see the single 行情 tab (visual verification still needs a human). See
[`../DESIGN.md`](../DESIGN.md) for the full design and execution records.
