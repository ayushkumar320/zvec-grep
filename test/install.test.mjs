import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import {
  chmod,
  lstat,
  mkdir,
  mkdtemp,
  readdir,
  readFile,
  rm,
  stat,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { createServer } from "node:net";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import test from "node:test";
import { installerSelectionLines } from "../dist/cli/install.js";
import { ZVEC_GREP_WORKSPACE_EVIDENCE_RULES } from "../dist/prompts/zvec-grep-guidance.js";

const execFileAsync = promisify(execFile);
const cliPath = resolve("dist/cli/index.js");
const qwenSearchPermission = "mcp__zvec_grep__zvec_grep_search";
const qwenRgPermission = "mcp__zvec_grep__zvec_grep_rg";

test("interactive installer marker follows the active agent", () => {
  const detected = new Set(["claude", "codex"]);
  const claude = installerSelectionLines(0, detected);
  const codex = installerSelectionLines(1, detected);
  const qwen = installerSelectionLines(4, detected);

  assert.match(claude[0], /● Claude Code\s+detected/);
  assert.match(claude[1], /○ Codex\s+detected/);
  assert.match(codex[0], /○ Claude Code\s+detected/);
  assert.match(codex[1], /● Codex\s+detected/);
  assert.match(qwen[0], /○ Claude Code\s+detected/);
  assert.match(qwen[1], /○ Codex\s+detected/);
  assert.match(qwen[2], /○ OpenCode\s+not found/);
  assert.match(qwen[3], /○ Cursor\s+not found/);
  assert.match(qwen[4], /● Qwen Code\s+not found/);
  assert.match(codex.at(-1), /Use ↑↓ to move · Enter to select/);
  assert.doesNotMatch(codex.join("\n"), /Space|\[●\]/);
});

test("install starts the shared server with the selected MCP toolset", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-server-"),
  );
  const home = join(temporaryDirectory, "home");
  const port = await availablePort();
  const serverUrl = `http://127.0.0.1:${port}/mcp`;
  await mkdir(join(temporaryDirectory, ".zvec-grep"), { recursive: true });
  await writeFile(
    join(temporaryDirectory, ".zvec-grep", "config.json"),
    `${JSON.stringify({
      version: 1,
      client: { serverUrl },
      server: { host: "127.0.0.1", port },
    })}\n`,
  );
  const environment = {
    ...process.env,
    HOME: temporaryDirectory,
    USERPROFILE: temporaryDirectory,
    CODEX_HOME: join(temporaryDirectory, ".codex"),
    ZVEC_GREP_HOME: home,
  };
  t.after(async () => {
    await execFileAsync(
      process.execPath,
      [cliPath, "server", "off", "--home", home],
      { env: environment },
    ).catch(() => undefined);
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  const { stdout } = await execFileAsync(
    process.execPath,
    [cliPath, "install", "--target", "codex", "--mcp-toolset", "full", "--yes"],
    { env: environment },
  );
  assert.match(stdout, new RegExp(`ready at ${serverUrl}`));
  const { stdout: statusOutput } = await execFileAsync(
    process.execPath,
    [cliPath, "server", "status", "--check-ready", "--home", home],
    { env: environment },
  );
  assert.match(statusOutput, /MCP toolset: full/);
  const config = await readFile(
    join(temporaryDirectory, ".codex", "config.toml"),
    "utf8",
  );
  assert.match(
    config,
    /^args = \["server", "--stdio", "--mcp-toolset", "full"\]$/m,
  );
});

test("Codex installer removes orphaned managed markers", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-"),
  );
  const codexHome = join(temporaryDirectory, ".codex");
  const configPath = join(codexHome, "config.toml");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(codexHome, { recursive: true });
  await writeFile(
    configPath,
    ["[mcp_servers.other]", 'command = "other"', "# ZVEC_GREP_END", ""].join(
      "\n",
    ),
  );

  await execFileAsync(
    process.execPath,
    [cliPath, "install", "--target", "codex", "--yes"],
    {
      env: {
        ...process.env,
        CODEX_HOME: codexHome,
        ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
      },
    },
  );

  const installed = await readFile(configPath, "utf8");
  assert.match(installed, /\[mcp_servers\.other\]/);
  assert.match(installed, /^command = "zg"$/m);
  assert.match(installed, /^args = \["server", "--stdio"\]$/m);
  assert.doesNotMatch(installed, /^bearer_token_env_var\s*=/m);
  assert.doesNotMatch(installed, /^url\s*=/m);
  assert.match(installed, /^tool_timeout_sec = 600$/m);
  assert.match(installed, /^default_tools_approval_mode = "approve"$/m);
  assert.doesNotMatch(installed, /^default_tools_approval_mode = "auto"$/m);
  assert.equal(countOccurrences(installed, "# ZVEC_GREP_START"), 1);
  assert.equal(countOccurrences(installed, "# ZVEC_GREP_END"), 1);
  assert.equal(countOccurrences(installed, "[mcp_servers.zvec_grep]"), 1);
});

test("Codex installer writes an explicit MCP token environment variable", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-token-"),
  );
  const codexHome = join(temporaryDirectory, ".codex");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await execFileAsync(
    process.execPath,
    [
      cliPath,
      "install",
      "--target",
      "codex",
      "--mcp-transport",
      "http",
      "--mcp-token-env",
      "ZVEC_GREP_SERVER_TOKEN",
      "--yes",
    ],
    {
      env: {
        ...process.env,
        CODEX_HOME: codexHome,
        ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
      },
    },
  );

  const installed = await readFile(join(codexHome, "config.toml"), "utf8");
  assert.match(installed, /^bearer_token_env_var = "ZVEC_GREP_SERVER_TOKEN"$/m);
});

test("Codex uninstaller removes only zvec-grep-managed integration blocks", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-uninstall-"),
  );
  const codexHome = join(temporaryDirectory, ".codex");
  const configPath = join(codexHome, "config.toml");
  const agentsPath = join(codexHome, "AGENTS.md");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(codexHome, { recursive: true });
  await writeFile(configPath, '[mcp_servers.other]\ncommand = "other"\n');
  await writeFile(agentsPath, "# Existing instructions\n");
  await installCodex(codexHome);
  await uninstallCodex(codexHome);
  await uninstallCodex(codexHome);

  const config = await readFile(configPath, "utf8");
  const agents = await readFile(agentsPath, "utf8");
  assert.match(config, /\[mcp_servers\.other\]\ncommand = "other"/);
  assert.doesNotMatch(config, /ZVEC_GREP|mcp_servers\.zvec_grep/);
  assert.match(agents, /# Existing instructions/);
  assert.doesNotMatch(agents, /ZVEC_GREP|## zvec-grep/);
});

test("Codex installer detects and replaces equivalent unmanaged MCP table headers", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-conflict-"),
  );
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  const cases = [
    ["leading-whitespace", "  [mcp_servers.zvec_grep]"],
    ["quoted-key", '[mcp_servers."zvec_grep"]'],
  ];

  for (const [name, tableHeader] of cases) {
    const codexHome = join(temporaryDirectory, name);
    const configPath = join(codexHome, "config.toml");
    const existing = [
      "[mcp_servers.other]",
      'command = "other"',
      "",
      tableHeader,
      'command = "old-zg"',
      "",
    ].join("\n");

    await mkdir(codexHome, { recursive: true });
    await writeFile(configPath, existing);

    await assert.rejects(installCodex(codexHome));
    assert.equal(await readFile(configPath, "utf8"), existing);

    await installCodex(codexHome, ["--force"]);

    const installed = await readFile(configPath, "utf8");
    assert.match(installed, /\[mcp_servers\.other\]\ncommand = "other"/);
    assert.doesNotMatch(installed, /old-zg/);
    assert.equal(countOccurrences(installed, "[mcp_servers.zvec_grep]"), 1);
  }
});

test("Codex installer ignores an orphaned end marker before a complete block", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-existing-"),
  );
  const codexHome = join(temporaryDirectory, ".codex");
  const configPath = join(codexHome, "config.toml");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(codexHome, { recursive: true });
  await writeFile(
    configPath,
    [
      "[mcp_servers.other]",
      'command = "other"',
      "# ZVEC_GREP_END",
      "",
      "# ZVEC_GREP_START",
      "[mcp_servers.zvec_grep]",
      'command = "old-zg"',
      "# ZVEC_GREP_END",
      "",
    ].join("\n"),
  );

  await execFileAsync(
    process.execPath,
    [cliPath, "install", "--target", "codex", "--yes"],
    {
      env: {
        ...process.env,
        CODEX_HOME: codexHome,
        ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
      },
    },
  );

  const installed = await readFile(configPath, "utf8");
  assert.match(installed, /\[mcp_servers\.other\]/);
  assert.doesNotMatch(installed, /old-zg/);
  assert.equal(countOccurrences(installed, "# ZVEC_GREP_START"), 1);
  assert.equal(countOccurrences(installed, "# ZVEC_GREP_END"), 1);
  assert.equal(countOccurrences(installed, "[mcp_servers.zvec_grep]"), 1);
});

test("Codex installer preserves user config after an orphaned start marker", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-orphan-start-"),
  );
  const codexHome = join(temporaryDirectory, ".codex");
  const configPath = join(codexHome, "config.toml");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(codexHome, { recursive: true });
  await writeFile(
    configPath,
    [
      "# ZVEC_GREP_START",
      "[mcp_servers.other]",
      'command = "other"',
      "",
      "# ZVEC_GREP_START",
      "[mcp_servers.zvec_grep]",
      'command = "old-zg"',
      "# ZVEC_GREP_END",
      "",
    ].join("\n"),
  );

  await installCodex(codexHome);

  const installed = await readFile(configPath, "utf8");
  assert.match(installed, /\[mcp_servers\.other\]\ncommand = "other"/);
  assert.doesNotMatch(installed, /old-zg/);
  assert.equal(countOccurrences(installed, "# ZVEC_GREP_START"), 1);
  assert.equal(countOccurrences(installed, "# ZVEC_GREP_END"), 1);
  assert.equal(countOccurrences(installed, "[mcp_servers.zvec_grep]"), 1);
});

test("Codex installer collapses duplicate managed blocks without deleting config between them", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-duplicate-"),
  );
  const codexHome = join(temporaryDirectory, ".codex");
  const configPath = join(codexHome, "config.toml");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(codexHome, { recursive: true });
  await writeFile(
    configPath,
    [
      "# ZVEC_GREP_START",
      "[mcp_servers.zvec_grep]",
      'command = "old-zg-one"',
      "# ZVEC_GREP_END",
      "",
      "[mcp_servers.other]",
      'command = "other"',
      "",
      "# ZVEC_GREP_START",
      "[mcp_servers.zvec_grep]",
      'command = "old-zg-two"',
      "# ZVEC_GREP_END",
      "",
    ].join("\n"),
  );

  await installCodex(codexHome);

  const installed = await readFile(configPath, "utf8");
  assert.match(installed, /\[mcp_servers\.other\]\ncommand = "other"/);
  assert.doesNotMatch(installed, /old-zg-(?:one|two)/);
  assert.equal(countOccurrences(installed, "# ZVEC_GREP_START"), 1);
  assert.equal(countOccurrences(installed, "# ZVEC_GREP_END"), 1);
  assert.equal(countOccurrences(installed, "[mcp_servers.zvec_grep]"), 1);
});

test(
  "Codex installer atomically updates symlink targets and preserves their modes",
  {
    skip:
      process.platform === "win32"
        ? "Windows symlink and Unix mode semantics differ"
        : false,
  },
  async (t) => {
    const temporaryDirectory = await mkdtemp(
      join(tmpdir(), "zvec-grep-install-symlink-"),
    );
    const codexHome = join(temporaryDirectory, ".codex");
    const dotfiles = join(temporaryDirectory, "dotfiles");
    const configTarget = join(dotfiles, "config.toml");
    const configPath = join(codexHome, "config.toml");
    t.after(async () => {
      await rm(temporaryDirectory, { recursive: true, force: true });
    });

    await mkdir(codexHome, { recursive: true });
    await mkdir(dotfiles, { recursive: true });
    await writeFile(configTarget, '[mcp_servers.other]\ncommand = "other"\n');
    await chmod(configTarget, 0o640);
    await symlink(configTarget, configPath);

    await installCodex(codexHome);

    assert.equal((await lstat(configPath)).isSymbolicLink(), true);
    assert.equal((await stat(configTarget)).mode & 0o777, 0o640);
    assert.match(
      await readFile(configTarget, "utf8"),
      /\[mcp_servers\.zvec_grep\]/,
    );
  },
);

test("Codex installer removes temporary files when an atomic replacement fails", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-failure-"),
  );
  const codexHome = join(temporaryDirectory, ".codex");
  const configPath = join(codexHome, "config.toml");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(configPath, { recursive: true });

  await assert.rejects(installCodex(codexHome));

  const entries = await readdir(codexHome);
  assert.equal(
    entries.some((entry) => entry.endsWith(".tmp")),
    false,
  );
});

test("Codex installer accepts a custom MCP tool timeout", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-timeout-"),
  );
  const codexHome = join(temporaryDirectory, ".codex");
  const configPath = join(codexHome, "config.toml");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await installCodex(codexHome, ["--mcp-tool-timeout=900"]);

  const installed = await readFile(configPath, "utf8");
  assert.match(installed, /^tool_timeout_sec = 900$/m);
});

test("Codex installer refreshes legacy managed guidance", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-legacy-guidance-"),
  );
  const codexHome = join(temporaryDirectory, ".codex");
  const agentsPath = join(codexHome, "AGENTS.md");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(codexHome, { recursive: true });
  await writeFile(
    agentsPath,
    [
      "# Existing instructions",
      "",
      "<!-- ZVEC_GREP_START -->",
      "## zvec-grep",
      "legacy guidance",
      "<!-- ZVEC_GREP_END -->",
      "",
    ].join("\n"),
  );

  await installCodex(codexHome);

  const agents = await readFile(agentsPath, "utf8");
  assert.match(agents, /# Existing instructions/);
  assert.match(
    agents,
    /when an exact word, phrase, name, date,[^\n]+use `zvec_grep_rg` when it is listed by the current host; otherwise native Grep or `rg`/i,
  );
  assert.match(
    agents,
    /Use `zvec_grep_search` when wording or location is unknown/,
  );
  assert.match(agents, /comparison or synthesis across files, sections/);
  assert.match(agents, /Choose the evidence source before the retrieval mode/);
  for (const rule of ZVEC_GREP_WORKSPACE_EVIDENCE_RULES) {
    assert.ok(agents.includes(`- ${rule}`));
  }
  assert.match(
    agents,
    /If the index is missing but exact or regex lookup can answer the task, use `zvec_grep_rg` when it is listed by the current host; otherwise native Grep or `rg`/,
  );
  assert.match(
    agents,
    /A workspace may contain source code, documentation, books/,
  );
  assert.match(agents, /one focused `zvec_grep_search` probe/);
  assert.match(agents, /When no sufficient exact anchor is available/);
  assert.match(agents, /probe does not apply to exact quotations/i);
  assert.match(
    agents,
    /unrelated open-world questions, current external facts/,
  );
  assert.match(agents, /Do not delegate solely to locate material/);
  assert.match(agents, /`query` creates one primary hybrid result group/);
  assert.match(agents, /`fts` is a retrieval route, not a hard filter/);
  assert.match(agents, /"root": "\/absolute\/workspace"/);
  assert.match(agents, /"fuse": true/);
  assert.match(agents, /Treat a sufficient snippet as already-read evidence/);
  assert.match(agents, /Creating, rebuilding, or dropping a persistent index/);
  assert.doesNotMatch(agents, /managed-rg/);
  assert.doesNotMatch(agents, /solely to locate code/);
  assert.doesNotMatch(agents, /indexed search first/);
  assert.doesNotMatch(agents, /Indexing and status/);
  assert.doesNotMatch(agents, /Remote data authorization/);
  assert.doesNotMatch(agents, /zg status/);
  assert.doesNotMatch(agents, /legacy guidance/);
});

test("Claude Code installer configures MCP trust and guidance", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-claude-"),
  );
  const claudeConfigDirectory = join(temporaryDirectory, ".claude");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(claudeConfigDirectory, { recursive: true });
  await writeFile(
    join(claudeConfigDirectory, ".claude.json"),
    `${JSON.stringify({ mcpServers: { zvec_grep: { type: "http", url: "http://127.0.0.1:7999/mcp", alwaysLoad: true } } }, null, 2)}\n`,
  );

  const { stdout } = await execFileAsync(
    process.execPath,
    [cliPath, "install", "--target", "claude", "--yes"],
    {
      env: {
        ...process.env,
        HOME: temporaryDirectory,
        CLAUDE_CONFIG_DIR: claudeConfigDirectory,
        ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
      },
    },
  );

  const mcpConfig = JSON.parse(
    await readFile(join(claudeConfigDirectory, ".claude.json"), "utf8"),
  );
  const settings = JSON.parse(
    await readFile(join(claudeConfigDirectory, "settings.json"), "utf8"),
  );
  const guidance = await readFile(
    join(claudeConfigDirectory, "CLAUDE.md"),
    "utf8",
  );

  assert.deepEqual(mcpConfig.mcpServers.zvec_grep, {
    alwaysLoad: true,
    type: "stdio",
    command: "zg",
    args: ["server", "--stdio"],
  });
  assert.ok(settings.permissions.allow.includes("mcp__zvec_grep__*"));
  assert.match(guidance, /zvec_grep_search/);
  assert.match(guidance, /`zvec_grep_rg` when it is listed/);
  assert.match(
    guidance,
    /when an exact word, phrase, name, date,[^\n]+use `zvec_grep_rg` when it is listed by the current host; otherwise native Grep or `rg`/i,
  );
  assert.doesNotMatch(guidance, /managed-rg/);
  assert.doesNotMatch(guidance, /Indexing and status/);
  assert.doesNotMatch(guidance, /Remote data authorization/);
  assert.doesNotMatch(guidance, /zg status/);
  assert.match(stdout, /zvec-grep setup/);
  assert.match(stdout, /Installing integrations/);
  assert.match(stdout, /Claude Code/);
  assert.doesNotMatch(stdout, /Guidance/);
  assert.doesNotMatch(stdout, /Trust|MCP trust/);
  assert.match(stdout, /Remote data\s+Authorization requested/);
});

test("Claude Code installer preserves user configuration on install and uninstall", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-claude-preserve-"),
  );
  const claudeConfigDirectory = join(temporaryDirectory, ".claude");
  const mcpConfigPath = join(claudeConfigDirectory, ".claude.json");
  const settingsPath = join(claudeConfigDirectory, "settings.json");
  const guidancePath = join(claudeConfigDirectory, "CLAUDE.md");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(claudeConfigDirectory, { recursive: true });
  await writeFile(
    mcpConfigPath,
    `${JSON.stringify({ mcpServers: { other: { type: "http", url: "https://example.test/mcp" } }, theme: "dark" }, null, 2)}\n`,
  );
  await writeFile(
    settingsPath,
    `${JSON.stringify({ permissions: { allow: ["Bash(git status)"], deny: ["Bash(rm *)"] } }, null, 2)}\n`,
  );
  await writeFile(guidancePath, "# Existing Claude guidance\n");

  const environment = {
    ...process.env,
    HOME: temporaryDirectory,
    CLAUDE_CONFIG_DIR: claudeConfigDirectory,
    ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
  };
  await execFileAsync(
    process.execPath,
    [cliPath, "install", "--target", "claude", "--yes"],
    { env: environment },
  );
  await execFileAsync(
    process.execPath,
    [cliPath, "uninstall", "--target", "claude", "--yes"],
    { env: environment },
  );

  const mcpConfig = JSON.parse(await readFile(mcpConfigPath, "utf8"));
  const settings = JSON.parse(await readFile(settingsPath, "utf8"));
  const guidance = await readFile(guidancePath, "utf8");
  assert.deepEqual(mcpConfig, {
    mcpServers: {
      other: { type: "http", url: "https://example.test/mcp" },
    },
    theme: "dark",
  });
  assert.deepEqual(settings, {
    permissions: {
      allow: ["Bash(git status)"],
      deny: ["Bash(rm *)"],
    },
  });
  assert.match(guidance, /# Existing Claude guidance/);
  assert.doesNotMatch(guidance, /ZVEC_GREP|## zvec-grep/);
});

test("Claude Code installer writes MCP token environment expansion", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-claude-token-"),
  );
  const claudeConfigDirectory = join(temporaryDirectory, ".claude");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await execFileAsync(
    process.execPath,
    [
      cliPath,
      "install",
      "--target",
      "claude",
      "--mcp-transport",
      "http",
      "--mcp-token-env",
      "ZVEC_GREP_SERVER_TOKEN",
      "--yes",
    ],
    {
      env: {
        ...process.env,
        HOME: temporaryDirectory,
        CLAUDE_CONFIG_DIR: claudeConfigDirectory,
        ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
      },
    },
  );

  const mcpConfig = JSON.parse(
    await readFile(join(claudeConfigDirectory, ".claude.json"), "utf8"),
  );
  assert.equal(
    mcpConfig.mcpServers.zvec_grep.headers.Authorization,
    "Bearer ${ZVEC_GREP_SERVER_TOKEN}",
  );
});

test("Claude Code installer accepts cc and claude-code compatibility aliases", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-claude-aliases-"),
  );
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  for (const alias of ["cc", "claude-code"]) {
    const configDirectory = join(temporaryDirectory, alias);
    await installTarget(alias, { CLAUDE_CONFIG_DIR: configDirectory });
    const config = JSON.parse(
      await readFile(join(configDirectory, ".claude.json"), "utf8"),
    );
    assert.equal(config.mcpServers.zvec_grep.command, "zg");
    assert.deepEqual(config.mcpServers.zvec_grep.args, ["server", "--stdio"]);
  }
});

test("Qwen Code installer accepts qwen aliases and numeric target 5", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-qwen-aliases-"),
  );
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  for (const target of ["qwen", "qwen-code", "qwencode", "5"]) {
    const qwenHome = join(temporaryDirectory, target);
    await installTarget(target, { QWEN_HOME: qwenHome });
    const config = JSON.parse(
      await readFile(join(qwenHome, "settings.json"), "utf8"),
    );
    assert.equal(config.mcpServers.zvec_grep.command, "zg");
    assert.deepEqual(config.mcpServers.zvec_grep.args, ["server", "--stdio"]);
  }
});

test("Qwen Code installer configures full stdio tools, timeout, permissions, and guidance", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-qwen-stdio-"),
  );
  const qwenHome = join(temporaryDirectory, ".qwen");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await installTarget("qwen", { QWEN_HOME: qwenHome }, [
    "--mcp-toolset=full",
    "--mcp-tool-timeout=900",
  ]);

  const config = JSON.parse(
    await readFile(join(qwenHome, "settings.json"), "utf8"),
  );
  assert.deepEqual(config.mcpServers.zvec_grep, {
    command: "zg",
    args: ["server", "--stdio", "--mcp-toolset", "full"],
    timeout: 900000,
  });
  assert.deepEqual(config.permissions.allow, [
    qwenSearchPermission,
    qwenRgPermission,
  ]);
  assert.equal(config.mcpServers.zvec_grep.trust, undefined);
  assert.equal(config.permissions.allow.includes("mcp__zvec_grep__*"), false);

  const guidance = await readFile(join(qwenHome, "QWEN.md"), "utf8");
  assert.match(guidance, new RegExp("`" + qwenSearchPermission + "`"));
  assert.match(guidance, new RegExp("`" + qwenRgPermission + "`"));
  assert.equal(countOccurrences(guidance, "<!-- ZVEC_GREP_START -->"), 1);
  assert.equal(countOccurrences(guidance, "<!-- ZVEC_GREP_END -->"), 1);
});

test("Qwen Code installer writes Streamable HTTP configuration and token expansion", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-qwen-http-"),
  );
  const qwenHome = join(temporaryDirectory, ".qwen");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await installTarget("qwen", { QWEN_HOME: qwenHome }, [
    "--mcp-transport=http",
    "--mcp-tool-timeout=42",
    "--mcp-token-env=ZVEC_GREP_SERVER_TOKEN",
  ]);

  const config = JSON.parse(
    await readFile(join(qwenHome, "settings.json"), "utf8"),
  );
  assert.deepEqual(config.mcpServers.zvec_grep, {
    httpUrl: "http://127.0.0.1:7999/mcp",
    timeout: 42000,
    headers: {
      Authorization: "Bearer ${ZVEC_GREP_SERVER_TOKEN}",
    },
  });
  assert.equal(config.mcpServers.zvec_grep.url, undefined);
  assert.equal(config.mcpServers.zvec_grep.command, undefined);
  assert.deepEqual(config.permissions.allow, [qwenSearchPermission]);
});

test("Qwen Code installer preserves comments and rejects trailing commas", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-qwen-jsonc-"),
  );
  const qwenHome = join(temporaryDirectory, ".qwen");
  const configPath = join(qwenHome, "settings.json");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(qwenHome, { recursive: true });
  await writeFile(
    configPath,
    [
      "{",
      "  // Keep the user's theme comment.",
      '  "theme": "dark",',
      "  /* Keep the other MCP server comment. */",
      '  "mcpServers": {',
      '    "other": { "httpUrl": "https://example.test/mcp" }',
      "  }",
      "}",
      "",
    ].join("\n"),
  );

  await installTarget("qwen", { QWEN_HOME: qwenHome });
  const installed = await readFile(configPath, "utf8");
  assert.match(installed, /\/\/ Keep the user's theme comment\./);
  assert.match(installed, /\/\* Keep the other MCP server comment\. \*\//);
  assert.match(installed, /"theme"\s*:\s*"dark"/);
  assert.match(installed, /"other"\s*:\s*\{[^}]*example\.test\/mcp/s);
  assert.match(installed, /"command"\s*:\s*"zg"/);

  const invalidHome = join(temporaryDirectory, "invalid");
  const invalidConfigPath = join(invalidHome, "settings.json");
  const invalidSource = '{\n  "theme": "dark",\n}\n';
  await mkdir(invalidHome, { recursive: true });
  await writeFile(invalidConfigPath, invalidSource);

  await assert.rejects(
    installTarget("qwen", { QWEN_HOME: invalidHome }),
    /settings\.json|Qwen|JSON/i,
  );
  assert.equal(await readFile(invalidConfigPath, "utf8"), invalidSource);
  await assert.rejects(stat(join(invalidHome, "QWEN.md")), {
    code: "ENOENT",
  });
});

test("Qwen Code installer requires force for unmanaged servers and force replaces them cleanly", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-qwen-conflict-"),
  );
  const qwenHome = join(temporaryDirectory, ".qwen");
  const configPath = join(qwenHome, "settings.json");
  const original = `${JSON.stringify(
    {
      theme: "dark",
      mcpServers: {
        zvec_grep: {
          httpUrl: "https://example.test/foreign-mcp",
          trust: true,
          description: "user-owned server",
        },
      },
    },
    null,
    2,
  )}\n`;
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(qwenHome, { recursive: true });
  await writeFile(configPath, original);
  await assert.rejects(
    installTarget("qwen", { QWEN_HOME: qwenHome }),
    /--force/,
  );
  assert.equal(await readFile(configPath, "utf8"), original);
  await assert.rejects(stat(join(qwenHome, "QWEN.md")), { code: "ENOENT" });

  await installTarget("qwen", { QWEN_HOME: qwenHome }, ["--force"]);
  const config = JSON.parse(await readFile(configPath, "utf8"));
  assert.equal(config.theme, "dark");
  assert.deepEqual(config.mcpServers.zvec_grep, {
    command: "zg",
    args: ["server", "--stdio"],
    timeout: 600000,
  });
  assert.equal(config.mcpServers.zvec_grep.trust, undefined);
  assert.equal(config.mcpServers.zvec_grep.description, undefined);
});

test("Qwen Code installer replaces its managed server entry cleanly", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-qwen-policy-"),
  );
  const qwenHome = join(temporaryDirectory, ".qwen");
  const configPath = join(qwenHome, "settings.json");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(qwenHome, { recursive: true });
  await writeFile(
    configPath,
    `${JSON.stringify(
      {
        mcpServers: {
          zvec_grep: {
            command: "zg",
            args: ["server", "--stdio"],
            timeout: 1000,
            headers: { "X-Old": "remove me" },
            trust: true,
            description: "Keep this policy",
            includeTools: ["zvec_grep_search"],
            excludeTools: ["zvec_grep_drop"],
            discoveryTimeoutMs: 3210,
          },
        },
        permissions: { allow: ["Bash(git status)"] },
      },
      null,
      2,
    )}\n`,
  );

  await installTarget("qwen", { QWEN_HOME: qwenHome }, [
    "--mcp-transport=http",
    "--mcp-toolset=full",
    "--mcp-token-env=ZVEC_GREP_SERVER_TOKEN",
  ]);

  const config = JSON.parse(await readFile(configPath, "utf8"));
  assert.deepEqual(config.mcpServers.zvec_grep, {
    httpUrl: "http://127.0.0.1:7999/mcp",
    timeout: 600000,
    headers: {
      Authorization: "Bearer ${ZVEC_GREP_SERVER_TOKEN}",
    },
  });
  assert.deepEqual(config.permissions.allow, [
    "Bash(git status)",
    qwenSearchPermission,
    qwenRgPermission,
  ]);
});

test("Qwen Code install and uninstall are idempotent and preserve user content", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-qwen-idempotent-"),
  );
  const qwenHome = join(temporaryDirectory, ".qwen");
  const configPath = join(qwenHome, "settings.json");
  const guidancePath = join(qwenHome, "QWEN.md");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(qwenHome, { recursive: true });
  await writeFile(
    configPath,
    `${JSON.stringify(
      {
        theme: "dark",
        mcpServers: {
          other: { httpUrl: "https://example.test/mcp" },
        },
        permissions: {
          allow: ["Bash(git status)"],
          deny: ["Bash(rm *)"],
        },
      },
      null,
      2,
    )}\n`,
  );
  await writeFile(guidancePath, "# Existing Qwen guidance\n");

  await installTarget("qwen", { QWEN_HOME: qwenHome }, ["--mcp-toolset=full"]);
  const firstInstallConfig = await readFile(configPath, "utf8");
  const firstInstallGuidance = await readFile(guidancePath, "utf8");
  await installTarget("qwen", { QWEN_HOME: qwenHome }, ["--mcp-toolset=full"]);
  assert.equal(await readFile(configPath, "utf8"), firstInstallConfig);
  assert.equal(await readFile(guidancePath, "utf8"), firstInstallGuidance);

  await uninstallTarget("qwen", { QWEN_HOME: qwenHome });
  const firstUninstallConfig = await readFile(configPath, "utf8");
  const firstUninstallGuidance = await readFile(guidancePath, "utf8");
  await uninstallTarget("qwen", { QWEN_HOME: qwenHome });
  assert.equal(await readFile(configPath, "utf8"), firstUninstallConfig);
  assert.equal(await readFile(guidancePath, "utf8"), firstUninstallGuidance);

  const config = JSON.parse(firstUninstallConfig);
  assert.equal(config.theme, "dark");
  assert.equal(config.mcpServers.zvec_grep, undefined);
  assert.equal(config.mcpServers.other.httpUrl, "https://example.test/mcp");
  assert.deepEqual(config.permissions, {
    allow: ["Bash(git status)"],
    deny: ["Bash(rm *)"],
  });
  assert.match(firstUninstallGuidance, /# Existing Qwen guidance/);
  assert.doesNotMatch(firstUninstallGuidance, /ZVEC_GREP|## zvec-grep/);
});

test("Qwen Code uninstaller cleans permissions only when managed installation evidence exists", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-uninstall-qwen-evidence-"),
  );
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  const untouchedHome = join(temporaryDirectory, "untouched");
  const untouchedConfigPath = join(untouchedHome, "settings.json");
  const untouchedGuidancePath = join(untouchedHome, "QWEN.md");
  const untouchedConfig = `${JSON.stringify(
    {
      permissions: {
        allow: ["Bash(git status)", qwenSearchPermission, qwenRgPermission],
      },
    },
    null,
    2,
  )}\n`;
  await mkdir(untouchedHome, { recursive: true });
  await writeFile(untouchedConfigPath, untouchedConfig);
  await writeFile(untouchedGuidancePath, "# User-owned guidance\n");

  await uninstallTarget("qwen", { QWEN_HOME: untouchedHome });
  assert.equal(await readFile(untouchedConfigPath, "utf8"), untouchedConfig);
  assert.equal(
    await readFile(untouchedGuidancePath, "utf8"),
    "# User-owned guidance\n",
  );

  const markerHome = join(temporaryDirectory, "marker-evidence");
  const markerConfigPath = join(markerHome, "settings.json");
  const markerGuidancePath = join(markerHome, "QWEN.md");
  await mkdir(markerHome, { recursive: true });
  await writeFile(markerConfigPath, untouchedConfig);
  await writeFile(
    markerGuidancePath,
    [
      "# Keep me",
      "<!-- ZVEC_GREP_START -->",
      "managed guidance",
      "<!-- ZVEC_GREP_END -->",
      "",
    ].join("\n"),
  );

  await uninstallTarget("qwen", { QWEN_HOME: markerHome });
  const markerConfig = JSON.parse(await readFile(markerConfigPath, "utf8"));
  assert.deepEqual(markerConfig.permissions, {
    allow: ["Bash(git status)"],
  });
  const markerGuidance = await readFile(markerGuidancePath, "utf8");
  assert.match(markerGuidance, /# Keep me/);
  assert.doesNotMatch(markerGuidance, /ZVEC_GREP|managed guidance/);
});

test("Qwen Code installer warns when context.fileName excludes QWEN.md", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-qwen-context-"),
  );
  const qwenHome = join(temporaryDirectory, ".qwen");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await mkdir(qwenHome, { recursive: true });
  await writeFile(
    join(qwenHome, "settings.json"),
    '{"context":{"fileName":["CONTEXT.md"]}}\n',
  );
  const { stdout, stderr } = await installTarget("qwen", {
    QWEN_HOME: qwenHome,
  });
  const output = `${stdout}\n${stderr}`;
  assert.match(output, /QWEN\.md/i);
  assert.match(output, /context\.fileName|not load|exclud/i);
  await stat(join(qwenHome, "QWEN.md"));
});

test("Qwen Code installer resolves QWEN_HOME and dotenv fallbacks", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-qwen-home-"),
  );
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  const dotenvHome = join(temporaryDirectory, "dotenv-home");
  const redirectedFromQwenEnv = join(temporaryDirectory, "qwen-env-target");
  const redirectedFromHomeEnv = join(temporaryDirectory, "home-env-target");
  await mkdir(join(dotenvHome, ".qwen"), { recursive: true });
  await writeFile(
    join(dotenvHome, ".qwen", ".env"),
    `QWEN_HOME=${redirectedFromQwenEnv}\n`,
  );
  await writeFile(
    join(dotenvHome, ".env"),
    `QWEN_HOME=${redirectedFromHomeEnv}\n`,
  );
  await installTarget("qwen", {
    HOME: dotenvHome,
    USERPROFILE: dotenvHome,
    QWEN_HOME: undefined,
  });
  await stat(join(redirectedFromQwenEnv, "settings.json"));
  await assert.rejects(stat(join(redirectedFromHomeEnv, "settings.json")), {
    code: "ENOENT",
  });

  const homeEnvHome = join(temporaryDirectory, "home-env-home");
  const redirectedFromOnlyHomeEnv = join(
    temporaryDirectory,
    "only-home-env-target",
  );
  await mkdir(homeEnvHome, { recursive: true });
  await writeFile(
    join(homeEnvHome, ".env"),
    `QWEN_HOME=${redirectedFromOnlyHomeEnv}\n`,
  );
  await installTarget("qwen", {
    HOME: homeEnvHome,
    USERPROFILE: homeEnvHome,
    QWEN_HOME: undefined,
  });
  await stat(join(redirectedFromOnlyHomeEnv, "settings.json"));

  const emptyValueHome = join(temporaryDirectory, "empty-value-home");
  const ignoredRedirect = join(temporaryDirectory, "ignored-redirect");
  await mkdir(join(emptyValueHome, ".qwen"), { recursive: true });
  await writeFile(
    join(emptyValueHome, ".qwen", ".env"),
    `QWEN_HOME=${ignoredRedirect}\n`,
  );
  await installTarget("qwen", {
    HOME: emptyValueHome,
    USERPROFILE: emptyValueHome,
    QWEN_HOME: "",
  });
  await stat(join(emptyValueHome, ".qwen", "settings.json"));
  await assert.rejects(stat(join(ignoredRedirect, "settings.json")), {
    code: "ENOENT",
  });
});

test("Cursor installer manages a global Streamable HTTP MCP server", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-cursor-"),
  );
  const cursorConfigDirectory = join(temporaryDirectory, ".cursor");
  const configPath = join(cursorConfigDirectory, "mcp.json");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await installTarget("cursor", { CURSOR_CONFIG_DIR: cursorConfigDirectory }, [
    "--mcp-transport",
    "http",
    "--mcp-token-env",
    "ZVEC_GREP_SERVER_TOKEN",
  ]);

  const config = JSON.parse(await readFile(configPath, "utf8"));
  assert.deepEqual(config.mcpServers.zvec_grep, {
    url: "http://127.0.0.1:7999/mcp",
    headers: {
      Authorization: "Bearer ${ZVEC_GREP_SERVER_TOKEN}",
    },
  });

  await uninstallTarget("cursor", {
    CURSOR_CONFIG_DIR: cursorConfigDirectory,
  });
  const uninstalled = JSON.parse(await readFile(configPath, "utf8"));
  assert.equal(uninstalled.mcpServers, undefined);
});

test("OpenCode installer preserves config and manages a remote MCP server", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-opencode-"),
  );
  const configPath = join(temporaryDirectory, "opencode.json");
  const guidancePath = join(temporaryDirectory, "AGENTS.md");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });

  await writeFile(guidancePath, "# Existing OpenCode guidance\n");
  await writeFile(
    configPath,
    `${JSON.stringify({ model: "custom/model", mcp: { other: { type: "remote", url: "https://example.com/mcp" } } }, null, 2)}\n`,
  );
  await installTarget("opencode", { OPENCODE_CONFIG: configPath }, [
    "--mcp-transport=http",
    "--mcp-tool-timeout=900",
    "--mcp-token-env=ZVEC_GREP_SERVER_TOKEN",
  ]);

  const config = JSON.parse(await readFile(configPath, "utf8"));
  assert.equal(config.model, "custom/model");
  assert.equal(config.mcp.other.url, "https://example.com/mcp");
  assert.deepEqual(config.mcp.zvec_grep, {
    type: "remote",
    url: "http://127.0.0.1:7999/mcp",
    enabled: true,
    timeout: 900000,
    oauth: false,
    headers: {
      Authorization: "Bearer {env:ZVEC_GREP_SERVER_TOKEN}",
    },
  });
  const guidance = await readFile(guidancePath, "utf8");
  assert.match(guidance, /# Existing OpenCode guidance/);
  assert.match(
    guidance,
    /when an exact word, phrase, name, date,[^\n]+use `zvec_grep_zvec_grep_rg` when it is listed by the current host; otherwise native Grep or `rg`/i,
  );
  assert.match(
    guidance,
    /Use `zvec_grep_zvec_grep_search` when wording or location is unknown/,
  );
  assert.match(
    guidance,
    /Choose the evidence source before the retrieval mode/,
  );
  assert.match(guidance, /one focused `zvec_grep_zvec_grep_search` probe/);
  assert.match(guidance, /`zvec_grep_zvec_grep_rg` when it is listed/);
  assert.match(guidance, /probe does not apply to exact quotations/i);
  assert.match(
    guidance,
    /unrelated open-world questions, current external facts/,
  );
  assert.doesNotMatch(guidance, /managed-rg/);
  assert.equal(countOccurrences(guidance, "<!-- ZVEC_GREP_START -->"), 1);
  assert.equal(countOccurrences(guidance, "<!-- ZVEC_GREP_END -->"), 1);

  await uninstallTarget("opencode", { OPENCODE_CONFIG: configPath });
  const uninstalled = JSON.parse(await readFile(configPath, "utf8"));
  assert.equal(uninstalled.mcp.zvec_grep, undefined);
  assert.equal(uninstalled.mcp.other.url, "https://example.com/mcp");
  const uninstalledGuidance = await readFile(guidancePath, "utf8");
  assert.match(uninstalledGuidance, /# Existing OpenCode guidance/);
  assert.doesNotMatch(uninstalledGuidance, /ZVEC_GREP|## zvec-grep/);
});

test("JSON installers require force before replacing an unmanaged server", async (t) => {
  const temporaryDirectory = await mkdtemp(
    join(tmpdir(), "zvec-grep-install-json-conflict-"),
  );
  const configPath = join(temporaryDirectory, "opencode.json");
  t.after(async () => {
    await rm(temporaryDirectory, { recursive: true, force: true });
  });
  await writeFile(
    configPath,
    '{"mcp":{"zvec_grep":{"type":"remote","url":"https://example.com/mcp"}}}\n',
  );

  await assert.rejects(
    installTarget("opencode", { OPENCODE_CONFIG: configPath }),
    /--force/,
  );
  await installTarget("opencode", { OPENCODE_CONFIG: configPath }, [
    "--force",
    "--mcp-transport=http",
  ]);
  const config = JSON.parse(await readFile(configPath, "utf8"));
  assert.equal(config.mcp.zvec_grep.url, "http://127.0.0.1:7999/mcp");
});

test(
  "auto target installs only detected agents",
  {
    skip:
      process.platform === "win32" ? "PATH executable semantics differ" : false,
  },
  async (t) => {
    const temporaryDirectory = await mkdtemp(
      join(tmpdir(), "zvec-grep-install-auto-"),
    );
    const binaryDirectory = join(temporaryDirectory, "bin");
    const claudeConfigDirectory = join(temporaryDirectory, ".claude");
    const codexHome = join(temporaryDirectory, ".codex");
    t.after(async () => {
      await rm(temporaryDirectory, { recursive: true, force: true });
    });

    await mkdir(binaryDirectory, { recursive: true });
    const claudeExecutable = join(binaryDirectory, "claude");
    await writeFile(claudeExecutable, "#!/bin/sh\n");
    await chmod(claudeExecutable, 0o755);

    const { stdout } = await execFileAsync(
      process.execPath,
      [cliPath, "install", "--yes"],
      {
        env: {
          ...process.env,
          PATH: binaryDirectory,
          HOME: temporaryDirectory,
          CLAUDE_CONFIG_DIR: claudeConfigDirectory,
          CODEX_HOME: codexHome,
          ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
        },
      },
    );

    assert.match(stdout, /Claude Code/);
    assert.doesNotMatch(stdout, /Codex/);
    await stat(join(claudeConfigDirectory, ".claude.json"));
    await assert.rejects(stat(join(codexHome, "config.toml")), {
      code: "ENOENT",
    });
  },
);

test(
  "auto target installs Qwen Code when only qwen is detected",
  {
    skip:
      process.platform === "win32" ? "PATH executable semantics differ" : false,
  },
  async (t) => {
    const temporaryDirectory = await mkdtemp(
      join(tmpdir(), "zvec-grep-install-auto-qwen-"),
    );
    const binaryDirectory = join(temporaryDirectory, "bin");
    const qwenHome = join(temporaryDirectory, ".qwen");
    t.after(async () => {
      await rm(temporaryDirectory, { recursive: true, force: true });
    });

    await mkdir(binaryDirectory, { recursive: true });
    const qwenExecutable = join(binaryDirectory, "qwen");
    await writeFile(qwenExecutable, "#!/bin/sh\n");
    await chmod(qwenExecutable, 0o755);

    const { stdout } = await execFileAsync(
      process.execPath,
      [cliPath, "install", "--yes"],
      {
        env: {
          ...process.env,
          PATH: binaryDirectory,
          HOME: temporaryDirectory,
          USERPROFILE: temporaryDirectory,
          QWEN_HOME: qwenHome,
          ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
        },
      },
    );

    assert.match(stdout, /Qwen Code/);
    assert.doesNotMatch(stdout, /Claude Code|Codex|OpenCode|Cursor/);
    const config = JSON.parse(
      await readFile(join(qwenHome, "settings.json"), "utf8"),
    );
    assert.equal(config.mcpServers.zvec_grep.command, "zg");
  },
);

async function installCodex(codexHome, extraArgs = []) {
  await execFileAsync(
    process.execPath,
    [cliPath, "install", "--target", "codex", "--yes", ...extraArgs],
    {
      env: {
        ...process.env,
        CODEX_HOME: codexHome,
        ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
      },
    },
  );
}

async function uninstallCodex(codexHome, extraArgs = []) {
  await execFileAsync(
    process.execPath,
    [cliPath, "uninstall", "--target", "codex", "--yes", ...extraArgs],
    {
      env: {
        ...process.env,
        CODEX_HOME: codexHome,
      },
    },
  );
}

async function installTarget(target, env, extraArgs = []) {
  const environment = {
    ...process.env,
    ZVEC_GREP_INSTALL_SKIP_SERVER: "1",
    ...env,
  };
  for (const [key, value] of Object.entries(environment)) {
    if (value === undefined) delete environment[key];
  }
  return execFileAsync(
    process.execPath,
    [cliPath, "install", "--target", target, "--yes", ...extraArgs],
    { env: environment },
  );
}

async function uninstallTarget(target, env, extraArgs = []) {
  await execFileAsync(
    process.execPath,
    [cliPath, "uninstall", "--target", target, "--yes", ...extraArgs],
    { env: { ...process.env, ...env } },
  );
}

function countOccurrences(value, search) {
  return value.split(search).length - 1;
}

async function availablePort() {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  assert.ok(address && typeof address === "object");
  await new Promise((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
  return address.port;
}
