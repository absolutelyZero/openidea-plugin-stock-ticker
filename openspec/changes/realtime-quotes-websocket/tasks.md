# Tasks

## 1. 阶段 0：端点与协议验证（硬门槛）

- [ ] 1.1 定位可用的 WSS 行情端点与握手要求（host、path、subprotocol、必要的 header 或首个订阅帧），记录候选与实测状态码；验证：至少一个候选返回 `101` 并能保持连接 30 秒以上。
  - **实测（2026-11-16）**：`push2.eastmoney.com/webpush/` → **403**（换 4 种 Referer、加 `Sec-WebSocket-Protocol: webpush` 仍 403）；`push2delay.eastmoney.com/webpush/`、`push2his.eastmoney.com/webpush/` 同 403；`qt.gtimg.cn`、`proxy.finance.qq.com`、`web.sqt.gtimg.cn` 根路径均 200 非 101。**未找到返回 101 的公开端点。**
- [ ] 1.2 用最小客户端完成"建连 → 订阅一只标的 → 收到帧 → 解析出代码/价格/行情时间"；验证：输出端点、订阅帧与响应帧格式的书面说明。
  - **阻塞**：1.1 未通过，无端点可订阅。
- [x] 1.3 Gate 决策：若无法取得稳定端点，或协议需要不可得的凭据/签名，记录结论并终止本变更（不进入第 2 组）。
  - **结论：终止**。东财 `webpush` 需要未公开的浏览器鉴权（403），腾讯无 WS（仅 HTTP 轮询）；`akshare-proxy-patch` 一类方案需要付费 TOKEN 才能绕过封控，不适合作为插件内置数据源。

## 2. 升级工具链与清单

- [ ] 2.1 vendor 1.6 SDK，把 `engines.openidea` 改为 `^1.6`，运行 `openidea-plugin build`/`check`；验证：`check ok`，且插件 API 版本显示 1.6。
- [ ] 2.2 `plugin.toml` 增加 `on-event:websocket-ready` 与 WSS 所需的 `net:<host>`；验证：`check` 通过，能力列表只多出预期项。

## 3. 实时通道

- [ ] 3.1 新增 `src/ws.rs`：封装 connect / drain / close，单连接按 symbol 路由；验证：用 MockHost 的 `websockets` 断言连接与出站订阅帧。
- [ ] 3.2 帧解析并合并进现有 `Snapshot`，同一 symbol 按 `Quote.ts` 取较新者；验证：晚到的旧帧不覆盖新值的测试。
- [ ] 3.3 断线重连：1→2→4→…→30 秒封顶，收到 `closed` 立即重建；验证：连续 close 后重连间隔递增的测试。
- [ ] 3.4 异常处理：未知帧忽略、队列溢出后关闭并重建连接；验证：`push_websocket_message` 超限与坏帧两条用例。

## 4. 事件与生命周期

- [ ] 4.1 `handle_events` 增加 `websocket-ready` 分支：`drain` 后更新快照并 publish；验证：投递事件后 `snapshot` 变化。
- [ ] 4.2 `activate` 建立、`deactivate` 释放连接；验证：停用后 mock 断言连接已关闭。
- [ ] 4.3 无项目时降级：`project()` 为 `None` 不建连，轮询与事件心跳照常；验证：该场景下单测通过。

## 5. 文档

- [ ] 5.1 README 增加实时能力、IDE ≥ 0.1.43 要求与降级行为；验证：文中 host/能力与 `plugin.toml` 一致。
- [ ] 5.2 DESIGN 增加 WS 架构、重连策略、合并规则与端点风险；验证：与 design.md 无矛盾。

## 6. 构建与发布

- [ ] 6.1 运行 `cargo test`、`openidea-plugin build`，记录 wasm 体积；验证：测试与 `check ok` 输出。
- [ ] 6.2 版本升到 0.3.0 后签名 `pack`，对签名包 `check` 并记录 SHA-256；验证：`check ok` 且 SHA 已记录。
- [ ] 6.3 在 registry 草稿追加 0.3.0 条目；验证：通过 `registry.py` 的 URL/SHA 规则。

## 7. 集成验证

- [ ] 7.1 IDE 内面板静止 2 分钟，行情时钟持续前进；验证：时间戳连续变化而非冻结。
- [ ] 7.2 断网再恢复：观察退避与自动恢复；验证：日志显示重连且报价恢复。
- [ ] 7.3 无项目窗口：确认降级为轮询且状态行说明；验证：仍能按事件刷新。

## Workflow follow-up

- 评审通过并完成 7.x 后归档该变更，确认主 spec 生成。
- 归档后复核 `openspec/specs/market-data/realtime-quotes/spec.md` 的 Purpose 与需求。
