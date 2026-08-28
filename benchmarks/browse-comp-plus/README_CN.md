<p align="right">
  <a href="./README.md">English</a> | 中文
</p>

# BrowseComp-Plus

此 benchmark 使用固定的 [BrowseComp-Plus](https://github.com/texttron/BrowseComp-Plus) 语料库，对 Codex 进行原生配对评测。

整体原则与原论文一致，但在语料处理和评测流程上略有调整，以更贴近用户实际使用通用 Agent 的场景。

每个问题均通过相互独立的配对 trial 进行评测；每次评测都使用相同的模型、prompt、语料库、Codex 配置和限制：

- **Baseline：** Codex 使用其标准工具集。
- **zvec-grep：** 保持相同的 Codex 配置，仅通过 `zg install` 增加 zvec-grep MCP 工具和使用指引。

Benchmark 记录回答质量、Token 用量、Agent 执行耗时、工具调用次数和完整的 Codex 轨迹。

## 评测结果

[最新完整报告](./LATEST_REPORT.md)提供了完整结果和复现信息。以下是本次 study 的摘要：在 300 组配对 trial 中，zvec-grep 在保持回答质量的同时，将平均 Input Token 减少 **37.56%**、工具调用次数减少 **43.52%**、Agent 执行时间减少 **38.58%**。

### 评测配置

Study 选取 100 个 case，以兼顾覆盖范围、运行时间和成本。样本并非随机抽取，而是按照锁定的 Hugging Face `test` split 原始顺序选择。我们没有在这部分数据中发现明显的顺序偏置。采用公开且固定的顺序，也能尽量减少人为选择空间，避免挑选更有利于 zvec-grep 的 case。

对于确认存在错误，或语料不足以支持确定答案的 case，我们会将其排除。所有排除项均记录在 [`suites/study.txt`](./suites/study.txt) 中。

| 配置项 | 值 |
| --- | --- |
| 评测规模 | 100 个 case · 300 组配对 trial |
| Agent | `gpt-5.6-sol` · `high` reasoning |
| Embedding 模型 | `qwen/qwen3.7-text-embedding` |

### 主要结果

所有 300 组配对 trial 均纳入均值。变化表示 zvec-grep 相对 Baseline 的差异。

| 指标 | Baseline | zvec-grep | 变化 |
| --- | ---: | ---: | ---: |
| 回答准确率 | 98.67% | 99.00% | +0.33 pp |
| Input Token | 1.68M | 1.05M | **−37.56%** |
| 工具调用次数 | 25.42 | 14.36 | **−43.52%** |
| Agent 执行时间 | 259.4 秒 | 159.3 秒 | **−38.58%** |

zvec-grep 索引准备过程与 Agent 执行过程分开测量和报告。

## 前置条件

在此目录中安装 Python 环境并检查宿主环境：

```sh
cd benchmarks/browse-comp-plus
uv sync
source .venv/bin/activate
zg-bench doctor
```

宿主环境需要提供：

- 安装了 `uv` 的 macOS 或 Linux；
- 已安装并完成身份验证的 Codex CLI；
- 已安装 `zg`。

## 准备 benchmark

下载锁定版本的官方数据，将语料库中的每个 `text` 字段原样生成为 `<docid>.md`，并构建可复用索引：

```sh
zg-bench prepare
```

首次准备需要网络连接，并需要足够的磁盘空间来存放下载的数据、生成的语料库和索引。

后续运行会复用已经完成的下载、语料库和索引阶段。

## 运行

使用一个问题验证完整的配对流程：

```sh
zg-bench run --suite smoke
```

Codex 模型和 reasoning effort 在 `benchmark.toml` 中配置。Runner 会在创建 trial 前验证配置的模型。

运行固定随机选取的 5 个问题组成的 CI 子集：

```sh
zg-bench run --suite ci
```

运行固定的 Study 子集：

```sh
zg-bench run --suite study
```

运行锁定的官方数据集中的全部样例：

```sh
zg-bench run --suite full
```

## 评测与报告

使用盲测 Codex 评审器评测最近一次运行，并生成最终报告：

```sh
zg-bench evaluate
```

仅对于 `smoke` 套件，评测还会审计 zvec-grep profile 的工具轨迹，并报告 zvec-grep 是否使用正确。该审计独立于对回答正确性的盲测评审。

如有需要，可以明确指定 run：

```sh
zg-bench evaluate <run-id>
```

重新生成最近一次运行的 Token、耗时、完成状态和配对样例报告：

```sh
zg-bench report
```

如有需要，可以明确指定 run：

```sh
zg-bench report <run-id>
```

删除所有 run 和生成的报告，同时保留下载的数据、workspace 和可复用索引：

```sh
zg-bench clean
```

## 产物

生成的数据保存在 `artifacts/` 下，不会提交到代码仓库。其中包括锁定的源数据快照、生成的语料库、可复用索引、各次运行隔离的 profile、原始尝试、评审器输入和报告。Gold data 和 manifest 始终位于 Agent workspace 之外。
