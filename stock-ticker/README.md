# Stock Ticker (OpenIDEA plugin)

A-share quotes in a single native side tool window. API 1.3 (`engines.openidea = "^1.3"`).

Status: **M0–M5 implemented and validated**. 55 unit tests pass, and
`openidea-plugin build` / `check` both report `ok`. The folder is **linked**
into the local OpenIDEA plugins directory; open Settings › Plugins and Reload
to see the single 行情 tab (visual verification still needs a human). See
[`../DESIGN.md`](../DESIGN.md) for the full design and execution records.

> The view uses a native `Body::sortable_table`, not a sandboxed HTML frame.
> Native views re-render through `host::refresh_view` — the reliable, documented
> path. The earlier HTML-frame approach failed to reload in some IDE builds, so
> clicks and auto-refresh never reached the UI; the native table fixes both.

## Features

- **4 columns only**: `代码 | 名称 | 现价 | 涨跌%` (plain text; no red/green coloring).
- **行情** — a single native sortable-table tab: clickable headers to re-sort;
  double-click (or Enter) a row to open its quote page; toolbar refresh / add /
  open / remove. Index rows are pinned on top when enabled.
- **添加支持模糊搜索**：输入完整代码（`600519`）直接添加；输入中文名
  （`东方`）、代码片段或拼音首字母（`dftx` → 东方通信）**并回车**后，
  **同一个工具窗原地切换成匹配结果列表**（不弹第二个对话框），双击或回车某行即添加，
  点「返回」回到行情表；无结果提示「未找到匹配」。调用东方财富联想接口
  `searchapi.eastmoney.com/api/suggest/get`，结果只保留 A 股
  （`Classify=AStock` + 合法 secid）。
  > API 1.3 的 `host::input` 是纯文本框、没有 type-ahead/suggestions，只在提交时
    返回一次文本；`host::pick` 的列表也没有过滤输入框。因此**无法逐字符实时下拉**，
    关键词必须先提交一次，这是宿主控件的硬限制。
- East Money `push2.eastmoney.com` batched quotes with
  `push2delay.eastmoney.com` fallback; one HTTPS request for the whole
  watchlist + indices; `begin_task` + 4 s timeout; graceful degradation when
  `net:*` is revoked.
- Watchlist, last snapshot, sort order and alert de-duplication survive idle
  reclaim and IDE restarts via `host::storage`.
- **Faster price updates without breaking the 60 s floor**: `on-schedule:60`
  stays the guaranteed background timer, and `on-event:editor-focused` /
  `on-event:file-saved` act as a **15 s-throttled activity heartbeat** — while
  you are editing, quotes can refresh up to every 15 s; when idle it stays 60 s.
  The IDE hard-rejects `on-schedule` periods below 60 s
  (`ActivationEvent::parse`), so a literal 15 s timer is impossible. Toolbar /
  `Ctrl+Alt+S` manual refresh, status bar item, price alerts (once per UTC day),
  settings page with a "测试连接" action.
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
├─ tests/fixtures/         # real East Money response snapshot (contract test)
├─ src/
│  ├─ lib.rs               # Plugin + Views, publish(), row-index resolution
│  ├─ model.rs             # pure domain model, normalize, ordered_rows
│  ├─ quote.rs             # East Money quote JSON parsing
│  ├─ search.rs            # fuzzy symbol/name/pinyin suggestion lookup
│  ├─ state.rs             # storage + live settings
│  ├─ sync.rs              # fetch, backup source, alerts
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
the two local ones; revoking a network grant degrades gracefully instead of
breaking the view.

| Capability | Risk | Why this plugin needs it | If revoked |
| --- | --- | --- | --- |
| `storage` (required) | low | Persist the watchlist, last quote snapshot, sort order and alert de-duplication across IDE restarts and the 5-minute idle reclaim | plugin cannot load |
| `events` (required) | low | Receive the 60 s `Scheduled` tick and the editor/file events used by the 15 s activity heartbeat | no automatic refresh |
| `net:push2.eastmoney.com` (optional) | medium | Primary batched A-share quote endpoint: one HTTPS request per refresh for the whole watchlist plus indices | falls back to the delayed host |
| `net:push2delay.eastmoney.com` (optional) | medium | Backup source with the same schema when the primary fails or is denied | cached snapshot shown with a stale status |
| `net:searchapi.eastmoney.com` (optional) | medium | Fuzzy lookup when adding by Chinese name, pinyin initials (`dftx`) or a partial code | adding by full code still works; name search reports the missing grant |

No `process:*`, `fs.*`, `secrets`, `project.write`, `ide.commands` or clipboard
access is used. All requests are read-only HTTPS with a 4 s timeout; no data
leaves the machine other than these three quote/search requests.

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
| Batched quotes (primary) | `push2.eastmoney.com` | `net:push2.eastmoney.com` |
| Batched quotes (backup) | `push2delay.eastmoney.com` | `net:push2delay.eastmoney.com` |
| Fuzzy symbol search | `searchapi.eastmoney.com` | `net:searchapi.eastmoney.com` |

All three are **optional** capabilities. If the search grant is missing, the add
command reports an actionable message instead of failing silently. Note that the
quotes list endpoint (`clist/get`) does **not** support text/pinyin search — it
ignores `keyword` and returns a generic first page, so it must not be used for
symbol lookup.

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
SVGs. **Signing and marketplace publication are not done** — they need a
publisher key and explicit approval (see DESIGN.md §6.6).
