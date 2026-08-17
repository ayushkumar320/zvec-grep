<p align="right">
  <a href="./README.md">English</a> | 中文
</p>

# 性能测试

每项评测均以**可复现性为设计原则**，用于衡量 zvec-grep 在不同工作负载下
对 Agent 回答质量和检索效率的影响。每个 benchmark 都是独立完整的项目，
具备**固定的输入和依赖**、自己的 runner 和评测流程、清晰的生成产物边界，
以及详细的 README。

配置与执行命令参见各 benchmark 的 README。

## Benchmark 套件

| Benchmark | 说明 | Agent | 研究规模 |
| --- | --- | --- | --- |
| [BrowseComp-Plus](browse-comp-plus/README_CN.md) | 在大型固定语料库上评测多文档证据检索和回答准确率 | Codex | 80 个样例 |
| [SWE-QA-Bench](swe-qa-bench/README_CN.md) | 评测代码仓库级、跨文件、多跳的软件工程问答 | OpenCode | 20 个任务 |

## 评测协议

所有 benchmark 均采用受控的配对 A/B 评测。对于每个样例，baseline 和
treatment 使用相同的任务输入、Agent、模型、环境和限制。

- **Baseline：** Agent 使用其标准工具和指令。
- **Treatment（zvec-grep）：** 同一个 Agent 额外获得准备好的索引、
  zvec-grep 工具和标准使用指引。

两组配对运行的**唯一预期差异**是能否使用 zvec-grep。为使比较聚焦于
Agent 行为，索引准备过程单独测量和报告。

## 评测指标

根据具体 benchmark，我们会测量：

- 回答质量；
- 输入 Token 用量；
- 工具调用次数；
- Agent 墙钟时间；
- 完成状态和原始轨迹。

回答质量越高、资源用量越低，结果越好。各 benchmark 使用的具体评审方式、
指标定义、聚合规则、完整结果和复现步骤，请参见对应的 README。

## 可复现性

仅应比较同一次配对评测中的 baseline 和 zvec-grep。不同模型、环境、数据集
版本、代码仓库 commit 或索引配置产生的结果不能直接比较。复现结果时，请
使用各 benchmark 锁定的依赖和文档所述的准备流程。
