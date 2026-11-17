# Tasks

## 1. 支持性验证（Gate）

- [x] 1.1 用最小 WebView 视图验证 `views[].web` 在 1.3 SDK 下是否可用（页面 `postMessage` → 插件回推 → 页面收到）；验证：一次完整往返成功，记录是否需要升 `^1.4`。
  - **结果**：探针视图 `webprobe` 收到 `{"type":"pong","echo":"{\"type\":\"ping\"}"}`，往返成功；`v0.1.39/crates/ide-host/src/plugins/web.rs` 已有运行时加载逻辑，**无需升 `^1.4`**。
- [ ] 1.2 确认页面约束：无网络访问、内联脚本可执行、`data-theme` 与主题变量可用；验证：页面按主题渲染且网络请求被拒。

## 2. Manifest 与 Rust 视图

- [x] 2.1 `[[contributes.views]]` 改为 `web = "web/index.html"`，`render` 返回占位 `View::message`；验证：`check` 通过，视图能加载页面。
- [x] 2.2 实现 `on_message` 分发（`refresh` / `prefs` / `remove` / `open` / `sort` / `add` / `hello`）与 `host::post_view_message` 回推；验证：Rust 单测覆盖每种消息。
- [x] 2.3 快照序列化：报价、config（精度/箭头/间隔）、source、行情时钟、ageSec、error；验证：序列化结果包含全部字段且中文不转义损坏。

## 3. Web 页面

- [ ] 3.1 表格渲染 4 列（代码/名称/现价/涨跌%），支持表头排序、双击打开、选中行删除；验证：与改造前行为逐项对照一致。
- [ ] 3.2 定时器：首次加载立即刷新，之后按 `intervalSec`（clamp 3–300）循环；验证：改间隔后节奏随之变化。
- [ ] 3.3 状态与错误：显示数据源、行情时钟、间隔与错误提示，失败时保留旧数据；验证：模拟失败后旧表格仍在且显示错误。

## 4. 测试

- [x] 4.1 Rust 单测：消息分发、快照字段、间隔 clamp（3/300 边界）；验证：`cargo test` 全绿（34 passed）。
- [ ] 4.2 页面手动验证：排序、删除确认、双击、定时器、主题切换；验证：逐项记录结果。

## 5. 文档

- [x] 5.1 README 更新为 WebView 架构、默认 10 秒可调、IDE 版本要求；验证：与 `plugin.toml` 一致。
- [x] 5.2 DESIGN 增加 WebView 消息协议与定时器决策，并标注与此前 `Body::Html` 失败的区别；验证：与 design.md 无矛盾。

## 6. 构建与发布

- [x] 6.1 `cargo test`、`openidea-plugin build`，记录 wasm 与包体积；验证：`check ok`（wasm 487.6 KiB，web/index.html 7.9 KB）。
- [ ] 6.2 版本升 0.3.0 后签名 `pack`，`check` 并记录 SHA-256；验证：`check ok`。
- [ ] 6.3 registry 草稿追加 0.3.0 条目；验证：通过 `registry.py` 规则。

## 7. 集成验证

- [ ] 7.1 IDE 内面板静止 2 分钟：行情时钟每 10 秒前进；验证：时间戳连续变化。
- [ ] 7.2 关闭视图后不再刷新；验证：日志无新请求。
- [ ] 7.3 断网/恢复：错误可见且恢复后继续刷新；验证：状态行与页面提示。

## Workflow follow-up

- 评审通过并完成 7.x 后归档该变更，确认主 spec 生成。
- 归档后复核 `openspec/specs/ui/webview-watchlist/spec.md` 的 Purpose 与需求。
