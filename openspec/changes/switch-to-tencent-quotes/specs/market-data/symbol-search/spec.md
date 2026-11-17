# Spec Delta

## Purpose

让用户按中文名、拼音首字母或股票代码快速找到 A 股标的并加入自选股，而不需要记住完整代码。

## ADDED Requirements

### Requirement: 多形式检索
系统 SHALL 支持以中文名称、拼音首字母或股票代码片段检索 A 股标的。

#### Scenario: 中文名称
- **WHEN** 用户输入「茅台」
- **THEN** 返回名称包含「茅台」的 A 股标的

#### Scenario: 拼音首字母
- **WHEN** 用户输入「gzmt」
- **THEN** 返回贵州茅台

#### Scenario: 代码片段
- **WHEN** 用户输入「600519」
- **THEN** 返回代码为 600519 的标的

### Requirement: 仅返回 A 股
系统 SHALL 只返回沪深京 A 股标的，过滤基金、债券、指数、港股与美股等非 A 股结果。

#### Scenario: 结果中混有非 A 股
- **WHEN** 数据源同时返回 A 股与美股或基金结果
- **THEN** 候选列表只包含 A 股标的

### Requirement: 无匹配与失败反馈
系统 SHALL 在检索无结果或请求失败时给出明确反馈，且不影响当前自选股数据。

#### Scenario: 无匹配结果
- **WHEN** 检索词没有任何 A 股匹配
- **THEN** 面板提示无匹配，并保留原自选股列表

#### Scenario: 检索 host 未授权
- **WHEN** 用户未授权 `net:proxy.finance.qq.com`
- **THEN** 系统提示需要授权，并说明按完整代码直接添加仍可用
