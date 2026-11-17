# Design

## Context

行情面板是 WebView 页面（`views[].web = "web/index.html"`），页面只渲染、不取数，通过 `openidea.postMessage` 与插件通信；插件取数后用 `host::post_view_message` 回推快照（见 `webview-auto-refresh`）。当前 `State.watch: Vec<WatchItem>` 的顺序只在 `ordered_rows` 里被当作平局决胜，实际显示顺序完全由 `State.sort: TableSort` 决定，`watch.v1` 保存的插入顺序从未成为可见顺序。

约束：wasm 实例空闲 5 分钟后回收，`host::storage` 是唯一真源；`TableSort` 来自宿主 crate 不可改；API 1.3 无拖拽相关原生能力，交互必须落在页面 JS。

## Goals / Non-Goals

**Goals:**
- 用户拖拽即可定义自选股顺序，且该顺序成为跨刷新/重启/重载的权威顺序。
- 表头列排序保留为临时视图，与自定义顺序互斥切换，互不破坏对方的数据。
- 拖拽与选中、打开、移除、定时刷新共存且不互相误触发。

**Non-Goals:**
- 列宽/列顺序的调整、跨面板拖拽、触摸手势。
- 改变数据源、列集合、刷新节奏、红绿约定与命令集合。
- 引入新的宿主 API 或提升 `engines.openidea` 版本要求。

## Decisions

### 1. 用「视图模式」字段区分自定义顺序与列排序
在 `State` 增加 `order_mode`（取值 `custom` / `column`），持久化到独立 key `order-mode.v1`。
- `custom`：`ordered_rows` 跳过排序，直接按 `watch` 的顺序输出行。
- `column`：维持现有按 `sort` 列排序（缺值排末尾、symbol 作稳定决胜）的行为。

拖拽消息把模式切到 `custom`，表头 `sort` 消息切到 `column`。这样「拖拽改底层顺序」与「列排序仅临时视图」互不覆盖：列排序从不写 `watch.v1`，拖拽重排从不改 `sort`。

**替代方案**：把顺序单独存到 `order.v1`。被否决——`watch.v1` 本身是有序数组，额外 key 会造成两份顺序真源与同步负担。

### 2. `reorder` 消息携带完整 symbol 列表，插件端做权威重排
页面在 drop 时按 DOM 顺序收集所有 `data-symbol`，发送 `{"type":"reorder","order":[symbols...]}`。插件按列表重排 `state.watch`：
- 按 `order` 中出现的先后，从原 `watch` 中取出对应项；
- 未在 `order` 中列出的项（防御性处理，正常页面不会遗漏）按原相对顺序追加到末尾；
- 忽略 `order` 中的重复项与不存在的 symbol。

重排后写回 `watch.v1`、切到 `custom` 模式并保存、然后 `publish` 回推新快照。页面不自行重排数据模型，只发消息——重排逻辑只在插件一处，保证数据与显示一致且可单测。

### 3. 页面用原生 HTML5 Drag and Drop
在 `<tr>` 上设 `draggable="true"`，`dragstart` 记录被拖 symbol；`dragover` 在目标行显示插入位置（上/下由鼠标相对行中点决定，加 CSS 类画一条指示线）并 `preventDefault` 以允许 drop；`drop` 计算最终顺序、更新本地 `state.rows` 立即重绘、再发 `reorder` 消息；`dragend` 清理样式与拖动状态。

**替代方案**：pointer events 自绘拖拽。被否决——需自行实现滚动、命中与占位，复杂度高；HTML5 DnD 在 IDE 内嵌 WebView（Chromium）可用且足够。降级：`draggable` 不可用时退化为原有的表头排序，不破坏既有功能。

### 4. 拖拽与点击/双击的冲突处理
`dragstart` 设置 `dragging` 标志，`dragend` 在下一帧清除。`click` / `dblclick` 处理器首先检查该标志（或记录拖拽结束时间戳，短窗口内忽略点击），避免一次拖拽被识别为选中或打开。选中高亮在重绘后按 `state.selected` 恢复（现有逻辑）。

### 5. 加载兼容与默认值
`State::load` 读取 `order-mode.v1`，缺失或非法时回退为 `column`（保持旧版本升级后行为不变），不影响 `watch.v1` / `sort.v1` 的读取。

## Risks / Trade-offs

- **定时刷新打断拖拽** → 刷新回推的快照会触发 `renderRows` 重绘。拖拽期间若收到快照，页面延迟应用该快照直到 `dragend`，避免拖拽中被重绘中断；`reorder` 后的即时重绘与随后的快照顺序一致。
- **拖拽过程中插件实例回收** → `reorder` 消息即时持久化，页面 drop 后即更新本地顺序，即便实例回收、重载后也从 `watch.v1` + `order-mode.v1` 恢复正确顺序。
- **DOM 顺序与数据不同步** → 页面 drop 后先按 DOM 顺序更新 `state.rows` 再发消息；插件回推的快照是最终权威，二者内容一致（同一 symbol 列表）。
- **HTML5 DnD 在目标 IDE WebView 的兼容性** → 用当前 IDE（用户环境 0.1.45）实测；不兼容时保留表头排序作为可用路径，并在 tasks 中留人工验证关卡。
- **被拖行是唯一选中项** → 重排后按 symbol 恢复高亮，选中状态不因 DOM 重建丢失。

## Migration Plan

无需数据迁移：新增 key 缺失时回退到 `column` 模式，旧自选股与排序数据原样保留。发布遵循现有流程（版本递增、签名、registry 登记）；回滚只需移除新消息分支与字段，`watch.v1` 仍是合法的有序列表。

## Open Questions

（无。范围与关键行为已与用户确认；兼容性降级路径已在决策 3 中给出。）
