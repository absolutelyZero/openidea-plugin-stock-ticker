# Proposal

## Why

0.2.2 在「面板可见、用户静止」时无法周期刷新：宿主只在插件 `Inactive` 时投递 `on-schedule`（`v0.1.39` 的 `ide-host/plugins/mod.rs:2106` 要求 `status == Inactive`），存活实例只重置计时器。实测行情时钟会冻结，刷新实际靠零散事件，间隔不可控。

参考插件 `jiusi666/openidea-stock-watch` 证明另一条路可行：它的 `activation` 里没有 `on-schedule`，视图是 `views[].web = "web/index.html"`（WebView），页面里用 `setInterval`（默认 10 秒，可调 3–300）经 `openidea.postMessage` 让插件取数并回推。定时器属于网页环境，不受宿主调度限制。

本变更把行情面板改为 WebView 视图，实现真正的秒级自动刷新；同时取代因无可用 WSS 端点而终止的 `realtime-quotes-websocket`。

## What Changes

- **BREAKING（UI 实现）**：`[[contributes.views]]` 由原生 `sortable_table` 改为 `web = "web/index.html"`；`render` 返回占位消息，界面由页面绘制。
- 页面 JS 持有刷新定时器：默认 **10 秒**，可配置 **3–300 秒**；通过 `openidea.postMessage` 请求刷新，插件用 `host::post_view_message` 回推快照。
- Rust 侧保持被动：`on_message` 收到 `refresh` → 现有 `qt.gtimg.cn` 取数 → 序列化快照回推；`storage`、预警、状态栏逻辑不变。
- `activation` 去掉 `on-schedule:60`（不再依赖；避免无效激活），保留 `on-view:watchlist`。
- `engines.openidea` 先验证 1.3 是否支持 `views[].web`；不支持则升 `^1.4`（IDE ≥ 0.1.40）。
- 版本 0.3.0，重新签名并登记 registry。
- 保持现有 4 列（代码/名称/现价/涨跌%）与「无红绿」约定，不改数据源与命令集合。

## Capabilities

### New Capabilities
- `ui/webview-watchlist`: WebView 行情面板的渲染、页面内定时刷新节奏，以及页面与插件之间的消息协议。

### Modified Capabilities
（无。`market-data/quotes` 仍属于未归档的 `switch-to-tencent-quotes`，本变更不改其取数需求。）

## Impact

- 新增 `stock-ticker/web/index.html`（内联 CSS/JS，无外部资源、无网络访问），预计 60–90 KB。
- `stock-ticker/src/lib.rs`：`render` 改为占位、新增 `on_message`/`post_view_message` 推送、快照序列化。
- `stock-ticker/plugin.toml`：`views[].web`、activation 调整、可能的 engines 升级。
- 包体积增加（HTML 随包分发，不进 wasm）；Rust 侧逻辑基本不变。
- 测试方式变化：Rust 侧测消息协议与序列化；页面交互需人工/浏览器验证。
- 不在范围：更换数据源、WebSocket、红绿配色、新增命令。
- 待确认：1.3 是否支持 `views[].web`；WebView 隐藏时定时器是否被宿主节流。
