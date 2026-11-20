# Proposal

## Why

行情面板目前只以四列纯文本展示现价与涨跌幅，用户想看一只股票的当日走势必须双击跳到外部网页，无法在「写代码时扫一眼」的场景里快速比较。需要把面板改为固定左右分栏：鼠标悬浮（或键盘聚焦）某只股票行时，右侧立即展示该股当日分时图；快速划过多行时只呈现最终停留项，切换不闪烁、不残留，鼠标离开列表后图表收起回到占位。

同时需要补齐加载中、无数据、接口失败三类可观察状态，以及键盘与读屏用户可用的等价交互。

## What Changes

- 行情面板改为固定左右分栏：左侧自选股列表（现有表格原样保留），右侧分时图区域；未选择任何股票时显示占位提示。
- 新增页面→插件消息 `minute`（`symbol` / `reqId` / `force`）与插件→页面响应 `minute`（`reqId` / `status` / `points` / `error` / `grant` 等），不改动现有 `snapshot` 与命令集合。
- 新增腾讯分时接口（`web.ifzq.gtimg.cn/appstock/app/minute/query`）的取数、解析、错误分类与按 symbol 的短时缓存；响应遵循现有 4 s 超时与 `begin_task` 预算约束。
- 右侧使用内联 SVG 绘制分时折线：X 轴固定覆盖 09:30–15:00 全时段（午休折叠），昨收用加粗虚线并标注价格，最新点用圆点 + 加粗价格标签突出，折线按 A 股习惯红涨绿跌；无任何外部脚本、字体或 CDN。
- 补齐加载中 / 无数据 / 失败（含网络权限未授予）三态展示；请求使用单调 `reqId` 与 120 ms 防抖作废旧响应，避免闪烁与残留。
- 补齐基本可访问性：列表行可 Tab 聚焦、↑↓/Home/End 移动焦点，图表带 `aria-label` 文本摘要，状态区 `aria-live`，涨跌方向同时用文本 `▲▼` / `+−` 表达。
- **BREAKING**:新增可选能力 `net:web.ifzq.gtimg.cn`；升级后用户需重新授权该网络权限。未授权时只影响分时图（显示可操作授权提示），行情列表维持现有降级行为。
- **配色与离场**:分时图采用 A 股红涨绿跌（v0.4.2 曾改单色，v0.4.3 按用户要求恢复红绿），同时保留 `▲▼` / `±%` 文本与读屏摘要，不依赖颜色单独传达信息；鼠标/焦点离开列表即收起图表（取消 v0.4 的「移出保留」）。仍不做 K 线图、秒级刷新与成交量副图。

## Capabilities

### New Capabilities
- `ui/intraday-chart`: 自选股行悬停/聚焦触发的当日分时图联动，包括左右分栏布局、请求竞态控制、三态展示、键盘与读屏可访问性，以及分时数据消息协议与错误分类。

### Modified Capabilities
（无。主 specs 目录当前只有 `ui/watchlist-reorder`；本变更不修改其需求，仅新增独立能力。）

## Impact

- `stock-ticker/web/index.html`：新增分栏 DOM/CSS、内联 SVG 折线、hover/focus 事件、`reqId` 防竞态、三态 UI 与 ARIA；现有选中、双击打开、拖拽排序、表头排序、定时刷新逻辑必须保持不回归。
- `stock-ticker/src/minute.rs`（新建）：分时接口 URL、JSON 解析、错误分类与取数缓存逻辑。
- `stock-ticker/src/model.rs`：新增 `MinutePoint` / `MinuteSeries`；把 `tencent_code` 提取为 `pub(crate)` 供行情与分时共用。
- `stock-ticker/src/sync.rs`：改为调用提取后的 `model::tencent_code`，行为不变。
- `stock-ticker/src/state.rs`：新增实例内存 `minute_cache`（不写 storage）。
- `stock-ticker/src/lib.rs`：新增 `mod minute`、`minute_json` / `publish_minute` 与 `handle` 的 `minute` 分支；分时响应独立推送，不改快照与状态栏。
- `stock-ticker/plugin.toml`：版本递增（BREAKING），`[capabilities].optional` 追加 `net:web.ifzq.gtimg.cn`，change-notes 提示重新授权。
- `stock-ticker/README.md`、`DESIGN.md`：权限表、数据源表、FR-9 与「明确不做」清单、WebView 取数链路、预算与决策记录同步；新增 6.11 分时图联动小节。
- 测试：Rust 侧新增解析/MockHost 网络/错误降级/消息协议单测；前端交互需 IDE WebView 人工验证。
- 不在范围：K 线图、秒级刷新、成交量副图、跨面板拖拽、触摸手势、数据源替换与行情列表列集合变更。
