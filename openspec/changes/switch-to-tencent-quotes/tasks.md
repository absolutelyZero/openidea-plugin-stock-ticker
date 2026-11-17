# Tasks

## 1. 能力声明与依赖

- [x] 1.1 把 `plugin.toml` 的 `[capabilities] optional` 改为 `net:qt.gtimg.cn`、`net:proxy.finance.qq.com`，删除三个东财 host；运行 `openidea-plugin check` 确认 manifest 合法且能力列表只剩 2 个 net。
- [ ] 1.2 引入 GBK 解码依赖并按 release 配置构建，记录 `dist/plugin.wasm` 体积增量；实测 **+153,050 B（+46.2%，331,406 → 484,456 B）超过 60 KB 阈值**，待决定：接受该体积，或改用 design D3 的 GBK 子集表方案。

## 2. 行情获取与解析

- [x] 2.1 用一段真实 `qt.gtimg.cn` 响应建立「字段索引 → 含义」映射（名称、代码、现价、昨收、今开、成交量、时间、涨跌、涨跌幅），把映射写入代码注释或常量表；验证：用该样本能正确取出全部核心字段。
- [x] 2.2 实现批量 URL 构造（`sh/sz/bj` 前缀推导）与 GBK 解码后的解析；验证：单元测试内嵌真实响应，30 条输入得到 30 条报价且名称正确。
- [x] 2.3 实现失败处理：非 200、连接中断、`Denied` 三种情况保留上一次快照并给出一次可见提示，不自动重试；验证：三条测试分别断言快照保留与提示内容。
- [x] 2.4 把 `sync.rs` 的东财 JSON 测试替换为腾讯契约测试（含停牌/零成交、指数、北交所样本）；验证：`cargo test` 中 sync 相关用例全绿。

## 3. 标的检索

- [x] 3.1 实现 `proxy.finance.qq.com` JSON 查询与 `data.stock` 解析（市场、代码、名称、拼音、类型）；验证：中文「茅台」、宽泛词「银行」、拼音「gzmt」、代码「600519」四类测试命中预期 A 股。
- [x] 3.2 实现只保留沪深京 A 股的过滤，剔除港美股、基金、指数；验证：内嵌混合结果样本，断言非 A 股被过滤。
- [x] 3.3 实现无匹配与未授权反馈，且不破坏当前自选股列表；验证：无匹配与 `Denied` 两条测试断言提示文案与列表不变。
- [x] 3.4 把 `search.rs` 的东财 suggest 测试替换为腾讯真实响应片段；验证：`cargo test` 中 search 相关用例全绿。

## 4. 刷新路径与状态适配

- [x] 4.1 调整 `lib.rs` 的刷新与错误提示路径，使新数据源失败时走可见提示而非静默；验证：模拟失败后 `render` 输出包含错误提示且表格保留上次数据。
- [x] 4.2 更新 `state.rs` 的报价字段映射与持久化结构，保证旧快照可读；验证：持久化 round-trip 测试通过。
- [x] 4.4 让刷新可见并去掉首次等待：状态行显示行情时间戳（`HH:MM:SS`）；`activate` 在数据陈旧（≥90 s）且有自选股时立即刷新；`FilesChanged` 纳入心跳。验证：`cargo test` 53 passed、`build` + `check` ok。
- [x] 4.3 确认 `on-schedule:60` 与 15 秒节流保持不变；activation 在 0.2.1 追加 `on-event:files-changed` 作为心跳触发源（只增加触发机会，不改变 60 s / 15 s 间隔）；验证：心跳相关测试仍通过。

## 5. 文档

- [x] 5.1 更新 `stock-ticker/README.md`：能力理由改为 2 个腾讯 host、删除东财端点表、说明升级需重新授权；验证：文中列出的 host 与 `plugin.toml` 完全一致。
- [x] 5.2 更新 `DESIGN.md` 的数据源、解析与 GBK 决策，并标注单源风险；验证：与 `design.md` 的 D1–D6 无矛盾。

## 6. 构建、签名与登记

- [x] 6.1 运行 `cargo test`、`openidea-plugin build`，确认全部通过并记录 wasm 体积；验证：测试结果与 `check ok` 输出。
- [x] 6.2 将版本升到 0.2.0 后 `pack --key --publisher absolutelyzero`，对签名包运行 `check` 并记录 SHA-256；验证：`check ok` 且 SHA 已记录。
- [x] 6.3 在 registry PR 的 `plugins/absolutelyzero.stock-ticker.toml` 追加 0.2.0 版本条目（package URL + SHA-256）；验证：条目通过 `scripts/registry.py` 的 URI/SHA 规则校验。

## 7. 集成验证

- [ ] 7.1 在 IDE 内 Reload 后验证首次请求弹出 2 个新 host 的授权、拒绝时提示正确、授权后行情正常；验证：面板显示实时报价且日志无 `Denied` 循环。
- [ ] 7.2 验证 30 只批量、指数、停牌标的的显示与涨跌幅数值与数据源一致；验证：逐行核对面板与 `qt.gtimg.cn` 原始响应。

## Workflow follow-up

- 评审通过后归档该变更（`openspec archive`），确认主 spec 正确生成。
- 归档后复核 `openspec/specs/market-data/*/spec.md` 的 Purpose 与需求完整性。
