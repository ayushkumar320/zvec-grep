<p align="right">
  <a href="./README.md">English</a> | 中文
</p>

<div align="center">
  <p>
    <a href="./docs/08-roadmap.md"><img src="https://img.shields.io/badge/status-work%20in%20progress-F59E0B?style=for-the-badge" alt="项目开发中" /></a>
  </p>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./.github/assets/zg-logo-dark.svg">
    <img src="./.github/assets/zg-logo.svg" width="150" alt="zg logo" />
  </picture>
  <p><strong>Know the words—or don’t. Just zg.</strong></p>
  <p>面向人与 Agent 的本地优先统一检索层。</p>

  <p>
    <a href="https://www.npmjs.com/package/@zvec/zvec-grep"><img src="https://img.shields.io/npm/v/@zvec/zvec-grep.svg" alt="npm 版本" /></a>
    <a href="https://github.com/zvec-ai/zvec-grep/actions/workflows/ci.yml"><img src="https://github.com/zvec-ai/zvec-grep/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
    <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="Apache 2.0 许可证" /></a>
    <img src="https://img.shields.io/badge/node-%3E%3D22-blue.svg" alt="Node.js 22 或更新版本" />
  </p>

  <p>
    <a href="#tour">🎬 <strong>功能演示</strong></a> |
    <a href="#features">💫 <strong>核心特性</strong></a> |
    <a href="#try-it-yourself">🚀 <strong>动手体验</strong></a> |
    <a href="./docs/README.md">📚 <strong>文档</strong></a> |
    <a href="#benchmarks">📊 <strong>性能测试</strong></a> |
    <a href="#community">🤝 <strong>社区</strong></a>
  </p>
</div>

**zg**（**z**vec-**g**rep）将 ripgrep、BM25 与向量检索统一在一个
[本地优先的检索入口](./docs/05-architecture.md)中。既可以由人在终端中搜索，
也可以让 Agent 根据问题选择合适的本地检索方式。

<a id="tour"></a>

## 🎬 功能演示

<div align="center">
  <img src="./.github/assets/zvec-grep-tour.gif" width="1000" alt="安装 Agent 集成、为工作区建索引并让 Agent 使用 zvec-grep 检索本地内容" />
</div>

<a id="features"></a>

## 💫 为什么选择 zg？

- **人与 Agent 开箱即用**：安装一次、索引一次，即可在 macOS、Linux 和
  Windows 上通过 CLI 或 Agent 复用同一个工作区。
- **不止关键词搜索**：先按语义发现内容、按相关性排序，再在需要时使用精确文本
  或正则完成验证。
- **多格式检索**：搜索源代码、文档和结构化数据，同时保留有用的内容结构与
  来源位置。
- **更少搜索，更少上下文**：经过排序并保留来源的结果，可以减少工具调用、
  Token 消耗与无关噪声，更快找到所需证据。
- **默认本地运行**：文件、索引与本地模型都留在本机；只有经过你的授权，远程
  Embedding 服务才能接收数据。

<a id="try-it-yourself"></a>

## 🚀 动手体验

### 1. 准备示例书架

```bash
# 需要 Node.js 22 或更新版本。
npm install -g @zvec/zvec-grep

mkdir zg-mystery && cd zg-mystery
curl --retry 3 --retry-all-errors --progress-bar -fL \
  -o alice-in-wonderland.txt https://raw.githubusercontent.com/GITenberg/Alice-s-Adventures-in-Wonderland_11/master/11.txt \
  -o sherlock-holmes.txt https://raw.githubusercontent.com/GITenberg/The-Memoirs-of-Sherlock-Holmes_834/master/834.txt

zg index --embedding local/potion-retrieval-32m
```

### 2. 选择检索方式

#### Agent：通过 OpenCode 提问

配置好 [OpenCode](https://opencode.ai/) 后：

```bash
zg install --target opencode --yes
opencode run --model opencode/deepseek-v4-flash-free \
  "An unseen creature left a few marks. What did the detective infer? Cite local evidence."
```

Prompt 中没有指定任何工具，OpenCode 会自主选择 zg：

```text
⚙ zvec_grep_zvec_grep_search {"root":".../zg-mystery","query":"An unseen creature left a few marks. What did the detective infer?","fts":["marks","unseen creature","inferred","detective"],"fuse":true}
→ Read sherlock-holmes.txt [offset=5440, limit=120]

Answer:
The detective is Sherlock Holmes in *The Adventure of the Crooked Man*.
From the animal's foot-marks, he inferred:

- It was not a dog, cat, monkey, or other familiar creature: its print had
  five foot-pads, long nails, and was nearly as large as a dessert-spoon
  (sherlock-holmes.txt:5479-5486).
- It had a long body and very short legs: nearly two feet long, but with a
  stride of only about three inches (sherlock-holmes.txt:5497-5502).
- It was carnivorous because it climbed the curtain toward a canary's cage
  (sherlock-holmes.txt:5509-5510).
- It was probably from the weasel or stoat tribe, though unusually large
  (sherlock-holmes.txt:5514-5516).

Holmes also inferred that a man entered through the window with it and later
left with the key (sherlock-holmes.txt:5464-5470, 5527-5528).
```

#### 用户：直接检索

不通过 Agent，直接搜索同一个书架：

```bash
zg query --human "An unseen creature left a few marks. What did the detective infer?" --limit 3
```

zg 会将 `sherlock-holmes.txt` 中的相关段落排在
`alice-in-wonderland.txt` 前面。

<a id="benchmarks"></a>

## 📊 性能测试

zg 是通用检索工具，目前的性能测试覆盖代码与文本检索。

### 代码仓库检索

我们使用 [SWE-QA-Bench](https://github.com/peng-weihan/SWE-QA-Bench)，它要求
Agent 在真实代码库中完成跨文件、多跳推理。

- **覆盖范围：**20 个检索密集型任务，覆盖论文中的 **What、Where、How、Why**
  四个一级类别、8 种意图和 11 个代码仓库。
- **Agent：**使用 **Claude Opus 5** 的 Claude Code。
- **Embedding：**zg 方案使用 **Qwen3.7 Text Embedding**。
- **测试方式：**每个任务、每种方案运行 3 次。

| 方案 | 评审得分 /100 ↑ | 输入 Token ↓ | 工具调用 ↓ | 耗时（秒）↓ | 成本 ↓ |
| --- | ---: | ---: | ---: | ---: | ---: |
| 原生工具 | 80.42 | 558,651 | 23.42 | 127.5 | $0.905 |
| 原生工具 + zg | 81.92 | 294,262 | 9.70 | 79.7 | $0.558 |
| 变化 | **+1.50 个百分点** | **−47.3%** | **−58.6%** | **−37.5%** | **−38.3%** |

在 20 个任务 × 3 次运行中，zg 将评审得分提升了 1.50 分，同时将输入 Token
减少 47.3%、工具调用减少 58.6%、执行时间减少 37.5%、成本减少 38.3%。

### 大规模文本检索

我们使用 [BrowseComp-Plus](https://github.com/texttron/BrowseComp-Plus)，它要求
Agent 从固定网页语料库中检索多文档证据。测试包含 30 个问题，覆盖 10 个映射主题
和三个证据广度级别，每种方案运行 3 次。

| 方案 | 准确率 ↑ | 证据召回率 ↑ | 搜索调用 ↓ | 输入 Token ↓ | 耗时（秒）↓ | 成本 ↓ |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Agent + BM25 | TBD | TBD | TBD | TBD | TBD | TBD |
| Agent + zg | TBD | TBD | TBD | TBD | TBD | TBD |
| 变化 | TBD | TBD | TBD | TBD | TBD | TBD |

在 30 个问题 × 3 次运行中，zg 对准确率、证据召回率、搜索调用、输入 Token、
执行时间和成本的影响尚待测试结果补充。

如需复现测试，请按照[性能测试指南](./benchmarks/README.md)准备环境、固定版本、
任务清单、基线配置、运行命令和结果汇总。所有方案使用相同的 Agent、模型、Prompt、
工具预算和三次运行协议。

## 📚 文档

| 指南 | 你可以完成什么 |
| :--- | :--- |
| [Agent 集成](./docs/01-agents.md) | 将 zg 接入 Codex、Claude Code、Cursor 或 OpenCode，并验证是否正常工作。 |
| [CLI 指南](./docs/02-cli.md) | 在终端中搜索、索引和管理本地工作区。 |
| [MCP 指南](./docs/03-mcp.md) | 了解 Agent 可以使用哪些 zg 工具，以及访问权限如何受到保护。 |
| [检索 Pipeline](./docs/04-pipeline.md) | 选择索引范围、保持内容新鲜，并获得更好的检索结果。 |
| [架构](./docs/05-architecture.md) | 了解 zg 如何处理查询，以及数据会保留在哪里。 |
| [服务端与执行模式](./docs/06-server.md) | 在一次性命令和长期运行的本地服务之间选择。 |
| [Embedding 模型](./docs/07-embedding.md) | 根据速度、检索质量、隐私和本机硬件选择合适的模型。 |
| [Roadmap](./docs/08-roadmap.md) | 了解接下来的产品方向，并参与影响 zg 的优先级。 |

<a id="community"></a>

## 🤝 加入社区

<div align="center">

| 💬 钉钉群 | 📱 微信群 | 🎮 Discord | X (Twitter) |
| :---: | :---: | :---: | :---: |
| <img src="https://zvec.oss-cn-hongkong.aliyuncs.com/qrcode/dingding.png" width="150" alt="钉钉二维码"/> | <img src="https://zvec.oss-cn-hongkong.aliyuncs.com/qrcode/wechat.png?v1" width="150" alt="微信二维码"/> | [![Discord](https://img.shields.io/badge/Discord-Join%20Server-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/rKddFBBu9z) | [![X (formerly Twitter) Follow](https://img.shields.io/twitter/follow/ZvecAI)](<https://x.com/ZvecAI>) |
| 扫码加入 | 扫码加入 | 点击加入 | 点击关注 |

</div>

## ❤️ 参与贡献

始终欢迎社区贡献——缺陷修复、新功能和文档改进都会让 zvec-grep 变得更好。

请查阅我们的[贡献指南](./CONTRIBUTING.md)开始参与！
