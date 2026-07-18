#!/usr/bin/env node
// @ts-check
/**
 * Version bump automation for this project.
 *
 * Usage:
 *   pnpm version:bump <version> [--dry-run] [--skip-checks] [--push]
 *
 * Examples:
 *   pnpm version:bump 0.1.2
 *   pnpm version:bump v0.1.2
 *   pnpm version:bump 0.1.2 --dry-run
 *   pnpm version:bump 0.1.2 --skip-checks
 *   pnpm version:bump 0.1.2 --push
 *
 * 该脚本只在 dev 分支执行，负责：
 *   1. 校验前置条件（分支必须是 dev、工作区干净）
 *   2. 同步更新 package.json / src-tauri/Cargo.toml / src-tauri/tauri.conf.json 的版本号
 *   3. 更新 pnpm lockfile 与 src-tauri/Cargo.lock
 *   4. 运行本地检查（pnpm build、cargo test --lib），--skip-checks 可跳过
 *   5. 提交版本 bump；可选 --push 推送 dev
 *
 * 正式发版（打 tag、触发 GitHub Actions）由 main 分支上的 `pnpm release` 负责。
 */

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const ROOT = join(__dirname, "..");

const PKG_JSON = join(ROOT, "package.json");
const CARGO_TOML = join(ROOT, "src-tauri", "Cargo.toml");
const TAURI_CONF = join(ROOT, "src-tauri", "tauri.conf.json");
const SRC_TAURI = join(ROOT, "src-tauri");

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z-.]+)?(?:\+[0-9A-Za-z-.]+)?$/;

// ---- 参数解析 -------------------------------------------------------------

/** @param {string[]} argv */
function parseArgs(argv) {
  const flags = { dryRun: false, skipChecks: false, push: false };
  /** @type {string[]} */
  const positionals = [];
  for (const arg of argv) {
    if (arg === "--dry-run") flags.dryRun = true;
    else if (arg === "--skip-checks") flags.skipChecks = true;
    else if (arg === "--push") flags.push = true;
    else if (arg.startsWith("--")) fail(`未知参数: ${arg}`);
    else positionals.push(arg);
  }
  if (positionals.length === 0) {
    fail("缺少版本号参数。用法: pnpm version:bump <version> [--dry-run] [--skip-checks] [--push]");
  }
  if (positionals.length > 1) {
    fail(`只接受一个版本号参数，收到多个: ${positionals.join(", ")}`);
  }
  return { rawVersion: positionals[0], ...flags };
}

/** 归一化版本号：去掉前导 v，校验 semver。返回 { version, tag }。 */
function normalizeVersion(raw) {
  const version = raw.startsWith("v") || raw.startsWith("V") ? raw.slice(1) : raw;
  if (!SEMVER_RE.test(version)) {
    fail(`无效的版本号: "${raw}"。请提供合法的 semver，例如 0.1.2 或 v0.1.2。`);
  }
  return { version, tag: `v${version}` };
}

// ---- 日志与错误 -----------------------------------------------------------

/** @param {string} msg */
function log(msg) {
  console.log(msg);
}
/** @param {string} msg */
function info(msg) {
  console.log(`\x1b[36m›\x1b[0m ${msg}`);
}
/** @param {string} msg */
function ok(msg) {
  console.log(`\x1b[32m✓\x1b[0m ${msg}`);
}
/** @param {string} msg */
function fail(msg) {
  console.error(`\x1b[31m✗ 版本 bump 失败:\x1b[0m ${msg}`);
  process.exit(1);
}

// ---- 命令执行 -------------------------------------------------------------

/**
 * 运行命令并捕获输出，失败时抛错（用于前置校验查询）。
 * @param {string} cmd
 * @param {string[]} args
 * @param {{ cwd?: string, allowFailure?: boolean }} [opts]
 * @returns {{ status: number, stdout: string, stderr: string }}
 */
function capture(cmd, args, opts = {}) {
  const res = spawnSync(cmd, args, {
    cwd: opts.cwd ?? ROOT,
    encoding: "utf8",
    // git/cargo/pnpm 在 PATH 中即可；Windows 下 pnpm 是 .cmd，需要 shell。
    shell: process.platform === "win32" && (cmd === "pnpm" || cmd === "npm"),
  });
  if (res.error) {
    if (opts.allowFailure) return { status: 1, stdout: "", stderr: String(res.error.message) };
    fail(`无法执行命令 ${cmd}: ${res.error.message}`);
  }
  return {
    status: res.status ?? 1,
    stdout: (res.stdout ?? "").trim(),
    stderr: (res.stderr ?? "").trim(),
  };
}

/**
 * 运行命令并将输出直接透传到终端；失败即终止。
 * dry-run 模式下只打印命令。
 * @param {string} cmd
 * @param {string[]} args
 * @param {{ cwd?: string, dryRun: boolean }} opts
 */
function run(cmd, args, opts) {
  const cwd = opts.cwd ?? ROOT;
  const display = `${cmd} ${args.join(" ")}`;
  const cwdNote = cwd === ROOT ? "" : ` (cwd: ${relativeToRoot(cwd)})`;
  if (opts.dryRun) {
    log(`  [dry-run] $ ${display}${cwdNote}`);
    return;
  }
  info(`$ ${display}${cwdNote}`);
  const res = spawnSync(cmd, args, {
    cwd,
    stdio: "inherit",
    shell: process.platform === "win32" && (cmd === "pnpm" || cmd === "npm"),
  });
  if (res.error) fail(`命令执行出错 (${display}): ${res.error.message}`);
  if ((res.status ?? 1) !== 0) fail(`命令返回非零退出码 (${display}): ${res.status}`);
}

/**
 * 运行命令但丢弃 stdout（stderr 仍继承到终端，便于失败诊断）；失败即终止。
 * 用于会向 stdout 输出大量内容、但我们只关心其副作用的命令（如 cargo metadata 刷新 Cargo.lock）。
 * dry-run 模式下只打印命令。
 * @param {string} cmd
 * @param {string[]} args
 * @param {{ cwd?: string, dryRun: boolean, note?: string }} opts
 */
function runSilent(cmd, args, opts) {
  const cwd = opts.cwd ?? ROOT;
  const display = `${cmd} ${args.join(" ")}`;
  const cwdNote = cwd === ROOT ? "" : ` (cwd: ${relativeToRoot(cwd)})`;
  if (opts.dryRun) {
    log(`  [dry-run] $ ${display}${cwdNote}`);
    return;
  }
  info(opts.note ?? `$ ${display}${cwdNote}（输出已隐藏）`);
  const res = spawnSync(cmd, args, {
    cwd,
    stdio: ["ignore", "ignore", "inherit"],
    shell: process.platform === "win32" && (cmd === "pnpm" || cmd === "npm"),
  });
  if (res.error) fail(`命令执行出错 (${display}): ${res.error.message}`);
  if ((res.status ?? 1) !== 0) fail(`命令返回非零退出码 (${display}): ${res.status}`);
}

/** @param {string} p */
function relativeToRoot(p) {
  return p.replace(ROOT, ".").replace(/\\/g, "/");
}

// ---- 前置校验 -------------------------------------------------------------

function preflight() {
  info("执行版本 bump 前置校验...");

  // a) 当前分支必须是 dev
  const branch = capture("git", ["rev-parse", "--abbrev-ref", "HEAD"]);
  if (branch.status !== 0) fail(`无法获取当前 git 分支: ${branch.stderr}`);
  if (branch.stdout !== "dev") {
    fail(`当前分支为 "${branch.stdout}"，版本 bump 必须在 dev 分支上执行。`);
  }
  ok("分支检查通过 (dev)");

  // b) 工作区必须干净
  const status = capture("git", ["status", "--porcelain"]);
  if (status.status !== 0) fail(`无法获取 git 状态: ${status.stderr}`);
  if (status.stdout !== "") {
    fail(
      "工作区存在未提交的改动，请先提交或清理后再执行版本 bump。当前改动:\n" +
        status.stdout
          .split("\n")
          .map((l) => `    ${l}`)
          .join("\n"),
    );
  }
  ok("工作区干净");
}

// ---- 版本写入 -------------------------------------------------------------

/**
 * @typedef {{ file: string, rel: string, old: string, newContent: string, note: string }} PendingWrite
 */

/**
 * 解析并生成 package.json / tauri.conf.json（JSON）的新内容，保留 2 空格缩进与末尾换行。
 * 只做解析与内容生成，不写文件；解析失败直接 fail（退出），从而阻止后续任何写入。
 * @param {string} file
 * @param {string} version
 * @returns {PendingWrite}
 */
function prepareJsonVersion(file, version) {
  const rel = relativeToRoot(file);
  const raw = readFileSync(file, "utf8");
  let data;
  try {
    data = JSON.parse(raw);
  } catch (err) {
    fail(`解析 ${rel} 失败: ${err instanceof Error ? err.message : String(err)}`);
    throw err; // 不可达：fail() 已退出进程，仅用于满足类型检查。
  }
  const old = String(data.version ?? "");
  data.version = version;
  const hadTrailingNewline = raw.endsWith("\n");
  const newContent = JSON.stringify(data, null, 2) + (hadTrailingNewline ? "\n" : "");
  return { file, rel, old, newContent, note: `${old} -> ${version}` };
}

/**
 * 解析 Cargo.toml，只替换 [package] 段内的 version，不动依赖版本。
 * 只做解析与内容生成，不写文件；找不到 version 行则 fail（退出），阻止后续任何写入。
 *
 * 逐行处理时保留每行原始行尾（CRLF 或 LF），避免把 CRLF 全改成 LF：
 *   - 用 `split(/(?<=\n)/)` 让每个元素带上自己的 `\r\n` / `\n`；
 *   - version 行正则不依赖 `.` 匹配 `\r`，闭合引号后用 `[^\r\n]*` 吃掉行内剩余内容，
 *     并单独保留可选的 `\r?\n` 行尾，从而正确匹配 CRLF 与 LF。
 * @param {string} version
 * @returns {PendingWrite}
 */
function prepareCargoVersion(version) {
  const rel = relativeToRoot(CARGO_TOML);
  const raw = readFileSync(CARGO_TOML, "utf8");
  const lines = raw.split(/(?<=\n)/);

  let inPackage = false;
  let replaced = false;
  let oldVersion = "";
  const versionLineRe = /^(\s*version\s*=\s*")([^"]*)("[^\r\n]*(?:\r?\n)?)$/;

  const out = lines.map((line) => {
    const sectionMatch = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (sectionMatch) {
      inPackage = sectionMatch[1].trim() === "package";
      return line;
    }
    if (inPackage && !replaced) {
      const m = line.match(versionLineRe);
      if (m) {
        oldVersion = m[2];
        replaced = true;
        return `${m[1]}${version}${m[3]}`;
      }
    }
    return line;
  });

  if (!replaced) {
    fail(`无法在 ${rel} 的 [package] 段中找到 version 字段。`);
  }
  return {
    file: CARGO_TOML,
    rel,
    old: oldVersion,
    newContent: out.join(""),
    note: `[package] version ${oldVersion} -> ${version}`,
  };
}

// ---- 主流程 ---------------------------------------------------------------

function main() {
  const { rawVersion, dryRun, skipChecks, push } = parseArgs(process.argv.slice(2));
  const { version, tag } = normalizeVersion(rawVersion);

  log("");
  info(`准备将版本 bump 到 ${version} (tag ${tag})`);
  if (dryRun) info("模式: --dry-run（不修改文件、不 commit、不 push）");
  if (skipChecks) info("模式: --skip-checks（跳过 pnpm build 与 cargo test --lib）");
  if (push) info("模式: --push（提交后推送 origin dev）");
  log("");

  // 1. 前置校验
  preflight();
  log("");

  // 2. 更新版本号
  //   先解析并生成三个文件的新内容（任一解析失败会在 prepare* 内 fail 退出，
  //   此时尚未写入任何文件，避免半截修改）；三者都准备成功后再统一写入。
  info("更新版本号...");
  const pending = [
    prepareJsonVersion(PKG_JSON, version),
    prepareCargoVersion(version),
    prepareJsonVersion(TAURI_CONF, version),
  ];
  if (dryRun) {
    for (const p of pending) {
      log(`  [dry-run] 更新 ${p.rel}: ${p.note}`);
    }
  } else {
    for (const p of pending) {
      writeFileSync(p.file, p.newContent, "utf8");
      ok(`更新 ${p.rel}: ${p.note}`);
    }
  }
  log("");

  // 3. 更新 lockfile（pnpm-lock.yaml 与 src-tauri/Cargo.lock）
  info("更新 lockfile...");
  run("pnpm", ["install", "--lockfile-only"], { dryRun });
  // cargo metadata 会读取 Cargo.toml 并刷新 Cargo.lock 中本包的版本，
  // 但不会跑完整测试；无论是否 --skip-checks 都要执行。
  // 它会向 stdout 输出大量 JSON，这里用 runSilent 丢弃 stdout、保留 stderr。
  runSilent("cargo", ["metadata", "--format-version", "1", "--quiet"], {
    dryRun,
    cwd: SRC_TAURI,
    note: "$ cargo metadata --format-version 1 --quiet (cwd: ./src-tauri) — 刷新 Cargo.lock（JSON 输出已隐藏）",
  });
  log("");

  // 4. 本地检查
  if (skipChecks) {
    info("跳过本地检查（--skip-checks）");
  } else {
    info("运行本地检查...");
    run("pnpm", ["build"], { dryRun });
    run("cargo", ["test", "--lib"], { dryRun, cwd: SRC_TAURI });
  }
  log("");

  // 5. 提交版本 bump
  info("提交版本 bump...");
  const filesToAdd = [
    "package.json",
    "pnpm-lock.yaml",
    "src-tauri/Cargo.toml",
    "src-tauri/Cargo.lock",
    "src-tauri/tauri.conf.json",
  ];
  run("git", ["add", ...filesToAdd], { dryRun });
  run("git", ["commit", "-m", `chore: bump version to ${tag}`], { dryRun });
  if (push) {
    run("git", ["push", "origin", "dev"], { dryRun });
  }
  log("");

  if (dryRun) {
    ok(`dry-run 完成：以上为版本 bump 到 ${version} 将执行的步骤，未做任何实际改动。`);
  } else if (push) {
    ok(`版本 bump 完成并已推送 dev！接着可合并 dev 到 main，再在 main 上执行 pnpm release ${version}。`);
  } else {
    ok(`版本 bump 完成（已本地提交，未推送）！可 git push origin dev 后合并到 main，再执行 pnpm release ${version}。`);
  }
}

main();
