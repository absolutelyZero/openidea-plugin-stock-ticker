# Tasks

## 1. 状态与模型：视图模式与按 watch 顺序输出

- [x] 1.1 在 `state.rs` 增加 `order_mode` 字段（`custom`/`column`）与 `order-mode.v1` 持久化，`load` 缺失时回退 `column`；新增加载/回退单元测试，验证 `cargo test -p stock-ticker` 通过且旧数据（无 key）不丢失 watch/sort。
- [x] 1.2 在 `model.rs` 的 `ordered_rows` 支持自定义模式按 `watch` 顺序直接输出（`RenderOptions` 传入模式）；新增单测验证 custom 模式忽略列排序、column 模式行为不变，`cargo test -p stock-ticker` 通过。

## 2. 插件消息处理：reorder 与模式切换

- [x] 2.1 在 `lib.rs` 的 `handle` 新增 `reorder` 分支：按 symbol 列表重排 `state.watch`（未列出项追加末尾、忽略重复与无效项）、写回 `watch.v1`、切到 `custom` 模式并保存、重新 `publish`；新增单测验证顺序、持久化内容与模式，且重复/缺失 symbol 不产生重复项，`cargo test -p stock-ticker` 通过。
- [x] 2.2 修改 `sort` 消息分支：设置列排序的同时切到 `column` 模式并保存，不改 `watch.v1`；新增/调整单测验证点击排序后底层顺序不变、模式为 `column`，`cargo test -p stock-ticker` 通过。
- [x] 2.3 确保 `snapshot_json` 携带当前 `order_mode` 供页面渲染判断；`cargo build -p stock-ticker --target wasm32-unknown-unknown` 通过且快照 JSON 可解析。

## 3. 页面交互：拖拽重排

- [x] 3.1 在 `web/index.html` 实现行拖拽重排与插入位置指示：因 IDE 0.1.45 WebView 在 dragstart 后吞掉 dragover/drop，最终改用 mouse 手势（mousedown/mousemove/mouseup + Y 坐标命中）自实现；松手后按 `state.rows` 顺序发送 `reorder` 消息；JS 静态检查与手势状态机用例通过。
- [x] 3.2 处理拖拽与单击选中、双击打开、移除的冲突（拖动标志/时间窗），并在按 `order_mode` 渲染时保留选中高亮；确认 `sort` 点击切到列排序视图的逻辑仍生效。
- [x] 3.3 处理拖拽期间到达的刷新快照（延迟应用到 mouseup，重排后清空以免旧顺序回弹）；2026-11-17 用户在 IDE 0.1.45 WebView 内确认拖拽正常（诊断日志验证过事件链后已移除诊断条）。

## 4. 文档、打包与回归

- [x] 4.1 更新 `README.md` / `DESIGN.md`（若仓库存在）关于自选股排序的说明，写明拖拽自定义顺序与表头列排序的互斥关系；文档与实现一致。
- [x] 4.2 运行全量 `cargo test -p stock-ticker` 与 wasm 构建，确认无回归、快照与消息协议可用。
- [x] 4.3 按现有流程递增版本、构建/校验/签名插件包并登记 registry（版本与哈希如实记录）；`openidea-plugin check` 通过。
