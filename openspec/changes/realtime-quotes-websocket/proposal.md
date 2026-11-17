# Proposal

## Why

当前插件在 API 1.3 下无法在「面板可见且用户静止」时自动刷新：宿主只在插件 `Inactive` 时投递 `on-schedule`（`v0.1.39` 的 `ide-host/plugins/mod.rs:2106` 要求 `status == Inactive`），存活实例到点只重置计时器、不发事件。实测面板打开后行情时钟冻结，实际刷新间隔约 5–6 分钟（idle 回收后才会被 schedule 唤醒一次）。

API 1.6 引入 host-owned WebSocket：连接由宿主持有、后台运行，`websocket-ready` 是普通事件，既投递给存活实例，也能唤醒被回收的插件。这是 1.x 里唯一能实现「真正实时」的路径。

## What Changes

- **BREAKING（兼容性）**：`engines.openidea` 由 `^1.3` 升到 `^1.6`，可安装 IDE 下限抬到 **0.1.43**；重新 vendor 1.6+ SDK 并重编译。
- 新增实时通道：`WebSocket::connect(project, wss_request)` → 订阅 `on-event:websocket-ready` → `drain` 增量帧 → 更新对应标的报价；连接 Drop 即关闭，每插件最多 8 条。
- 保留现有 HTTP 轮询（`qt.gtimg.cn`）作为兜底：无打开项目、WS 连接失败或数据源不支持 WS 时继续工作；WS 与轮询共享同一 `Snapshot` 与 15 秒节流。
- 断线重连：指数退避（1 s→2 s→…→30 s 封顶），成功后恢复；队列溢出时宿主会关闭连接，需按 `closed` 状态重建。
- 版本 0.3.0，重新签名并登记 registry。
- **前置验证（阶段 0，已执行，未通过）**：对 `push2.eastmoney.com/webpush/`、`push2delay`、`push2his` 做 WS 握手均返回 **403**（多种 Referer/子协议无效）；`qt.gtimg.cn`、`proxy.finance.qq.com`、`web.sqt.gtimg.cn` 根路径返回 200 而非 101。未找到可用于第三方客户端的公开 WSS 行情端点，本变更按 Gate 终止；保留 0.2.2 的轮询 + 事件心跳。

## Capabilities

### New Capabilities
- `market-data/realtime-quotes`: 通过宿主持有的 WebSocket 接收行情推送并增量更新报价；不可用时降级为轮询。

### Modified Capabilities
（无。`market-data/quotes` 仍属于未归档的 `switch-to-tencent-quotes`，本变更不改动它的需求。）

## Impact

- 代码：`plugin.toml`（engines/activation/capability）、`Cargo.toml` + `../vendor/openidea-plugin`（1.6+）、新增 `src/ws.rs`、`src/sync.rs`（与轮询共享快照）、`src/state.rs`（连接状态）、`src/lib.rs`（事件分支）。
- 权限：`net:<host>`（优先复用行情 host）；若 WSS 使用独立域，需新增一个 `net:` 能力并让用户重新授权。
- 发布：0.3.0 需重签、registry 新增版本；README/DESIGN 更新。
- 兼容性：`^1.6` 使 0.1.39–0.1.42 的 IDE 无法安装本版本；旧版 0.2.x 仍可安装。
- 不在范围：更换数据源、改变 UI 结构、修改 60 秒 `on-schedule` 声明。
- 待确认：WSS 端点与订阅帧格式；目标 IDE 是否 ≥ 0.1.43；`Invocation` 是否始终提供 `Project` 句柄（无项目时需降级）。
