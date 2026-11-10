# OpenIDEA 股票行情插件 — 设计方案（v0.3 · 定稿：原生表格、无红绿）

> 状态：**M0–M6 已实现，55 个单测通过，官方 `build`/`check` 通过，已 `link` 到本机 IDE**。新增「15 s 活动心跳」以更快看到价格变化（60 s 后台下限不变），见 §6.5。添加交互改为**面板内结果列表**（单标签不变、不弹第二个对话框）；API 1.3 无 type-ahead，关键词需提交一次，见 §6.4。按用户反馈，行情页**退回原生 `sortable_table`、去掉红绿配色**，并修复「不自动刷新 / 无法选中」问题（根因：HTML 帧在部分 IDE 构建里不随 `refresh_view` 重绘、消息桥不通）。后续又修复：自动刷新被节流/非法 `on-schedule:15` 吞掉、添加时改为联想接口模糊搜索、删除改为选中行即确认、图标换名。**模糊搜索必须用 `searchapi` 联想接口，`clist/get` 不支持 keyword**（实测会静默忽略并返回全市场首页）。所有 API 标识符均按 **tag `v0.1.39`** 的源码逐字核对（该标签 `wit/plugin.wit` 为 `openidea:plugin@1.3.0`，SDK `API_VERSION = "1.3"`）。执行记录见附录 A–E。

## 0. 已确认的决策

| 编号 | 决策 | 影响 |
| -- | -- | -- |
| ① | `engines.openidea = "^1.3"` | 必须整体锁定 `v0.1.39` 的 SDK 与 CLI（main 是 1.4.0，`check` 会拒绝比宿主新的 minor） |
| ⑥ | A 股 MVP + **原生表格、纯文本、无红绿** | 原生 `Table` 单元格只有 `Vec<String>`、无 per-cell 样式 → 直接用 `Body::sortable_table`，放弃 `Body::Html` 彩色帧；方向以 `▲▼` 与 `+/-` 传达 |
| 回退原因 | HTML 帧在真实 IDE 中**不随 `refresh_view` 重绘、消息桥不通** | 导致「不自动刷新 / 点不中」，故回退到宿主侧重绘的原生表格（文档化、最可靠路径） |
| 其余 | 按 v0.1 默认 | 60 s 刷新、双源降级、storage 唯一真相源、权限最小化 |

---

## 1. 目标、范围与执行边界

### 1.1 定位
右侧工具窗显示自选股行情，纯文本 4 列（代码 / 名称 / 现价 / 涨跌%），原生可排序表格，**不依赖红绿配色**。面向「写代码时扫一眼」，不是秒级盯盘工具。

### 1.2 MVP 功能
- FR-1 工具窗（单一标签 `watchlist`）：原生可排序行情表，表头排序、双击/回车打开行情页、工具栏刷新/添加/打开/移除。
- FR-2 每 60 s 后台自动拉取（IDE 最小合法调度）；**用户活跃时按 15 s 节流追加刷新**；toolbar / 命令 / 快捷键手动刷新。
- FR-3 A 股行情：现价、涨跌额、涨跌幅、昨收、开/高/低、成交量、行情时间。
- FR-4 指数（上证、深证、创业板）置顶行（可关），状态栏常驻一行。
- FR-5 自选股增删与持久化，重启不丢；**添加支持代码 / 中文名 / 拼音首字母模糊搜索**（`searchapi.eastmoney.com` 联想接口，只保留 A 股），结果以**面板内 `Body::List`** 呈现，双击/回车添加、「返回」回表格（见 §6.4）。
- FR-6 价格上下限预警，通知去重（每个 UTC 日一次）。
- FR-7 **删除优先用表格当前选中行 + `host::confirm` 确认**，不再二次弹选择框；「打开」与未选中时的降级仍走原生 `host::pick`。
- FR-8 设置页：显示精度、涨跌幅样式（`arrow`/`sign`）、数据源、陈旧阈值、是否显示指数、测试连接。
- FR-9 **不使用红绿颜色**；涨跌以 `▲▼` 与 `+/-` 表示（不依赖颜色单独传达信息）。

### 1.3 明确不做
港股/美股、K 线图、下单、账户/凭据、任意文件读写、外部进程、秒级刷新、行情数据再分发、外部 CDN/字体/脚本、HTML 沙箱帧。

### 1.4 环境（实测）
- `rustc 1.96.0` / `cargo 1.96.0`；已装 `wasm32-wasip2` + `x86_64-pc-windows-msvc`。
- 目标宿主：OpenIDEA 0.1.39（API 1.3.0），仓库 tag `v0.1.39`。
- 工作目录 `D:/minipro/openidea_plugins_dev/stock`。

---

## 2. 总体架构

```
┌ 行情源（HTTPS / UTF-8 JSON） ───────────────────────────────┐
│  push2.eastmoney.com（主） / push2delay.eastmoney.com（备）   │
│  searchapi.eastmoney.com（仅「添加」时的联想模糊搜索）        │
└───────────────────────┬───────────────────────────────────┘
                        │ host::fetch(net:<host>)
┌───────────────────────▼───────────────────────────────────┐
│ WASM 插件实例（单线程，5 分钟空闲即回收）                    │
│  Plugin::{activate, handle_events, run_command}              │
│  Views::{render(view,...), handle(view, event,...)}          │
│  ├─ quote/     东财字段解析 + 降级/熔断                      │
│  ├─ model.rs   纯函数：ordered_rows / index_rows / 行号解析  │
│  ├─ state.rs   storage（唯一持久化真相）                     │
│  └─ sync.rs    fetch + 双源 + 任务 + 预警 + publish          │
└───┬──────────────┬───────────────┬──────────────┬─────────┘
    │ storage      │ set_status_item│ refresh_view │ open_url / pick / input
 ┌──▼───┐    ┌─────▼────┐   ┌───────▼───────┐
 │插件存储│    │ 状态栏    │   │ watchlist 标签（唯一，原生表格）│
 └───────┘    └───────────┘   └───────────────────────┘
```

**四条铁律**
1. `render()` **绝不发网络请求**，只读内存快照，保证 5 s 预算内完成。
2. 跨调用存活的数据全放 `host::storage`；实例内存只是缓存。
3. 网络调用一律包在 `host::begin_task()` 内并显式 `timeout_ms = 4000`。
4. 视图一律用 **原生 `Body::sortable_table`**；刷新走宿主侧 `refresh_view`（不依赖任何 JS 桥）。

---

## 3. 插件 API 契约（`v0.1.39` 逐字核定）

| 用途 | SDK 符号（均为 v0.1.39 实际名称） |
| --- | --- |
| 骨架 | `Plugin`：`activate` / `run_command` / `handle_events`；`register_plugin!(StockPlugin, views)`；`API_VERSION = "1.3"` |
| 视图 | `Views::render(view, viewport, ctx)` → `Result<View, Error>`；`Views::handle(view, event, ctx)` |
| 原生表格 | `Body::sortable_table(&Table, Option<TableSort>)` → `Body::Other{kind:"table"}`；`Table{ columns, total_rows, first_row, rows: Vec<Vec<String>>, editable }` |
| 事件解码 | `ViewEvent::{Button(String), sort()->Option<TableSort>, table_select()->Option<u32>, table_activate()->Option<u32>, Cell{row,column,value}, InsertRow(u32), DeleteRows(Vec<u32>), Message(String), Other{kind,data}}` |
| 网络 | `host::fetch(&HttpRequest) -> Result<HttpResponse, Error>`；`HttpRequest::get(url).header(...)` 含 `timeout_ms`；`HttpResponse{status,headers,body}` |
| 存储 | `host::storage::{get,set,delete,keys}`（≤ 64 MiB UTF-8） |
| 设置 | `host::setting_*(key)`：**只有读，没有 setter** |
| 状态栏 | `host::set_status_item(id, &StatusItem{text,tooltip,command})` |
| 通知 | `host::notify_with_actions(level, title, body, &[NotificationAction])` |
| 交互 | `host::pick(title, &[PickItem])`、`host::input(prompt, Option<&str>)` |
| 刷新/导航 | `host::refresh_view(view)`、`host::reveal_view(view)`、`host::open_url(url)` |
| 任务 | `host::begin_task(title, cancellable)`；`Task::report(done, total, msg)`、`is_cancelled()` |
| 订阅 | `host::subscribe(EventKind::Scheduled)`（需 `events`） |
| 错误 | `Error::{Denied, Stale, NotFound, Invalid, Cancelled, Failed}` |
| 测试 | `MockHost::install()`、`.with_capabilities`、`.on_fetch`、`.requests()`、`.set_setting`、`.storage()`、`.status_items()`、`.refreshed_views()`、`.opened_urls()`、`.view_messages()`、`.tasks()`、`.answer_next_prompt()` |

**1.3 中确认不存在**：设置项写入 API、表格单元格样式、WebSocket/推流/定时器、低于 60 s 的 `on-schedule`、`net.http:`（1.4 才有）。

---

## 4. 原生表格设计（当前方案）

### 4.1 单标签：`watchlist` 原生可排序表
`Table` 单元格是 `Vec<String>`，1.3 里**无任何 per-cell 样式**（已核对 v139 README §Views）。因此本插件不追求红绿，改用原生 `Body::sortable_table`：所有 IDE 原生交互（排序、行选中高亮、双击激活）都直接可用，且刷新走 `refresh_view` 宿主侧重绘，最可靠。

| 标签 | Body | 职责 |
| --- | --- | --- |
| `watchlist`（唯一标签） | `Body::sortable_table` | 4 列纯文本行情；表头点击排序；双击/回车打开行情页；工具栏刷新/添加/打开/移除 |

### 4.2 渲染契约
- `render()` 构建 `Table`：`columns = [代码, 名称, 现价, 涨跌%]`，`first_row=0`，`rows = Vec<Vec<String>>`，`editable=false`，再 `Body::sortable_table(&table, Some(state.sort))`。
- `rows` 顺序：先放指数行（若 `show_indices`），再放按 `state.sort` 排好序的自选股行；与 `symbols: Vec<String>` 一一对齐，供 `handle` 反查。
- 数据在 `render()` 时直接生成，首次打开即有内容（无空帧问题）。

### 4.3 交互契约
| 用户动作 | 事件 | 处理 |
| --- | --- | --- |
| 点工具栏按钮 | `ViewEvent::Button(id)` | 转发 `run_command` |
| 点表头排序 | `event.sort() -> TableSort{column,descending}` | 写 `state.sort` + 存 `storage` + `refresh_view` |
| 双击/回车某行 | `event.table_activate() -> row` | `symbols[row]` 取 symbol → `host::open_url` 打开行情页 |
| 行选中高亮 | `event.table_select()` | 由 IDE 原生处理，插件忽略 |

### 4.4 刷新契约（修复「不自动刷新」的关键）
- `publish(state) = host::refresh_view(VIEW_WATCHLIST) + set_status_item`。
- `sync::sync()` 成功/失败/空列表后都会调用 `publish`（`sync` 内部）。
- `handle_events(Event::Scheduled)` 在 `sync` 之后再显式调用一次 `publish`，确保即使 `sync` 被 55 s 节流跳过，视图仍重绘最新快照。

> **早期 HTML 帧为何被弃用**：实测 `refresh_view` 在沙箱帧里并不重装 iframe，且 `window.openidea` 消息桥在部分 IDE 构建里不通，导致刷新与点击全部失效。原生表格走宿主侧重绘，无此问题。

---

## 5. 数据契约

### 5.1 模型（`src/model.rs`，纯函数，native 可测）
```rust
pub struct WatchItem { pub symbol: String, pub alias: Option<String>,
    pub cost: Option<f64>, pub qty: Option<f64>,
    pub above: Option<f64>, pub below: Option<f64> }
pub struct Quote { pub symbol: String, pub name: String,
    pub price: Option<f64>, pub prev_close: Option<f64>, pub open: Option<f64>,
    pub high: Option<f64>, pub low: Option<f64>, pub change: Option<f64>,
    pub change_pct: Option<f64>, pub volume: Option<f64>,
    pub turnover: Option<f64>, pub ts: Option<i64> }
pub struct Snapshot { pub fetched_at: i64, pub source: String, pub quotes: Vec<Quote> }
pub struct Row { pub symbol: String, pub cells: Vec<String> }  // 原生表格一行
```
`normalize(input) -> Result<String, Error>`：接受 `1.600519`/`sh600519`/`600519`/`sz000001`，按首位推断市场（6→`1.`，0/3→`0.`），非法返回 `Error::Invalid`。

### 5.2 `host::storage` 键（唯一持久化真相）
| Key | 内容 | 写入时机 |
| --- | --- | --- |
| `watch.v1` | `Vec<WatchItem>` JSON | 增删后立即写 |
| `quotes.v1` | `Snapshot` JSON（仅最新一帧） | 每次成功拉取后写 |
| `sort.v1` | `TableSort` JSON | 排序变化后写 |
| `alert.<symbol>` | `yyyymmdd` | 预警触发当日去重 |
| `meta.v1` | `last_fetch_at`、`last_error`、失败计数 | 每次同步 |

### 5.3 设置页（`^1.3` 的 `pages`）
- `settings/general.json`：`precision`(int=2)、`showIndices`(bool=true)、`staleAfterSecs`(int=180)、`source`(enum `"primary"|"delayed"`)、`changeStyle`(enum `"sign"|"arrow"`)。（**已移除 `colorScheme`**，因不再配色。）
- `[[contributes.settings.pages.actions]]`：`title="测试连接"` → `command="test-connection"`。
- 设置键全插件唯一；页面 id 不含点；`schema` 为独立 JSON 文件。

---

## 6. 数据源（A 股 MVP）

### 6.1 主源（一次请求批量拉全部）
```
GET https://push2.eastmoney.com/api/qt/ulist.np/get
    ?fltt=2&invt=2&secids=1.600519,0.000001,1.000001,0.399001,0.399006
    &fields=f2,f3,f4,f5,f6,f12,f13,f14,f15,f16,f17,f18,f124
```
响应 `{ "rc":0, "data": { "diff": [ {...} ] } }`。字段：`f2` 现价、`f3` 涨跌幅%、`f4` 涨跌额、`f5` 量、`f6` 额、`f12` 代码、`f13` 市场、`f14` 名称、`f15/16/17` 高/低/开、`f18` 昨收、`f124` 行情时间戳。

**`fltt=2` 已实测（2026-11-09）：直接返回真实小数，不除以 100；`f124` 稳定返回 Unix 秒。** 真实响应固化为 `tests/fixtures/eastmoney-live.json`。解析器统一 `as_f64()` 容错：`"-"`/缺失/`null` → `None` → 渲染 `—`，绝不 panic。

**坑**：`f12` 只有 6 位代码，`000001` 既是平安银行（`0.000001`）又是上证指数（`1.000001`），必须结合 `f13`（`1` 沪 / `0` 深）重建 secid。

### 6.2 备源与降级
- 备源 `https://push2delay.eastmoney.com/...`：同 schema、HTTPS、UTF-8，仅行情延迟。
- 顺序：主源 → 备源 → 都失败则保留 `quotes.v1` 并标 `stale`；连续失败在 `meta.v1` 记熔断，一段时间直接走备源。

### 6.3 模糊搜索源（添加自选股，2026-11-10 实测修正）
```
GET https://searchapi.eastmoney.com/api/suggest/get?input=<urlencoded>&type=14&count=20
```
响应 `{ "QuotationCodeTable": { "Data": [ { "Name", "Code", "PinYin", "Classify", "QuoteID" } ], "TotalCount": n } }`；无匹配时 `Data: null`。**无需 token**。

实测（2026-11-10）：`input=东方` → 东方创业/东方明珠/东方通信/东方电气/东方证券…；`input=dftx` → 东方通信（PinYin `DFTX`）+ 一只美股 `DFTX`；`input=茅台` → 贵州茅台；`input=600519` → 贵州茅台。

**重要坑（本轮 bug 根因）**：曾用 `push2.eastmoney.com/api/qt/clist/get?...&keyword=东方` 做搜索。该接口是**行情列表**，`keyword` 参数被静默忽略，实测返回 `total:12328` 且前 20 条为「摘牌赛德/宝来利来/宜通海科…」等无关北交所股票，所以用户输入后看不到任何匹配项。`push2` 上也没有 `/api/suggest/get` 路径（实测返回空），因此**必须新增 `net:searchapi.eastmoney.com` 能力**。

解析约束：
- 只保留 `Classify == "AStock"`，过滤同一关键词命中的美股/港股/基金/指数；
- 用 `QuoteID`（`1.600519` / `0.000001`）作为 secid，与行情批量请求的 secid 格式一致，并用 `1.`|`0.` + 6 位数字做白名单校验，拒绝 `105.DFTX` 之类；
- 匹配结果（不论单/多）→ **面板内 `Body::List`**；0 结果 → 提示「未找到匹配」；权限缺失 → 明确提示授权 `net:searchapi.eastmoney.com`。

### 6.4 交互形态约束：为什么不是逐字符下拉（2026-11-10 源码核查）

用户原话是「输入首拼/中文/代码时下拉出匹配的股票」。**严格意义做不到**，证据来自 IDE `v0.1.39` 前端源码：

| 宿主能力 | 源码证据 | 结论 |
| --- | --- | --- |
| `host::input` | `runtimeUi.tsx:346` → `promptInput` → `inputDialogs.tsx:207` 纯 `<Input>` | 无 suggestions；只在 OK/Enter 提交时返回**一次**文本 |
| `host::pick` | `runtimeUi.tsx:344` → `pickItems` → `inputDialogs.tsx:268` 静态 `<List>`；`ui/List.tsx` 无 speedSearch | 静态列表，无过滤输入框 |
| `Body::Form` 的 `text` | `PluginView.tsx:410–441`：`TextField` 仅 `onBlur`/Enter 才 `commit`，且 `useEffect` 用 `initial` 重置 | 非实时，重绘会重置输入 |
| `Body::Form` 的 `choice` | `PluginView.tsx:398` → IDE `<Select>`（`Select.tsx:74` 有前缀 type-ahead） | 是下拉，但选项须插件预先算好 |
| `Body::Html` | 本项目实测：该 IDE 构建消息桥不通、`refresh_view` 不重绘帧 | 已否决 |
| main / 1.4 文档 | `plugins-README.md` 全文无 `suggest`/`autocomplete`/`filterable` | 升级 API 亦无此能力 |

**采用的方案 A**（用户 2026-11-10 选定）：单标签不变，输入关键词回车后，同一个视图从
`Body::sortable_table` 原地切换成 `Body::List`（匹配结果），双击/回车某行用
`ViewEvent::Activate(key)` 添加，点「返回」切回表格。不新增视图、不依赖消息桥、不弹第二个对话框。

- 结果态工具栏只有「返回」(`arrow-left`) 与「重新搜索」(`search`)，避免对不可见表格行误操作。
- **单击 `Select` 刻意忽略**：IDE 在方向键移动选中项时也会发 `select`（`PluginView.tsx:270,278`），
  若单击即添加会误加；因此用 `activate`（双击/回车）确认。
- `ListItem.key` 直接放 secid，`Activate` 时无需再查表；`add_symbol()` 为「代码直加」与「结果点击」的公共路径。
- **图标名已按 IDE 合法集合核对**：`refresh` / `add` / `external-link` / `trash` / `arrow-left` / `search` 全部有效；
  上一轮误写的 `link-external` **不是合法图标名**，已改为 `external-link`。

### 6.5 更快看到价格变化：15 s 活动心跳（2026-11-10）

- 用户真实目标是「更快看到价格变化」，不是严格的 15 s 定时器。
- **硬限制**：`crates/ide-extension/src/manifest.rs:126–130` 的
  `ActivationEvent::parse` 对 `secs < 60` 直接返回
  `Err("the shortest schedule is 60 seconds")`；`plugins/README.md:273` 也写明
  "at least 60"。写成 `on-schedule:15` 会导致 **`check` 失败、IDE 加载插件时被拒绝**
  （比刷新慢严重得多）。运行时 `ide-host/src/plugins/mod.rs:2095` 用的是
  `Duration::from_secs(secs)`、本身不 clamp，但到不了那一步。WASM 插件没有自有定时器，
  `Event::Scheduled` 是唯一周期信号。
- **采用的方案**：60 s 后台下限保持不变；`activation` 增加
  `on-event:editor-focused` / `on-event:file-saved`（均在 `EVENT_KINDS` 合法表内），
  `activate()` 订阅对应 `EventKind`；`handle_events` 在这些事件到达时，按
  「距上次尝试 ≥ `HEARTBEAT_SECS`(15)」节流调用 `refresh_now()`（强制同步 + `publish`）。
  用户编辑/切换/保存文件时最快约 15 s 看到新价；**完全静止仍为 60 s**。
- **防失败风暴**：`last_heartbeat_fetch` 记录的是**尝试时间**（请求前写入、实例内有效、
  不持久化），而非成功时间，因此行情源失败时不会每个 IDE 事件都重试。
  手动刷新与 Scheduled 路径也会更新该时间戳，避免刚拉完又被心跳重复拉。
- **不打断选择**：`search_results.is_some()`（面板内结果列表）时直接跳过心跳刷新。
- **重要纠正**：此前文档/答复推测「`refresh_view` 重绘会重置表格选中与滚动」。核查
  `PluginView.tsx:465–467`：`selected` 与 `scrollTop` 是 `TableBody` 组件本地 `useState`；
  body 渲染 `case "table"`（`:299`）没有 key，React 按位置复用组件 →
  **table→table 重绘保留选中集与滚动位置**。因此高频刷新不破坏 FR-7「选中行即删」。
- **代价与风险**：请求频率最高约为原来的 4 倍，东财接口可能限流；每次刷新都会创建
  `begin_task`（4 s 超时 + 双源降级）。想更保守只需把 `HEARTBEAT_SECS` 调大。
- **否决**：改 IDE 宿主 `ActivationEvent::parse` 绕过校验 —— 超出插件范围、破坏
  `check`、需重编译 IDE。

### 6.6 插件图标与上架准备（2026-11-10）

**图标规范（源码核对，避免做成不能用的素材）**：
- `manifest.rs:57–60`：`icon: Option<PathBuf>` / `icon_dark`，"SVG shown in Settings › Plugins and the marketplace"；`plugins/README.md:121–127`：每个 SVG **≤16 KiB**、安装时 sanitise、`icon` 需 API ≥1.2（本插件 `^1.3` 满足）。
- `ide-host/src/plugins/icons.rs:40` `MAX_PLUGIN_ICON = 16 << 10`；`:343 sanitize_svg` 只允许白名单元素/属性（`svg/g/path/circle/rect/line/polyline/...` 与 presentation attributes），禁止脚本、事件处理器、外部 `href`、非本地 fragment 的 `url()`、DTD/entities/CDATA；`title/desc/metadata` 连同内容被剥离。

**设计**：深蓝圆角方底 + 四根渐高的成交量柱（半透明白）+ 红色上升折线 + 末端红点（红=涨，A 股习惯；**仅用于品牌图标，行情表格仍无红绿**）。暗色版底板提亮到 `#233A63`、折线提亮到 `#FF6B66`，避免在深色主题里沉没。全部纯色、无渐变（规避 `url()` 风险）、无文字（规避字体依赖），在小尺寸下只保留粗线条轮廓。两套各 **925 B**。

**接入与验证**：`plugin.toml` 增加 `icon = "icon.svg"` / `icon-dark = "icon_dark.svg"`；`openidea-plugin check` **ok**。另用与 sanitizer 相同的白名单做静态核验，两个文件均 OK。生成**未签名本地验证包**
`pack . --out .workbuddy/tmp/absolutelyzero.stock-ticker-0.1.0.unsigned.oiplugin`（130.7 KiB），
包内 6 个文件含 `icon.svg` 与 `icon_dark.svg` → 证明素材确实随包分发。

> 注意：`pack` **不**执行图标 sanitize；真正的 `plugin_icon()` → `sanitize_svg()` 在
> `index`（`main.rs:653`）或 IDE 安装时发生，而 `index` 要求包**已签名**
> （`main.rs:625`）。因此完整的内容校验需要签名私钥，本轮未执行。

**上架仍缺的材料（未擅自填写，需用户决定）**：
| 项 | 现状 |
|---|---|
| `authors` | 空数组 |
| `license` / `LICENSE` 文件 | 均无（`pack` 会自动收录 `LICENSE*`/`README.md`） |
| `homepage` / `change-notes` | 均无（可选，marketplace 展示用） |
| publisher | `id = "absolutelyzero.stock-ticker"` → 签名时 `--publisher` **必须是 `absolutelyzero`**（`main.rs:562–563`）；换名须先改 id 前缀 |
| `version` | `0.1.0`，已合 semver |

**命令草案（未执行，涉及私钥与发布）**：
```
openidea-plugin keygen absolutelyzero.key                      # 一次性；私钥绝不入库、不外发
openidea-plugin pack . --key absolutelyzero.key --publisher absolutelyzero
openidea-plugin index market/ --publisher absolutelyzero --name "absolutelyZero" --key absolutelyzero.key
```

---

## 6.7 上架执行记录（A 路径 · 社区 marketplace，2026-11-10）

- 仓库：`https://github.com/absolutelyZero/openidea-plugin-stock-ticker`（**public**，main）。publisher = `absolutelyzero`，owners = `absolutelyZero`，许可证 MIT。
- 补齐材料：根与包内各一份 `LICENSE`；`plugin.toml` 补 `authors`/`license`/`homepage`/`change-notes`；**移除未使用的 `clipboard` 能力**（生产代码从未调用 `copy_to_clipboard`，仅测试 mock 曾声明）；README 新增「Capabilities and why」逐项说明（registry 硬性要求）与源码构建说明。
- `cargo test` 55 passed；`openidea-plugin build` → `dist/plugin.wasm` 323.6 KiB + `check ok`；能力降为 5 项（3 个 optional net + events + storage）。
- `keygen` 生成 publisher 密钥（私钥文件仅存于被 gitignore 排除的 `.workbuddy/tmp/absolutelyzero.key`，**绝不入库/不外发/不展示内容**），公钥 `lQ9T2cTXJkKXSX/wIZVsGMeCdifSrBBNDZiHItH5bvU=`。
- `pack . --key … --publisher absolutelyzero` → `.workbuddy/tmp/absolutelyzero.stock-ticker-0.1.0.oiplugin`（133.0 KiB, signed by absolutelyzero）；`check <包>` **ok**；SHA-256 `a4175d19d258cf87737142ebfb751610a6f09fb3311c149e9eb153a7d782669d`。
- PR 草稿已生成于 `.workbuddy/tmp/registry-pr/`：`publishers/absolutelyzero.toml`、`plugins/absolutelyzero.stock-ticker.toml`，格式对照 registry 现有 `aritti.*` 条目核对一致；registry 中无同名项。
- **已执行**：本地提交 `0f42f6d` 已 fast-forward push 到 main（远程 `plugin.toml`/`LICENSE` 已核实同步）；annotated tag `v0.1.0` 已推送，签名包 SHA-256 未变。
- **仍待用户执行**：把签名包作为 `v0.1.0` Release 附件上传；向 `l0ng-ai/openidea-plugins` 提 PR（需 fork）。本机无 `gh` CLI 与 GitHub token，且按权限边界不从 credential manager 提取凭据代发；Release API/PR 属需用户身份的外发动作。
- **风险**：registry CI 用**最新 release（v0.1.40 / API 1.4）** 的 CLI 跑 `check`，而本包由 0.1.39 / API 1.3 构建；文档称已发布的 1.3 插件继续可用，但本地 Windows 无法闭环验证，需在 CI 结果中确认。签名包内容一旦发布不可改，只能发新版本。
- **2026-11-10 改 publisher**：将发布者从模板占位 `acme` 改为真实发布者 `absolutelyzero`（与 GitHub 用户名一致）。已改 `plugin.toml` 的 `id` 前缀、`src/lib.rs` 测试上下文、`DESIGN.md` 示例；重新 `keygen`（新公钥 `lQ9T2cTXJkKXSX/wIZVsGMeCdifSrBBNDZiHItH5bvU=`）+ `pack --publisher absolutelyzero` 生成新签名包 `absolutelyzero.stock-ticker-0.1.0.oiplugin`（SHA `a4175d19d258cf87737142ebfb751610a6f09fb3311c149e9eb153a7d782669d`）；PR 草稿改为 `publishers/absolutelyzero.toml`、`plugins/absolutelyzero.stock-ticker.toml`。上一条 `0f42f6d`/tag `v0.1.0` 为 acme 版，本次将重新提交并把 tag 移动到新 commit。

---

## 7. 生命周期与时序

### 7.1 `activate(ctx)`
1. 从 storage 恢复 `watch.v1` / `quotes.v1` / `sort.v1`（损坏 → 默认 + `log(Warn)`，不使激活失败）。
2. `host::subscribe(EventKind::Scheduled)`（需 `events`）。1.3 的 `EventKind` 无 `SettingsChanged`；设置值由 `host::setting*` 在 `render` 时实时读取。
3. `host::set_status_item("index", 初始文本)`；`publish(&state)`。

### 7.2 `sync()`
```
let task = host::begin_task("刷新行情", true);
task.report(0, Some(3), Some("请求行情"));
let req = HttpRequest { timeout_ms: Some(4000),
                        ..HttpRequest::get(&url).header("Accept","application/json") };
match host::fetch(&req) {
  Ok(r) if r.status == 200 => 解析 → storage::set("quotes.v1") → 预警检查
  Err(Error::Denied(_))    => 不报错；状态栏「网络权限未授予」，展示缓存
  Err(Error::Cancelled)    => 直接返回，不覆盖缓存
  其他                      => 备源 / 保留缓存 + stale + log(Warn)
}
task.report(3, Some(3), None);
publish(state);   // refresh_view + 状态栏
```
节流：`now - last_fetch_at < 55 s` 跳过（但 `handle_events` 仍显式 `publish`）。

### 7.3 `Views::render(view, viewport, ctx)`
- `view == "watchlist"`：`Body::sortable_table(&table, Some(state.sort))`；`View::new(...).button(刷新/添加/打开/移除).status(...)`。空自选时仍返回该表（`total_rows = 0`）。
- 其它 `view`：`Body::Message("unknown view ...")`。

列：`代码 | 名称 | 现价 | 涨跌%`，共 4 列。排序索引上限 3。

### 7.4 `Views::handle(view, event, ctx)`
| 来源 | 事件 | 处理 |
| --- | --- | --- |
| 唯一标签 | `Button(id)` | `run_command(id)` |
| 唯一标签 | `event.sort()` | 写 `state.sort` + 存 + `refresh_view` |
| 唯一标签 | `event.table_activate()` | `symbols[row]` → `host::open_url` |
| 唯一标签 | 未知 `Other` / `Message` | 忽略 |

**行号铁律**：所有行号按当前 `table_rows()` 顺序解析，排序或数据变化后由 `render` 重新计算。

### 7.5 预警与状态栏
`sync()` 后比对 `above`/`below`；命中且 `alert.<symbol>` 非当日 → `notify_with_actions(Warning, "...", body, &[NotificationAction::new("打开面板","reveal")])`。`reveal` 调 `host::reveal_view("watchlist")`。状态栏 `set_status_item` 更新为 `上证 3100.50 ▲0.82% · 09:00:03`。

---

## 8. manifest（`plugin.toml`）
```toml
id = "absolutelyzero.stock-ticker"
name = "Stock Ticker"
version = "0.1.0"
description = "A-share quotes in a single native side tool window."
categories = ["tools"]
engines.openidea = "^1.3"
wasm = "dist/plugin.wasm"
activation = ["on-view:watchlist", "on-schedule:60",
              "on-event:editor-focused", "on-event:file-saved"]

[capabilities]
required = ["storage", "events"]
optional = ["net:push2.eastmoney.com", "net:push2delay.eastmoney.com", "net:searchapi.eastmoney.com", "clipboard"]

[[contributes.commands]]
id = "refresh"            # Stock: Refresh Quotes
id = "add"               # Stock: Add Symbol
id = "open"              # Stock: Open Quote Page
id = "remove"            # Stock: Remove Symbol
id = "reveal"            # Stock: Show Watchlist
id = "test-connection"   # Stock: Test Data Source

[[contributes.views]]
id = "watchlist"
title = "行情"
location = "right"
group = "stock"

[[contributes.statusBar]]
id = "index"
text = "行情 —"
tooltip = "点击刷新行情"
command = "refresh"

[[contributes.keymaps]]
command = "refresh"
key = "Ctrl+Alt+S"

[contributes.settings]
title = "Stock Ticker"
[[contributes.settings.pages]]
id = "general"
title = "常规"
schema = "settings/general.json"
[[contributes.settings.pages.actions]]
title = "测试连接"
command = "test-connection"
```
（早期方案的 `copy-code`/`open-quote`/`set-alert` 命令随右键菜单一并移除。）

**权限最小化**：`storage`/`events`=low；三个 `net:<host>`（行情主源、备源、联想搜索）与 `clipboard`=medium，均为 optional、可单独撤销。不申请 `process:*`、`fs.*`、`secrets`、`net.http:*`（1.4）。`Error::Denied` 一律降级：行情降级为缓存；搜索降级为「请授权 `net:searchapi.eastmoney.com`」的可操作提示。

---

## 9. 模块布局
```
stock-ticker/
├─ plugin.toml
├─ Cargo.toml              # cdylib, edition 2024, serde/serde_json, openidea-plugin(1.3)
├─ settings/general.json
├─ icon.svg / icon_dark.svg  # 插件 SVG 图标（浅色 / 暗色），各 925 B
├─ src/
│  ├─ lib.rs               # Plugin + Views；register_plugin!(StockPlugin, views)
│  ├─ state.rs             # storage 读写、DisplayConfig（无 colors 字段）
│  ├─ model.rs             # 纯函数：normalize / ordered_rows / index_rows / Row
│  ├─ quote.rs             # 东财行情 JSON 解析（ulist.np）
│  ├─ search.rs            # 联想接口模糊搜索（代码 / 中文名 / 拼音首字母）
│  └─ sync.rs              # begin_task + fetch + 降级 + 预警 + publish
├─ tests/fixtures/*.json   # 真实响应快照
└─ dist/plugin.wasm
```
（`web.rs` / `protocol.rs` / `table.rs` 已删除：不再需要 HTML 帧 / 原生双标签。）
依赖仅 `serde` / `serde_json` + SDK；全部可编译到 `wasm32-wasip2`。

---

## 10. 错误处理与边界
| 场景 | 行为 |
| --- | --- |
| `Error::Denied("net:...")` | 保留缓存，状态栏降级提示，表格照常渲染 |
| `Error::Cancelled` | 静默返回，不覆盖 `quotes.v1` |
| HTTP 非 200 / 8 MiB 截断 | 走备源；仍失败保留缓存并标 `stale` |
| JSON 失败 / `data:null` / 缺字段 / `"-"` | 字段 `None` → `—`；整帧失败不覆盖缓存 |
| `f124` 缺失或过期 | 状态行「数据陈旧 Ns」 |
| 自选股为空 | 返回 `total_rows=0` 的空表 |
| 行号越界/旧索引 | 忽略事件，不 panic |
| 未知消息 / 未知 `Other` | 忽略（前向兼容） |
| storage JSON 损坏 | 默认值 + `log(Warn)`，`activate` 不失败 |
| 实例回收重建 | `activate` 从 storage 全量恢复 |

**预算**：命令/激活 5 s 墙钟，task 打开时 60 s；256 MiB 内存；storage 64 MiB；fetch 响应 8 MiB；状态栏 ≤ 8；通知按钮 ≤ 3；空闲 5 分钟回收；`on-schedule` 最小 60 s；自选上限 200 行。

---

## 11. 安全与合规
- 能力 deny-by-default；`net` 按 host 精确声明，安装时逐条批准、可撤销；仅 HTTPS。
- 无凭据、无 token、无 `secrets`、无 `process:`、不读写项目/用户文件、不收集外发持仓。
- 所有外部输入按不可信数据解析，不 `unwrap`/`expect`；`panic = "abort"`。
- 真实持仓成本只落插件私有 storage（卸载即删）。

---

## 12. 测试方案

### 12.1 Native 单测（`cargo test`，无需 IDE、无网络）
```
let mock = MockHost::install().with_capabilities(&[
    "storage","events","clipboard","net:push2.eastmoney.com",
    "net:push2delay.eastmoney.com","net:searchapi.eastmoney.com"]);
mock.on_fetch(|_| Ok(HttpResponse { status:200, headers:vec![], body: FIXTURE.as_bytes().to_vec() }));
let mut p = StockPlugin::default();
p.activate(&mock.context("absolutelyzero.stock-ticker")).unwrap();
assert_eq!(p.render("watchlist", None, &ctx).unwrap().body, Body::Other{kind:"table",..});
assert!(mock.refreshed_views().contains(&"watchlist".to_string()));
```

| 用例 | 断言 |
| --- | --- |
| 正常解析 | 数值与 fixture 一致、`storage["quotes.v1"]` 已写、`refreshed_views` 含 watchlist |
| 空自选 | `total_rows=0` 的表，不发起请求 |
| 缺字段 / `"-"` / `null` / 非法 JSON | 不 panic，字段 `None`，旧快照不被覆盖 |
| `Error::Denied("net:...")` | 命令 `Ok`，状态栏降级，缓存仍可渲染 |
| HTTP 500 / 超时 | 走备源（`requests()[1].url`），再失败则 stale |
| 箭头样式 | `changeStyle="sign"` 时单元格无 `▲`；默认有 `▲` |
| `event.sort()` | 解码后 `state.sort` 更新并写 `storage` + `refresh_view` |
| `table-activate` | 双击行 → `opened_urls()` 含对应行情页 URL |
| 定时刷新 | `handle_events([Scheduled])` → 强制拉取 + `refreshed_views` 增长 |
| 活动心跳 | 陈旧的 `EditorFocused` 触发一次拉取；15 s 内重复事件不再请求；结果列表态跳过 |
| 事件订阅 | `activate` 订阅 `Scheduled` + `EditorFocused` + `FileSaved` |
| 模糊搜索 → 面板列表 | add 后 `Body::List`、2 项、`watch` 仍空、工具栏 2 个、status 含匹配数 |
| 拼音首字母 | `dftx` → 只剩 A 股「东方通信」，美股 `105.DFTX` 被过滤 |
| 结果行激活 | `ViewEvent::Activate(key)` → 添加该 secid 并切回 `table`，工具栏恢复 4 个 |
| 返回 / 单击防护 | `back` 丢弃结果；`Select` 不误加（方向键也会发 select） |
| 预警去重 | 同日第二次 sync 不再产生通知 |
| 实例回收 | 丢弃实例再 `activate`，自选股/快照/排序完整恢复 |
| 权限最小化 | 无能力时 `activate` 不失败、命令不崩 |

### 12.2 构建 / 集成验证
```
rustup target add wasm32-wasip2
cargo test                 # 55 passed
openidea-plugin build      # → dist/plugin.wasm，随后 check
openidea-plugin check      # 权威校验组件与 1.3 宿主匹配
openidea-plugin link .
```
`check` 是判定「组件导入/导出是否与 1.3 宿主完全匹配」的权威手段。

### 12.3 手工验收
1. 授权提示只列出声明的 5 项能力。
2. 右侧出现单一「行情」工具窗，4 列纯文本，指数行置顶。
3. 点表头可排序；排序后双击仍命中正确股票。
4. 涨跌以 `▲▼` 表示（无红绿）；切换 `sign`/`arrow` 即时生效。
5. 手动刷新、`Ctrl+Alt+S`、60 s 自动刷新均生效。
6. 双击行打开浏览器行情页；「添加」输入 `东方` / `dftx` 并回车后面板**原地**显示匹配列表，双击/回车某行添加、「返回」回表格；选中某行后「移除」弹出确认框即删（不再二次选择）。
7. 撤销 `net:*` 权限后仍能显示缓存并提示。
8. 预警触发一次、同日不重复；重启后状态保留。
9. 卸载后 storage 与状态栏项消失。

**验证状态：未执行目视** —— 需在 IDE Settings › Plugins 内 Reload 后人工确认。

---

## 13. 构建、安装与分发
```
# 必须从 v0.1.39 装 CLI，否则生成 1.4 SDK 组件
cargo install --locked --git https://github.com/l0ng-ai/openidea --tag v0.1.39 ide-host --bin openidea-plugin
rustup target add wasm32-wasip2
openidea-plugin new stock-ticker
```
生成后**必须核对 `Cargo.toml` 里 `openidea-plugin` 依赖指向 1.3 SDK**；若指向 main/1.4，`check` 会拒绝。开发循环：`build` → Settings › Plugins › Reload。分发（后续）：`keygen` → `pack --key --publisher` → `check` → 提 PR。

---

## 14. 里程碑
| 阶段 | 交付 | 验收 |
| --- | --- | --- |
| M0 骨架 ✅ | v0.1.39 SDK、manifest、单标签、状态栏 | `build`/`check` 通过 |
| M1 数据 ✅ | 解析 + 双源/任务/预警 + 持久化 | 含真实端点契约快照 |
| M2 彩色视图（已弃用） | 早期 `web.rs` HTML 帧 + 三档配色 | 后因刷新/选中失效回退 |
| M3 单标签精简 ✅ | 删除 `positions` 视图/`table.rs` | 增删/排序单测通过 |
| M4 自动化 ✅ | `on-schedule:60`、设置实时读取、状态栏、预警去重 | 节流/预警单测通过 |
| M5 加固 ✅ | 备源回退、脏数据降级、权限缺失降级 | 全部单测通过 |
| **M6 回退原生表格 ✅** | **删除 `web.rs`/`protocol.rs`，`render` 改 `Body::sortable_table`；`handle` 用 `sort()`/`table_activate()`；`publish` 走 `refresh_view`；`handle_events` 每 tick 保证刷新；去掉红绿配色** | **35 单测通过，wasm 构建通过** |
| 本机挂载 ✅ | `openidea-plugin link` | `links` 列出该目录；需 IDE Reload |
| IDE 目视验收 | 手工清单 1–9 | **待用户在 IDE 内确认** |

---

## 15. 风险与回滚
| 风险 | 缓解 |
| --- | --- |
| 误用 main 的 SDK/CLI → 组件是 1.4，被 1.3 宿主 `check` 拒绝 | 锁定 `--tag v0.1.39`；生成后核对 `Cargo.toml` 与 `API_VERSION` |
| 行情接口非官方，限流/改字段/`fltt` 语义冲突 | 字段容错、双源降级、契约测试、状态行显示数据时间 |
| 60 s 刷新下限 | 定位「扫一眼」；toolbar/快捷键即时刷新 |
| 原生表格无 per-cell 颜色 | 放弃红绿，改用 `▲▼`/+`-` 表达方向（用户已确认可接受） |
| 版本演进快 | host 调用收敛在 `state.rs`/`sync.rs`；CI 固定跑 `check` |

**回滚**：`openidea-plugin unlink` 或卸载插件即完成；`storage` 随卸载清除；无外部进程、无系统级安装、不改项目文件。

---

## 16. 待确认
| # | 项 | 状态 |
| --- | --- | --- |
| ① | `engines.openidea` 版本 | **已定 `^1.3`** |
| ⑥ | 市场范围与配色 | **已定 A 股 MVP + 原生表格、无红绿** |
| ② | `on-schedule` 是否还需 `subscribe(Scheduled)` | 两者都做，保守 |
| ③ | `set_status_item` 是否需静态 `statusBar` 声明 | 两者都做，保守 |
| ④ | 东财 `fltt=2` 数值语义、`f124` 是否稳定 | **已实测结案**：真实小数；`f124` Unix 秒 |
| ⑤ | `push2delay` HTTPS 与 schema 一致性 | **已实测结案**：可用、结构一致 |
| ⑦ | VS Build Tools 是否足以编译 CLI | **已实测结案**：足够 |
| ⑧ | 对外发布行情数据合规 | 仅自用阶段不阻塞 |
| ⑨ | 脚手架生成的 `Cargo.toml` SDK 是否指向 1.3 | 已 vendored 到 `vendor/openidea-plugin`，显式 `path` 引入 |
| ⑩ | `refresh_view` 是否重绘原生表 | **已实测结案**：原生表随 `refresh_view` 重绘（HTML 帧不重绘，故回退） |

---

## 附录 A–D：早期执行记录（摘要）
- **M0**（2026-11-09）：vendored SDK、单标签骨架、`cargo test` 8 passed、`build`/`check` ok、`link` 成功。
- **M1–M5**：落地 `model/quote/state/web/protocol/table/sync`；真实端点实测 `fltt=2`；`cargo test` 达 51→56 passed；`build` 352 KiB；`link` 成功。
- **列精简 + 着色修复**（12:35）：4 列、`Row.dir` 显式方向、三档配色；56 passed。
- **单标签收尾**：删除 `positions`/`table.rs`，保留 `watchlist` 彩色单标签。

## 附录 E：回退原生表格（2026-11-09 13:43）
用户反馈「行情页没有自动刷新，退回到原生列表吧，我不需要红绿了」。

| 改动 | 说明 |
| --- | --- |
| 删除模块 | `src/web.rs`（HTML 帧）、`src/protocol.rs`（帧↔插件消息） |
| `lib.rs` | `render()` 改 `Body::sortable_table(&table, Some(state.sort))`；`handle()` 用 `event.sort()` / `event.table_activate()`；`publish()` 走 `host::refresh_view`（不再 `post_view_message`）；`handle_events` 每 tick 显式 `publish`；`pick_symbol` 仅用 `host::pick`；移除 `copy-code`/`open-quote`/`set-alert` 命令 |
| `model.rs` | `Row` 去掉 `tones`/`dir`，仅含 `symbol`+`cells`；`index_rows` 改为 4 列统一；移除 `tone` |
| `state.rs` | `DisplayConfig` 移除 `colors` 字段与 `colorScheme` 读取 |
| `plugin.toml` | 移除 `copy-code`/`open-quote`/`set-alert` 命令及彩色描述 |
| 验证 | `cargo test` = **35 passed / 0 failed**；`cargo build --release --target wasm32-wasip2` 成功，`dist/plugin.wasm` = 322,633 字节；`check` 待跑 |
| 待用户操作 | OpenIDEA Settings › Plugins › Reload，按 §12.3 目视验收（自动刷新、双击打开、表头排序） |
