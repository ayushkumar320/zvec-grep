<p align="right">
  English | <a href="./README_CN.md">中文</a>
</p>

<div align="center">
  <p>
    <a href="./docs/08-roadmap.md"><img src="https://img.shields.io/badge/status-work%20in%20progress-F59E0B?style=for-the-badge" alt="Work in progress" /></a>
  </p>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./.github/assets/zg-logo-dark.svg">
    <img src="./.github/assets/zg-logo.svg" width="150" alt="zg logo" />
  </picture>
  <p><strong>Know the words—or don’t. Just zg.</strong></p>
  <p>The local-first search layer for humans and agents.</p>

  <p>
    <a href="https://www.npmjs.com/package/@zvec/zvec-grep"><img src="https://img.shields.io/npm/v/@zvec/zvec-grep.svg" alt="npm version" /></a>
    <a href="https://github.com/zvec-ai/zvec-grep/actions/workflows/ci.yml"><img src="https://github.com/zvec-ai/zvec-grep/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
    <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="Apache 2.0 license" /></a>
    <img src="https://img.shields.io/badge/node-%3E%3D22-blue.svg" alt="Node.js 22 or newer" />
  </p>

  <p>
    <a href="#tour">🎬 <strong>Tour</strong></a> |
    <a href="#features">💫 <strong>Features</strong></a> |
    <a href="#quickstart">🚀 <strong>Quickstart</strong></a> |
    <a href="./docs/README.md">📚 <strong>Docs</strong></a> |
    <a href="#benchmarks">📊 <strong>Benchmarks</strong></a> |
    <a href="#community">🤝 <strong>Community</strong></a>
  </p>
</div>

**zg** (**z**vec-**g**rep) unifies ripgrep, BM25, and vector search behind
[one local-first interface](./docs/05-architecture.md). Use it directly from the
terminal, or let your agent use it for you.

<a id="tour"></a>

## 🎬 See it in action

<div align="center">
  <img src="./.github/assets/zvec-grep-tour.gif" width="1000" alt="Install the agent integration, index a workspace, and let the agent search it with zvec-grep" />
</div>

Install the integration once, index the workspace, then search from the terminal
or ask your agent naturally.

<a id="features"></a>

## 💫 Why zg?

- **Works out of the box** — one guided install connects zg to your agent on
  macOS, Linux, or Windows; ask naturally, with no search syntax to learn.
- **All-in-one retrieval layer** — semantic discovery, BM25-ranked retrieval,
  exact text, and regex search all stay behind zg.
- **Agent- and human-friendly** — compact, file-oriented context for agents
  and readable terminal output for people, with less noise for both.
- **Local first, permission aware** — ripgrep, indexes, and local models stay
  on your machine; remote embeddings require explicit authorization.

<a id="quickstart"></a>

## 🚀 Quickstart

Requires Node.js 22 or newer on macOS, Linux, or Windows.

### 1. Install and connect your agent

```bash
npm install -g @zvec/zvec-grep
zg install
```

`zg install` detects Codex, Claude Code, Cursor, and OpenCode and configures the
local MCP integration. See [Agent integrations](./docs/01-agents.md) for managed
configuration, permissions, and uninstall instructions. You can also select one
explicitly:

```bash
zg install --target codex --yes
```

### 2. Index your workspace

```bash
cd your-workspace
zg index --embedding local/potion-code-16m-v2
```

This quickstart uses the lightweight
[Potion Code v2](./docs/07-embedding.md) model so you can build the first index
quickly. The first run downloads it; the index stays in `.zvec-grep/`, and later
updates only need `zg index`. See the
[retrieval pipeline](./docs/04-pipeline.md#indexing) to control scope, updates, and
rebuilds.

### 3. Ask your agent

```text
My app forgets dark mode every time I refresh. Find out why.
```

The indexed workspace may contain code, documentation, books, research material,
meeting notes, manuals, knowledge-base exports, or other local content. Workspace
search applies only when local material is intended as evidence; unrelated
open-world, current external, and web-only questions use the appropriate external
source.

When the answer should be grounded in the current indexed workspace, the agent
uses zg for semantic discovery and ranked keyword retrieval, while exact lexical
lookup stays with native grep or rg. The
[MCP guide](./docs/03-mcp.md) describes the default search tool and the optional
full toolset, which retains managed rg.

To search directly from the terminal, use the same local layer. See the
[CLI guide](./docs/02-cli.md) for routes, filters, and output controls.

```bash
zg query --human "theme preference persistence on startup" --limit 3
```

<a id="benchmarks"></a>

## 📊 Benchmarks

zg is a general-purpose retrieval tool. Current benchmarks cover code and text
retrieval.

### Repository code retrieval

We use [SWE-QA-Bench](https://github.com/peng-weihan/SWE-QA-Bench), which
requires cross-file and multi-hop reasoning over real-world codebases.

- **Coverage:** 20 retrieval-intensive tasks covering the paper's four
  top-level categories—**What, Where, How, and Why**—across 8 intentions and
  11 repositories.
- **Agent:** Claude Code with **Claude Opus 5**.
- **Embedding:** **Qwen3.7 Text Embedding** for the zg profile.
- **Protocol:** 3 runs per task and profile.

| Profile | Judge /100 ↑ | Input tokens ↓ | Tool calls ↓ | Time (s) ↓ | Cost ↓ |
| --- | ---: | ---: | ---: | ---: | ---: |
| Native tools | 80.42 | 558,651 | 23.42 | 127.5 | $0.905 |
| Native tools + zg | 81.92 | 294,262 | 9.70 | 79.7 | $0.558 |
| Change | **+1.50 pp** | **−47.3%** | **−58.6%** | **−37.5%** | **−38.3%** |

Across 20 tasks × 3 runs, zg increased the Judge score by 1.50 points while
reducing input tokens by 47.3%, tool calls by 58.6%, execution time by 37.5%,
and cost by 38.3%.

### Large-scale text retrieval

We use [BrowseComp-Plus](https://github.com/texttron/BrowseComp-Plus), which
requires multi-document evidence retrieval over a fixed web corpus. The
evaluation includes 30 queries covering 10 mapped topics and three
evidence-breadth levels, with 3 runs per profile.

| Profile | Accuracy ↑ | Evidence recall ↑ | Search calls ↓ | Input tokens ↓ | Time (s) ↓ | Cost ↓ |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Agent + BM25 | TBD | TBD | TBD | TBD | TBD | TBD |
| Agent + zg | TBD | TBD | TBD | TBD | TBD | TBD |
| Change | TBD | TBD | TBD | TBD | TBD | TBD |

Across 30 queries × 3 runs, zg changed accuracy by TBD pp, evidence recall by
TBD pp, search calls by TBD%, input tokens by TBD%, time by TBD%, and cost by
TBD%.

To reproduce the evaluation, follow the
[benchmark guide](./benchmarks/README.md) for environment setup, pinned
revisions, task manifests, baseline configuration, exact commands, and result
aggregation. All profiles use the same agent, model, prompt, tool budget, and
three-run protocol.

## 🗺️ Roadmap

zg is moving toward richer multimodal formats, stronger hybrid retrieval, a
more out-of-the-box experience, and support from desktop to mobile.

Explore the full [Roadmap](./docs/08-roadmap.md).

<a id="community"></a>

## 🤝 Join Our Community

<div align="center">

| 💬 DingTalk | 📱 WeChat | 🎮 Discord | X (Twitter) |
| :---: | :---: | :---: | :---: |
| <img src="https://zvec.oss-cn-hongkong.aliyuncs.com/qrcode/dingding.png" width="150" alt="DingTalk QR Code"/> | <img src="https://zvec.oss-cn-hongkong.aliyuncs.com/qrcode/wechat.png?v1" width="150" alt="WeChat QR Code"/> | [![Discord](https://img.shields.io/badge/Discord-Join%20Server-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/rKddFBBu9z) | [![X (formerly Twitter) Follow](https://img.shields.io/twitter/follow/ZvecAI)](<https://x.com/ZvecAI>) |
| Scan to join | Scan to join | Click to join | Click to follow |

</div>

## ❤️ Contributing

Community contributions are always welcome—bug fixes, features, and
documentation improvements all help make zvec-grep better.

Check out our [Contributing Guide](./CONTRIBUTING.md) to get started!
