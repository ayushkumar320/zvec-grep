<p align="right">
  <a href="./README.md">English</a> | 中文
</p>

# 性能测试

每项评测均以**可复现性为设计原则**，用于衡量 zvec-grep 在不同工作负载下对 Agent 回答质量和检索效率的影响。每个 benchmark 都可独立运行。其输入和依赖均已固定，并配有专用 runner 和完整的评测流程；运行生成的文件与源码明确分离，详细说明则收录在各自的 README 中。

配置与执行命令参见各 benchmark 的 README。

## 评测项目

| Benchmark | 说明 | Agent | 研究规模 |
| --- | --- | --- | --- |
| [BrowseComp-Plus](browse-comp-plus/README_CN.md) | 在大型固定语料库上评测多文档证据检索和回答准确率 | Codex | 80 个样例 |
| [SWE-QA-Bench](swe-qa-bench/README_CN.md) | 评测代码仓库级、跨文件、多跳的软件工程问答 | OpenCode | 20 个任务 |

## 评测协议

所有 benchmark 均采用受控的配对 A/B 评测。对于每个样例，Baseline 和 Treatment 使用完全相同的任务输入、Agent、模型、环境和资源限制。

- **Baseline：** Agent 使用其标准工具和指令。
- **Treatment (zvec-grep)：** 在 Baseline 基础上，额外为同一个 Agent 提供预先构建的索引、zvec-grep 工具及其使用说明。

两组之间**唯一的预期差异**是能否使用 zvec-grep。索引准备时间单独统计，不计入 Agent 执行时间，以免影响对 Agent 实际表现的比较。

## 评测指标

根据具体情况，benchmark 会测量：

| 指标 | 衡量内容 | 趋势 |
| --- | --- | --- |
| 回答质量 | 任务的评审得分或准确率 | 越高越好 |
| 输入 Token | Agent 执行期间模型消耗的输入 Token | 越低越好 |
| 工具调用 | Agent 执行期间记录的工具调用 | 越低越好 |
| Agent 执行耗时 | Agent 的实际执行耗时，不包括另行统计的 zvec-grep 索引准备时间 | 越低越好 |

根据需要，也可能报告其他指标。为了审计和诊断，也可能保留完成状态和原始轨迹。

## 结果比较

每个 benchmark 的 README 都提供完整的复现步骤。比较结果时需要注意：

- **确保整套评测配置一致。** 模型及版本、推理参数、Agent 框架及版本、基础 Prompt、共有工具、任务集、运行环境和资源限制都应保持一致。Baseline 使用更强的模型时，完全可能优于使用 zvec-grep 的较弱模型；这样的比较无法说明 zvec-grep 的实际效果。
- **考虑随机波动。** 模型输出、工具选择、Judge 得分和运行时间都可能在不同运行之间发生变化。在条件允许时，Baseline 和 Treatment 应采用相同数量的独立试验，并在汇总结果之外报告波动范围或置信区间。
- **在运行前固定分析方案。** 预先确定评测范围、Judge、聚合方式以及失败和超时的处理规则；不要选择性保留样例，也不要静默丢弃未完成的运行。
- **保持测量条件一致。** 硬件、系统负载、网络和缓存状态都会影响耗时；不同模型、Provider 和 Agent runner 对 Token 与工具调用的统计口径也可能不同。
- **避免数据泄漏。** 参考答案、历史输出、报告和其他评测产物都应置于 Agent 可访问的 workspace 之外。
