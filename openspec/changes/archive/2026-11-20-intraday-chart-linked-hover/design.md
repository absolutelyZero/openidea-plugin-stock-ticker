# Design

## Context

行情面板是 WebView 页面（`views[].web = "web/index.html"`），页面只渲染、不取数：页面 `setInterval` 发 `refresh`，插件 `host::fetch` 取腾讯快照后 `post_view_message` 回推 `snapshot`（见 `DESIGN.md` §6.10）。当前页面是单列纵向布局（工具栏 → 表格 → 页脚），只有 CSS `:hover` 背景变化，没有任何图表、hover 事件、ARIA 或键盘交互；`renderRows()` 每次快照都重建 `<tbody>`，拖拽期间用 `deferredSnapshot` 缓冲。

约束：WebView 无网络访问，分时数据必须经消息桥由插件转发；网络调用必须 `begin_task` + `timeout_ms = 4000`（`DESIGN.md:68`）；能力按 host 精确授权，新 host 属 BREAKING；必须保持 ES5 风格 JS、内联 CSS/SVG、无外部资源；响应体上限 8 MiB、命令 5 s / task 60 s 预算。腾讯分时接口 `web.ifzq.gtimg.cn/appstock/app/minute/query?code=<sh/sz/bj+代码>` 返回 JSON：`data.<code>.data.date` 为 `YYYYMMDD`，`data.<code>.data.data` 为 `"HHMM 价格 累计量 累计额"` 字符串数组（实现中发现采样数组比直觉路径多一层嵌套，解析器兼容嵌套与扁平两种结构，见 v0.4.1 修正）；该接口非官方、无 SLA。

## Goals / Non-Goals

**Goals:**
- 悬停/聚焦股票行即展示该股当日分时图；切换项只呈现最终停留项，不闪烁、不残留；移出列表保留当前图。
- 加载中、无数据、失败（含权限未授予）状态对 sighted 与读屏用户都可感知。
- 键盘用户能等价触发图表更新，且不破坏现有选中/打开/拖拽/排序/刷新交互。
- 失败与未授权只影响分时图，行情列表继续按现有降级逻辑工作。

**Non-Goals:**
- K 线图、秒级刷新、成交量副图、触摸手势、跨面板拖拽。
- 引入前端框架、图表库、外部 CDN/字体/脚本，或新的 Rust crate。
- 改变列集合、数据源（快照仍为 `qt.gtimg.cn`）、刷新节奏与命令集合。
- 把分时数据写入 storage 或做跨会话缓存（实例内存缓存即可）。

## Decisions

### 1. 固定左右分栏，图表容器独立于表格
页面结构改为 `.panes > .pane-list + .pane-chart`，原 `.wrap` 表格整体迁入左栏；`#foot` 移到 `.panes` 之后。图表 SVG 不放在 `<tbody>` 内，因此 `renderRows()` 整表重绘不会销毁图表；重绘后只恢复 `.chart-active` 行标记，不重建 SVG。

**替代方案**：图表浮层/按需弹出。被否决——用户已确认固定分栏；浮层在 IDE 侧边窄面板里容易遮挡列表且移出保留语义不清。

### 2. 请求响应带 `reqId`，页面侧作废旧请求
页面维护单调递增 `minuteSeq` 与 `minuteActiveReqId`。每次 `requestMinute()` 先作废旧 id 再发新请求；响应到达时 `reqId !== minuteActiveReqId` 一律丢弃。配合 120 ms 防抖，快速划过多行只请求最终停留项；迟到响应不会覆盖当前图（防残留）。Rust 侧无状态、原样回显 `reqId`。

**替代方案**：Rust 侧维护「最后请求 symbol」并按最新请求过滤。被否决——消息桥与插件实例可能被回收，页面侧序号更可靠且便于单测。

### 3. 缓存与节流
页面侧 `minuteLocalCache`（30 s）命中直接渲染，避免消息往返；插件侧 `State.minute_cache`（60 s TTL，key = symbol，value 自带 `date` 跨日失效）。`force` 仅在距上次取数超过 5 s 时真正发起请求，防止 hover 抖动造成任务泛洪。失败不写缓存；实例回收后缓存丢失可接受。

### 4. 分时数据模型与错误分类统一
`model.rs` 新增 `MinutePoint` / `MinuteSeries`；`tencent_code` 提取为 `pub(crate)`，行情与分时共用同一套 sh/sz/bj 转换（避免列表代码与图表代码不一致）。`minute.rs` 定义 `MinuteError::{Denied, Http, Empty, Parse, InvalidSymbol, Cancelled}`，映射到响应 `status` 与中文 `error`；Denied 额外回传 `grant = "net:web.ifzq.gtimg.cn"`，页面显示可操作授权提示。`empty` 与 `parse_error` 严格区分，便于定位接口漂移。

### 5. 分页响应协议独立于 snapshot
请求：`{"type":"minute","symbol":"1.600519","reqId":7,"force":false}`。
响应：`{"type":"minute","reqId":7,"symbol":"...","status":"ok","date":"20261120","prevClose":...,"points":[["0930",1438.55,18370]],"source":"腾讯分时","fetchedAt":...,"error":null,"grant":null}`。
`minute_json` / `publish_minute` 只推给 `watchlist` 视图，不调用 `publish`，不改 `set_index_status`；`handle` 校验 symbol 必须属于 `state.watch`，否则回 `invalid_symbol` 且不发 fetch。

### 6. SVG 绘制与视觉约定
`viewBox="0 0 600 240"`，内边距左 48 / 右 12 / 上 12 / 下 24。**X 轴固定覆盖完整交易时段**：`HHMM` 转为「开盘后第几分钟」（09:30→0、11:30→120、13:00→120、15:00→240，午休折叠），折线按真实时间推进，底部刻度固定为 09:30 / 10:30 / 11:30-13:00 / 14:00 / 15:00，并配极淡竖向虚线；数据只到盘中某刻时，右侧留白而不是把曲线拉伸铺满，面积也在最后一点处收口。Y 轴以昨收为中心对称（`half = max(|max-prev|, |min-prev|, prev*0.01)`）。**昨收**用 `--fg-muted` 的 1.5 px 加粗虚线，右端标注「昨收 <价格>」。**最新点**画光晕 + 实心圆点，并在旁以加粗同色数字标出最新价（靠右且与昨收线过近时自动下移到点下方，避免与昨收标签重叠）。折线与面积按最新价相对昨收取 `--up`（红）/ `--down`（绿）——v0.4.3 按用户要求恢复 A 股红绿。`#chart-meta` 输出「代码 名称 日期 最新价 `▲/▼` 涨跌幅 昨收」文本摘要，`aria-label` 同步；独立的图头 `<div id="chart-head">` 已于 v0.4.4 移除，避免与摘要重复占高。

**替代方案**：ECharts/Chart.js。被否决——项目禁止外部 CDN 且无前端构建链，内联 SVG 最稳。

### 7. 三态、切股与离场语义
加载中：保留旧 SVG 与旧摘要，叠加遮罩显示「正在加载 <目标名称>…」，避免把旧数据误标为新股票；新数据到达后摘要与 SVG 一次性替换。无数据：清空 SVG + 空占位 + 状态文案。失败：同股刷新失败保留旧图并在状态区显示错误；切股失败清空并展示错误占位（Denied 文案含 grant）。状态区 `role="status" aria-live="polite"`。
**离场（v0.4.3 行为变更）**：鼠标离开列表（`.pane-list` `mouseleave`）或键盘焦点离开列表（行 `blur` 且 `relatedTarget` 不在 `tbody` 内）即调用 `resetChart()`——清空 SVG、作废 `reqId`、取消防抖、恢复「将鼠标移到左侧股票行查看当日分时」占位；拖拽中不触发该复位。原 v0.4「移出保留当前图表」被用户显式取消。

### 8. 可访问性
列表行 `tabIndex=0`，↑↓/Home/End 移动焦点（focus 事件自然触发分时），Enter/Space 保留原选中语义；表头 `th.scope='col'`；行 focus 有可见 outline；图表 `role="img"` + 动态 `aria-label`；涨跌除颜色外仍以 `▲▼`/`+−` 文本表达，不依赖颜色单独传达信息；焦点离开列表时图表收起（与鼠标离场一致）。

### 9. 设计决策留痕
配色与离场行为在实现过程中各变动过一次：v0.4 首次实现即按 A 股红涨绿跌；v0.4.2 用户要求「分时图的 K 线不用区分红绿」，改为单色主题色；v0.4.3 用户明确「分时线还是采用红绿吧」，恢复 `--up` / `--down`（列表仍纯文本 `▲▼`）。图表始终附带 `▲▼` / `±%` 文本与读屏摘要，颜色只是增强而非唯一信息通道。离场语义同理：v0.4 为「移出保留」，v0.4.3 改为「移出即收起」。`DESIGN.md:33` K 线/秒级刷新仍在「明确不做」清单，分钟级分时折线不在其列。

## Risks / Trade-offs

- **接口无 SLA** → 防御式解析（坏点跳过、字段缺失按 empty 处理、坏 JSON 报 parse_error），失败不缓存；不影响快照列表。
- **能力重新授权（BREAKING）** → version 升 0.4.0，README/change-notes 显著提示；未授权时图表显示可操作提示、列表继续工作。
- **WebView 兼容性** → 只用 ES5 `var/function`、内联 SVG、CSS 变量回退；不用 `color-mix()`、新语法、外部资源；遮罩纯文字不做动画。
- **拖拽与重绘冲突** → `state.dragging` 时不触发分时请求；`deferredSnapshot` 逻辑保持不变；重绘后恢复 `.chart-active`。
- **请求泛洪** → 120 ms 防抖 + 页面 30 s / 插件 60 s 双层缓存 + force 最小 5 s 间隔。
- **昨收缺失** → 优先取分时 JSON 的 `qt` 数组 `[4]`，缺失时回退 `state.snapshot.quotes[].prev_close`；两者都缺时按 points 的 min/max 绘制，文本摘要不输出涨跌幅百分比。

## Migration Plan

新增能力属 BREAKING：升级后首次使用分时图需重新授权 `net:web.ifzq.gtimg.cn`；未授权前图表显示授权提示，行情列表、自选股、排序与拖拽完全不受影响。无 storage 数据结构变更，`minute_cache` 为实例内存态，无需数据迁移。回滚时移除 `minute` 消息分支、分栏 DOM 与新能力即可，旧快照逻辑原样保留。

## Open Questions

（无。数据源、配色、布局与交付流程均已与用户确认；腾讯接口的非官方性质已在风险中记录。）
