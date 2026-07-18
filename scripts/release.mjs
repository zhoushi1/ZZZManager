#!/usr/bin/env node
// @ts-check
/**
 * Release automation for this project.
 *
 * Usage:
 *   pnpm release <version> [--dry-run] [--skip-checks]
 *
 * Examples:
 *   pnpm release 0.1.2
 *   pnpm release v0.1.2
 *   pnpm release 0.1.2 --dry-run
 *   pnpm release 0.1.2 --skip-checks
 *
 * 该脚本只在 main 分支执行，且不修改任何文件、不提交版本号。
 * 它负责：
 *   1. 校验发布前置条件（分支必须是 main、工作区干净、origin 存在、tag 不存在）
 *   2. 校验当前 main 上的版本号已经等于目标版本（否则提示先在 dev 执行 version:bump 并合并）
 *   3. 运行本地检查（pnpm build、cargo test --lib），--skip-checks 可跳过
 *   4. 推送 main、创建并推送 tag，从而触发 GitHub Actions 打包
 *
 * 版本号更新与提交由 dev 分支上的 `pnpm version:bump` 负责。
 */

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const ROOT = join(__dirname, "..");

const PKG_JSON = join(ROOT, "package.json");
const CARGO_TOML = join(ROOT, "src-tauri", "Cargo.toml");
const TAURI_CONF = join(ROOT, "src-tauri", "tauri.conf.json");
const CARGO_LOCK = join(ROOT, "src-tauri", "Cargo.lock");
const SRC_TAURI = join(ROOT, "src-tauri");

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z-.]+)?(?:\+[0-9A-Za-z-.]+)?$/;

// ---- 参数解析 -------------------------------------------------------------

/** @param {string[]} argv */
function parseArgs(argv) {
  const flags = { dryRun: false, skipChecks: false };
  /** @type {string[]} */
  const positionals = [];
  for (const arg of argv) {
    if (arg === "--dry-run") flags.dryRun = true;
    else if (arg === "--skip-checks") flags.skipChecks = true;
    else if (arg.startsWith("--")) fail(`未知参数: ${arg}`);
    else positionals.push(arg);
  }
  if (positionals.length === 0) {
    fail("缺少版本号参数。用法: pnpm release <version> [--dry-run] [--skip-checks]");
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
  console.error(`\x1b[31m✗ 发布失败:\x1b[0m ${msg}`);
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
 * 运行命令并将输出直接透传到终端；失败即终止发布。
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

/** @param {string} p */
function relativeToRoot(p) {
  return p.replace(ROOT, ".").replace(/\\/g, "/");
}

// ---- 版本读取 -------------------------------------------------------------

/**
 * 读取 package.json / tauri.conf.json（JSON）中的 version 字段。
 * @param {string} file
 * @returns {string}
 */
function readJsonVersion(file) {
  const rel = relativeToRoot(file);
  const data = JSON.parse(readFileSync(file, "utf8"));
  if (typeof data.version !== "string") {
    fail(`无法在 ${rel} 中读取到 version 字段。`);
  }
  return data.version;
}

/** 读取 Cargo.toml [package] 段内的 version。 */
function readCargoTomlVersion() {
  const rel = relativeToRoot(CARGO_TOML);
  const lines = readFileSync(CARGO_TOML, "utf8").split("\n");
  let inPackage = false;
  const versionLineRe = /^\s*version\s*=\s*"([^"]*)"/;
  for (const line of lines) {
    const sectionMatch = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (sectionMatch) {
      inPackage = sectionMatch[1].trim() === "package";
      continue;
    }
    if (inPackage) {
      const m = line.match(versionLineRe);
      if (m) return m[1];
    }
  }
  fail(`无法在 ${rel} 的 [package] 段中找到 version 字段。`);
  return ""; // unreachable
}

/** 读取 Cargo.lock 中当前 package.json 包名对应包的 version。 */
function readCargoLockVersion() {
  const rel = relativeToRoot(CARGO_LOCK);
  const raw = readFileSync(CARGO_LOCK, "utf8");
  const packageName = JSON.parse(readFileSync(PKG_JSON, "utf8")).name;
  if (typeof packageName !== "string" || packageName.length === 0) {
    fail(`无法在 ${relativeToRoot(PKG_JSON)} 中读取包名。`);
  }
  const escapedName = packageName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const m = raw.match(
    new RegExp(
      `name\\s*=\\s*"${escapedName}"\\s*\\r?\\n\\s*version\\s*=\\s*"([^"]*)"`,
    ),
  );
  if (!m) {
    fail(`无法在 ${rel} 中找到 ${packageName} 包的 version。`);
  }
  return m[1];
}

/**
 * 校验所有版本文件的版本号是否等于目标版本，不一致则报错。
 * @param {string} version
 */
function verifyVersions(version) {
  info("校验当前 main 上的版本号...");
  const checks = [
    { label: relativeToRoot(PKG_JSON), actual: readJsonVersion(PKG_JSON) },
    { label: relativeToRoot(CARGO_TOML), actual: readCargoTomlVersion() },
    { label: relativeToRoot(TAURI_CONF), actual: readJsonVersion(TAURI_CONF) },
    { label: relativeToRoot(CARGO_LOCK), actual: readCargoLockVersion() },
  ];
  const mismatched = checks.filter((c) => c.actual !== version);
  if (mismatched.length > 0) {
    const detail = mismatched
      .map((c) => `    ${c.label}: 当前 ${c.actual}，期望 ${version}`)
      .join("\n");
    fail(
      `当前 main 上的版本号与目标版本 ${version} 不一致:\n${detail}\n\n` +
        `release 不再修改版本文件。请先在 dev 分支执行:\n` +
        `    pnpm version:bump ${version}\n` +
        `然后将 dev 合并到 main，再在 main 上执行 pnpm release ${version}。`,
    );
  }
  ok(`版本号已一致 (${version})`);
}

// ---- 前置校验 -------------------------------------------------------------

/** @param {string} tag */
function preflight(tag) {
  info("执行发布前置校验...");

  // a) 当前分支必须是 main
  const branch = capture("git", ["rev-parse", "--abbrev-ref", "HEAD"]);
  if (branch.status !== 0) fail(`无法获取当前 git 分支: ${branch.stderr}`);
  if (branch.stdout !== "main") {
    fail(`当前分支为 "${branch.stdout}"，必须在 main 分支上发布。`);
  }
  ok("分支检查通过 (main)");

  // b) 工作区必须干净
  const status = capture("git", ["status", "--porcelain"]);
  if (status.status !== 0) fail(`无法获取 git 状态: ${status.stderr}`);
  if (status.stdout !== "") {
    fail(
      "工作区存在未提交的改动，请先提交或清理后再发布。当前改动:\n" +
        status.stdout
          .split("\n")
          .map((l) => `    ${l}`)
          .join("\n"),
    );
  }
  ok("工作区干净");

  // c) 远端 origin 必须存在
  const remotes = capture("git", ["remote"]);
  if (remotes.status !== 0) fail(`无法获取 git 远端列表: ${remotes.stderr}`);
  if (!remotes.stdout.split("\n").includes("origin")) {
    fail('未找到名为 "origin" 的远端。请先配置 git remote origin。');
  }
  ok("远端 origin 存在");

  // d) tag 不得已存在（本地）
  const localTag = capture("git", ["rev-parse", "-q", "--verify", `refs/tags/${tag}`], {
    allowFailure: true,
  });
  if (localTag.status === 0) {
    fail(`本地已存在 tag ${tag}，请先删除或使用新的版本号。`);
  }
  ok(`本地不存在 tag ${tag}`);

  // e) tag 不得已存在（远端）
  const remoteTag = capture("git", ["ls-remote", "--tags", "origin", `refs/tags/${tag}`]);
  if (remoteTag.status !== 0) {
    fail(`无法查询远端 tag（请检查网络与远端权限）: ${remoteTag.stderr}`);
  }
  if (remoteTag.stdout !== "") {
    fail(`远端 origin 已存在 tag ${tag}，请使用新的版本号。`);
  }
  ok(`远端不存在 tag ${tag}`);
}

// ---- 主流程 ---------------------------------------------------------------

function main() {
  const { rawVersion, dryRun, skipChecks } = parseArgs(process.argv.slice(2));
  const { version, tag } = normalizeVersion(rawVersion);

  log("");
  info(`准备发布版本 ${version} (tag ${tag})`);
  if (dryRun) info("模式: --dry-run（不推送、不打 tag）");
  if (skipChecks) info("模式: --skip-checks（跳过本地检查命令）");
  log("");

  // 1. 前置校验
  preflight(tag);
  log("");

  // 2. 校验版本号已经等于目标版本（release 不再修改版本文件）
  verifyVersions(version);
  log("");

  // 3. 本地检查
  if (skipChecks) {
    info("跳过本地检查（--skip-checks）");
  } else {
    info("运行本地检查...");
    run("pnpm", ["build"], { dryRun });
    run("cargo", ["test", "--lib"], { dryRun, cwd: SRC_TAURI });
  }
  log("");

  // 4. 推送 main、打 tag、推送 tag
  info("推送与打 tag...");
  run("git", ["push", "origin", "main"], { dryRun });
  run("git", ["tag", "-a", tag, "-m", tag], { dryRun });
  run("git", ["push", "origin", tag], { dryRun });
  log("");

  if (dryRun) {
    ok(`dry-run 完成：以上为发布 ${version} 将执行的步骤，未做任何实际改动。`);
  } else {
    ok(`发布完成！tag ${tag} 已推送，GitHub Actions 将开始构建并创建草稿 Release。`);
  }
}

main();
