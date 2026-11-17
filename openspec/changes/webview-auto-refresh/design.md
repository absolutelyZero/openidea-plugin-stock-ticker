# Design

## Context

0.2.2 使用原生 `Body::sortable_table`，刷新依赖宿主事件与 `on-schedule`；但宿主只在插件 `Inactive` 时投递 `on-schedule`（`ide-host/plugins/mod.rs:2106`），面板可见时无周期信号。

参考实现 `jiusi666/openidea-stock-watch`：`activation = ["on-view:watchlist"]`、`views[].web = "web/index.html"`、`engines = "^1.4"`；页面 `index.html:1587` 用 `setInterval`（`intervalSec` 默认 10，clamp 3–300），经 `window.openidea.postMessage` 让插件刷新，插件用 `host::post_view_message` 回推快照。

WebView 的约束（文档 `plugins/README.md:1649–1651`）：页面只能加载内联脚本/样式与 `data:` 图片，**无网络访问**、无 IDE API；与插件只通过 `openidea.postMessage` / `openidea.onMessage` 通信；根节点带 `data-theme` 与主题 CSS 变量。

## Goals / Non-Goals

**Goals:**
- 面板可见时按 3–300 秒（默认 10）稳定自动刷新。
- 保留现有数据获取、存储、预警与 4 列展示。
- 页面与插件的协议单一、可测。

**Non-Goals:**
- 不改数据源、不加 WebSocket、不做红绿配色。
- 不把业务逻辑搬进 JS：取数、持久化、预警仍在 Rust。

## Decisions

### D1：用 `views[].web`，不用 `Body::Html`
- **选择**：manifest 声明 `web = "web/index.html"`，`render` 返回占位 `View::message`。
- **理由**：`Body::Html` 是 render 返回的 body 帧（此前在本机不重绘、桥不通）；`views[].web` 是独立 WebView 页面，走 `post_view_message`/`ViewEvent::Message`，是参考插件验证过的路径。

### D2：定时器放在页面 JS
- **选择**：`setInterval`，间隔来自设置（默认 10，clamp 3–300）。
- **理由**：网页定时器不受宿主调度限制；插件保持无状态、被动响应。
- **备选**：`on-schedule`（仅在 Inactive 投递，已证不可用）。

### D3：Rust 侧保持被动
- **选择**：`on_message("refresh")` → 现有 `sync` 取数 → `post_view_message` 回推；`storage`/预警/状态栏不变。
- **理由**：最小改动，复用已验证的取数与降级逻辑。

### D4：消息协议
- **页面 → 插件**：`{"type":"refresh"}`、`{"type":"prefs","intervalSec":10}`、`{"type":"remove","symbol":"1.600519"}`、`{"type":"open","symbol":"1.600519"}`。
- **插件 → 页面**：`{"type":"snapshot","quotes":[…],"config":{"precision":2,"arrowStyle":true,"intervalSec":10},"source":"腾讯行情","clock":"13:42:56","ageSec":2,"error":null}`。
- **理由**：只有一种回推形态，页面按 `type` 分发；错误与新鲜度随快照一起下发，避免额外通道。

### D5：`engines` 先验证 1.3，必要时升 1.4
- **选择**：先用 1.3 SDK 加一个最小 web view 验证；若 1.3 不支持则升 `^1.4`（IDE ≥ 0.1.40，用户 0.1.45 满足）。
- **理由**：避免无谓抬高门槛。

### D6：activation 去掉 `on-schedule`
- **选择**：保留 `on-view:watchlist`，移除 `on-schedule:60`。
- **理由**：刷新由页面定时器驱动，`on-schedule` 只会带来无效激活。

## Risks / Trade-offs

- **WebView 隐藏时节流** → 页面不可见时浏览器可能降频；这是期望行为（省资源），恢复可见时立即补一次刷新。
- **消息体积** → 快照含 200 只以内报价，单条 JSON 约 20–40 KB；每次刷新一条，可接受。
- **格式逻辑重复** → 涨跌箭头/精度格式化在 JS 与 Rust 各有一份；统一由 Rust 下发格式化后的单元格，JS 只负责渲染，减少分叉。
- **包体积增加** → HTML 约 60–90 KB，随包分发；wasm 不变。
- **1.3 支持性未知** → D5 先验证；不支持则升 1.4，旧 IDE 无法安装新版本。

## Migration Plan

1. 0.3.0：WebView 视图 + 页面定时器；Rust 取数/存储/预警不变。
2. 回滚：代码 `git revert`；已发布包不可改，用户可退回 0.2.x。
3. 旧版 0.2.x 的原生表格行为保持不变。

## Open Questions

（无。1.3 支持性与隐藏节流都作为第 1 组验证项，不影响规格。）
