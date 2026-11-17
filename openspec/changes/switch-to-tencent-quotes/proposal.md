# Proposal

## Why

当前行情与检索依赖东方财富的三个接口。上线前实测发现：同一时段 `push2.eastmoney.com` 的批量行情请求返回 `HTTP 000`（TLS 连接被中断），而腾讯 `qt.gtimg.cn` 在同一时刻稳定返回（单只 540 B / 0.11 s，30 只 15,408 B / 0.14 s，连续 20 次请求全部 200）。东财返回的是体量更大的嵌套 JSON，解析与失败定位成本都更高。

腾讯财经公开行情接口更轻、更稳；其 JSON 检索接口按 `[市场, 代码, 名称, 拼音, 类型]` 返回最多 50 条结果，可直接支撑现有的中文名 / 拼音首字母 / 代码检索（轻量的 `smartbox` 文本接口对「银行」这类宽泛词只返回 ETF，无法满足中文名检索）。把行情与检索统一切到腾讯后，网络能力从 5 项收敛为 4 项，授权面与审核面同时缩小。

刷新节奏不变：`on-schedule` 仍为 60 秒（IDE 硬下限），活跃心跳仍为 15 秒。本变更只更换数据源与解析链路，不追求更快的刷新。

## What Changes

- 行情批量获取改为 `https://qt.gtimg.cn/q=<code,code,...>`：一次请求覆盖整个自选股与指数，解析 `~` 分隔文本。
- 标的检索改为腾讯 JSON 检索接口 `https://proxy.finance.qq.com/ifzqgtimg/appstock/smartbox/search/get?q=<query>`：`data.stock` 按 `[市场, 代码, 名称, 拼音, 类型]` 返回最多 50 条，只保留 `GP-A`。
- 腾讯返回 `charset=GBK`，新增 GBK→UTF-8 解码用于股票名称；数字字段本身是 ASCII。
- **BREAKING** 网络能力变更：移除 `net:push2.eastmoney.com`、`net:push2delay.eastmoney.com`、`net:searchapi.eastmoney.com`；新增 `net:qt.gtimg.cn`、`net:proxy.finance.qq.com`。已安装用户升级时需重新授权新 host。
- 移除东财专用的响应模型与字段映射；请求失败时给出明确提示，不静默。
- 版本升到 0.2.0，重新构建、签名并登记到 registry。
- 不变：`on-schedule:60`、15 秒活动心跳、`storage` / `events` 能力、视图与命令结构。

## Capabilities

### New Capabilities
- `market-data/quotes`: 自选股与指数的批量报价获取、字段解析、刷新触发与失败处理。
- `market-data/symbol-search`: 按中文名、拼音首字母或代码检索 A 股标的。

### Modified Capabilities
（无。项目中尚无既有 spec。）

## Impact

- 代码：`stock-ticker/src/sync.rs`（行情 URL 与解析）、`src/search.rs`（检索）、`src/state.rs`（字段映射）、`src/lib.rs`（错误提示与刷新路径）、`Cargo.toml`（可能新增 GBK 解码依赖）。
- 配置：`stock-ticker/plugin.toml` 的 `[capabilities] optional`。
- 依赖与体积：GBK 解码可能增加 wasm 体积，需实测。
- 发布：0.2.0 需重新 `build` / `pack` / 签名，registry PR 增版本条目；README 与 DESIGN 的能力理由、限制说明同步更新。
- 不在范围：刷新间隔调整、WebSocket 实时推送、视图与命令结构变更。
- 待确认：GBK 解码方案对 wasm 体积的实际影响；腾讯接口的长期限流表现（当前只有 20 次短时样本）。
