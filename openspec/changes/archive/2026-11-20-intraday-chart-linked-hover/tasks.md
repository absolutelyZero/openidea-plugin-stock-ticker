# Tasks

## 1. OpenSpec 提案

- [x] 1.1 在 `openspec/changes/intraday-chart-linked-hover/` 产出 `.openspec.yaml`、`proposal.md`、`design.md`、`specs/ui/intraday-chart/spec.md`、`tasks.md`。验证：`openspec validate intraday-chart-linked-hover` 通过。

## 2. Rust 数据模型与符号转换

- [x] 2.1 在 `stock-ticker/src/model.rs` 新增 `MinutePoint` / `MinuteSeries` 结构体（Clone/Debug/Default/Serialize/Deserialize）。验证：`cargo test -p stock-ticker` 既有测试通过。
- [x] 2.2 把 `stock-ticker/src/sync.rs:19-32` 的 `tencent_code` 提取为 `model::pub(crate) fn tencent_code`，`sync.rs` 改调该函数。验证：`cargo test -p stock-ticker sync::tests` 通过，行情 URL 不变。

## 3. 分时取数与解析模块

- [x] 3.1 新建 `stock-ticker/src/minute.rs`：常量 `MINUTE_HOST` / `MINUTE_SOURCE` / TTL / force 最小间隔，`MinuteError` 枚举及 `status()` / `message()` / `grant()`。验证：单测覆盖每种错误映射。
- [x] 3.2 实现 `parse_minute(bytes, symbol)`：解析 `date` 与 `"HHMM 价格 累计量 累计额"` 数组，坏点跳过，空数组 → Empty，坏 JSON → Parse；昨收优先 `qt` 数组 `[4]`。验证：`parses_live_shape`、`empty_data_is_empty`、`malformed_json_is_parse_error`、`bad_points_are_skipped` 通过。
- [x] 3.3 实现 `minute_url(code)` 与 `fetch_minute(state, symbol, force)`：`begin_task` + `timeout_ms=4000` + `host::fetch`，缓存命中、TTL、force 最小间隔、跨日失效、失败不缓存。验证：MockHost 断言 URL/timeout/task、缓存命中不发请求、Denied/HTTP/空数据降级单测通过。

## 4. 缓存与消息协议

- [x] 4.1 `stock-ticker/src/state.rs` 的 `State` 新增实例内存 `minute_cache: HashMap<String, MinuteSeries>`，不写 storage。验证：`load_round_trips_all_keys` 等既有测试回归通过。
- [x] 4.2 `stock-ticker/src/lib.rs` 新增 `mod minute`、`minute_json()` / `publish_minute()`，并在 `handle()` 增加 `"minute"` 分支（含 symbol 归属校验、成功写入缓存、独立推送）。验证：协议单测通过。
- [x] 4.3 协议测试：reqId 原样回显、未知 symbol 返回 `invalid_symbol` 且无 fetch、Denied 携带 grant、minute 不改 snapshot/状态栏、与 reorder 共存。验证：`cargo test -p stock-ticker` 全绿。

## 5. WebView 联动实现

- [x] 5.1 `stock-ticker/web/index.html` 改固定左右分栏 DOM（`.panes > .pane-list + .pane-chart`），新增图表 DOM id 与 CSS 变量 `--up` / `--down` / `--chart-grid`、遮罩/空态/高亮/焦点样式。验证：DOM id 唯一，无外部资源引用。
- [x] 5.2 实现内联 SVG 折线 `renderChart(series)`：昨收虚线、时间刻度、折线、`aria-label` 与 `#chart-meta` 文本摘要。验证：静态检查 `createElementNS`、`role="img"`、动态 `aria-label` 存在。
- [x] 5.3 实现 `scheduleMinute` / `requestMinute` / `applyMinuteMessage`：120 ms 防抖、`reqId` 作废与迟到响应丢弃、页面 30 s 本地缓存、三态状态机（切股保留旧图直到一次性替换）。验证：静态检查 `m.type === 'minute'` 与 `m.reqId !== state.minuteActiveReqId` 存在。
- [x] 5.4 行事件与可访问性：`tabIndex=0`、`mouseenter`/`focus`、`onkeydown` ↑↓/Home/End、Enter/Space 选中、`th.scope='col'`、`aria-live`，拖拽期间不触发请求、重绘后恢复高亮。验证：静态检查 + IDE 人工验收。

## 6. 能力配置与文档

- [x] 6.1 `stock-ticker/plugin.toml`：版本升 `0.4.0`，description/change-notes 写明分时联动与重新授权，`[capabilities].optional` 追加 `net:web.ifzq.gtimg.cn`。验证：`openidea-plugin check` 通过。
- [x] 6.2 `stock-ticker/README.md`：功能描述、权限表（用途与撤销后影响）、数据源表加行。验证：与 plugin.toml 能力列表一致。
- [x] 6.3 `DESIGN.md`：标题、FR-9、明确不做清单、WebView 取数链路、预算、决策记录同步，新增 `6.12 分时图联动`。验证：显式记录「列表无红绿、图表红涨绿跌」与保留 K 线禁令。

## 7. 验证

- [x] 7.1 `cargo test -p stock-ticker` 全量通过。
- [x] 7.2 `cargo build -p stock-ticker --target wasm32-wasip2` 与 `openidea-plugin check` 通过。
- [x] 7.3 前端静态检查：DOM id 唯一、`minute` 分支/reqId 校验/`tabIndex`/`role`/`aria-live`/`scope`/`--up`/`--down` 齐备、无外部资源；抽取 `<script>` 用 `node --check` 校验语法。
- [x] 7.4 `openspec validate intraday-chart-linked-hover --strict` 通过。

## 8. 人工 IDE 验收

- [x] 8.1 hover 行 → 右侧立即出分时图；快速划过 5 行 → 仅最终行图表与请求，无中间股票残留。
- [x] 8.2 切换 hover → 旧图在遮罩下保留，新图一次性替换；鼠标移出列表 → 图表收起为占位（v0.4.3 起），迟到响应不回显。
- [x] 8.3 撤销 `net:web.ifzq.gtimg.cn` 授权 → 图表显示可操作提示，列表正常；非交易时段 → 空数据占位。
- [x] 8.4 键盘 Tab / ↑↓ / Home/End → 焦点与图表同步更新；读屏可听到状态播报；折线红涨绿跌，`▲▼` 与涨跌幅文本可读。
- [x] 8.5 拖拽排序、表头排序、添加/移除、定时刷新不回归；反复切换 50 次稳定。

## 9. 归档

- [x] 9.1 用户确认后按项目约定归档 change 并同步主 spec（`openspec/specs/ui/intraday-chart/spec.md`）。验证：`openspec validate --specs` 通过（8 条需求、28 个场景、逐字一致）。

## 10. 修正：分时图恒显示「暂无分时数据」（v0.4.1）

- [x] 10.1 复现与定位：实测 `web.ifzq.gtimg.cn/appstock/app/minute/query?code=sh600519` 返回 `data.<code>.data.data`（采样数组）与 `data.<code>.data.date`，比 0.4.0 解析路径多一层嵌套 → `as_array()` 取空 → 恒返回 `Empty`。
- [x] 10.2 `src/minute.rs::parse_minute` 改为先判 `data.<code>.data` 是否为对象：是则下钻取 `date`/`data`，否则按扁平结构读取，两种形状都接受。
- [x] 10.3 回归测试：`accepts_both_nested_and_flattened_payloads` + `parses_captured_live_payload`（fixture `tests/fixtures/minute_live_sh600519.json` 为真实报文裁剪）。验证：`cargo test` = 55 passed。
- [x] 10.4 文档同步：`DESIGN.md` §6.12 记录真实结构与本修正、`plugin.toml` 升至 0.4.1，本变更 `design.md` 更新数据形状描述。

## 11. 修正：分时图取消红绿（v0.4.2）

- [x] 11.1 `web/index.html`：移除 `--up` / `--down` 令牌，新增 `--chart-line: var(--openidea-accent, #4c9aff)`；折线与面积统一取 `--chart-line`，方向改由 `▲` / `▼` 与 `±%` 文本表达。
- [x] 11.2 图头/摘要文案去掉「（红）/（绿）」，保留「上涨 ▲ / 下跌 ▼ + 涨跌幅」，`aria-label` 同步。
- [x] 11.3 文档与规范同步：`DESIGN.md` FR-9、§1.1、§6.12、§12.3 与附录决策表回归「全界面无红绿」；本 spec 的「图表视觉与文本方向」需求改为单一主题色。
- [x] 11.4 验证：前端静态检查（无 `--up`/`--down` 残留、无红绿字面量）、真实数据渲染模拟（折线 `stroke=var(--chart-line)`、摘要含 `▲/▼`）、`cargo test`、`openidea-plugin build/check`、`openspec validate --strict`。

## 12. 调整：全时段 X 轴、强调最新点与昨收、恢复红绿、离场收起（v0.4.3）

- [x] 12.1 X 轴改为固定交易时段：`sessionMinute('HHMM')` 把 09:30–11:30 / 13:00–15:00 映射到 0–240（午休合并），折线按真实时间推进、面积在最后一点收口；刻度固定 09:30 / 10:30 / 11:30-13:00 / 14:00 / 15:00，配 0.35 透明竖向虚线。
- [x] 12.2 最新点：光晕 + 实心圆点 + 加粗同色价格标签；靠右且与昨收线垂直距离 < 20 px 时标签自动移到点下方，避免与「昨收」标签重叠。
- [x] 12.3 昨收：`--fg-muted` 1.5 px、`7 4` 虚线，右端标注「昨收 <价格>」，摘要与 `aria-label` 同步追加昨收。
- [x] 12.4 配色恢复 `--up` / `--down`（红涨绿跌），移除 `--chart-line`；文本方向仍为 `▲` / `▼` + `±%`。
- [x] 12.5 离场收起：`#pane-list` 的 `mouseleave` 与行 `blur`（`relatedTarget` 不在 `tbody`）调用 `resetChart()`——清空 SVG、作废 `reqId`、取消防抖、恢复占位；拖拽中跳过。
- [x] 12.6 验证：Node 模拟（真实报文）断言刻度、末点 x、面积收口、昨收线与标签、最新点圆点与价格、红绿取值、离场清空与迟到响应丢弃；headless Edge 截图目视确认无重叠。`cargo test` 55 passed、`openidea-plugin build/check` ok、`openspec validate --strict` ok。

## 13. 调整：移除图表图头（v0.4.4）

- [x] 13.1 `web/index.html`：删除 `<div id="chart-head">` 与其 `.chart-head` 样式；`applyMinuteData` / `resetChart` / `clearChart` 不再写入图头，代码·名称·日期等信息统一由 `#chart-meta` 摘要与 `aria-label` 承载。
- [x] 13.2 验证：Node 模拟确认无 `chart-head` 残留、摘要含代码/名称/日期/最新价/昨收、`aria-label` 与摘要一致、离场复位与空态文案正常；headless Edge 截图确认图表上移无空档、刻度与摘要无裁切。`cargo test`、`openidea-plugin build/check`、`openspec validate --strict` 通过。
