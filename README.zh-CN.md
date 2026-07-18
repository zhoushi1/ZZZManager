# ZZZManager

[English](./README.md)

ZZZManager 是一款本地优先的 AI 中转站余额管理桌面应用，将账号管理、自动余额查询、人民币额度换算、查询历史和 Webhook 通知集中在一个工作台中。

- 作者：`zhoushi1`
- 桌面技术栈：Tauri 2、Rust、React 19、SQLite
- 数据模式：本地单用户应用，无应用登录和云端同步

## 主要功能

- **运营概览**：查看账号数量、待处理问题、近期查询和近期通知投递。
- **账号管理**：新增、编辑、启用、禁用、删除、备注、官网跳转、排序和拖拽调整中转站账号。
- **多种视图**：紧凑列表和表格模式，支持按启用状态筛选及按余额排序。
- **手动与自动查询**：支持单账号和批量查询，并提供全局自动查询开关、全局默认值和账号级查询间隔。
- **额度换算**：将提供方返回的美元额度换算为人民币充值价值，并使用换算结果判断余额预警。
- **历史与调度**：筛选余额查询历史，查看每个账号的下次查询时间和调度资格。
- **通知渠道**：支持通用 Webhook、飞书、企业微信和钉钉，包含测试投递、投递历史和通知冷却时间。
- **桌面集成**：系统托盘、开机自启、单实例运行和应用内更新检查。
- **配置迁移**：导入和导出设置、账号、通知渠道及可选凭据。
- **外观与语言**：支持浅色、深色、跟随系统三种主题，以及中英文界面。

## 支持的提供方

| 提供方 | 凭据 | 余额接口 |
| --- | --- | --- |
| New API | Access Token + User ID | `GET /api/user/self` |
| Sub2API | API Key | `GET /v1/usage` |
| 自定义 HTTP | API Key 和自定义适配器配置 | 可配置 GET/POST 请求 |

自定义 HTTP 适配器支持请求头、可选 POST body，以及 remaining、used、total、unit、有效状态和消息等 JSON 字段路径。请求头和 body 模板可使用 `{{apiKey}}`、`{{baseUrl}}`、`{{userAgent}}` 占位符，模板不会执行 JavaScript。

## 额度换算

部分中转站会按照“充值 1 元，到账若干美元额度”的方式计费。账号中的充值比例表示 `1 元人民币` 可获得的美元额度。

例如比例设置为 `12` 时，提供方返回 `120 USD`，应用会显示实际充值价值约为 `¥10`。余额阈值将与 `¥10` 比较，自动余额预警通知也使用换算后的人民币金额；查询历史仍保留提供方返回的原始额度。

## 自动查询与通知

自动查询仅在应用进程运行时执行。关闭窗口会隐藏到系统托盘，自动查询继续运行；从托盘退出后才会停止。

通知触发条件：

- 余额不足；配置充值比例后按换算后的人民币余额判断。
- 连续三次自动查询失败。
- 凭据无效。

只有自动查询会发送通知。手动查询只更新账号快照和查询历史，不会触发 Webhook。通知冷却时间用于避免重复推送。

## 设置

- 默认余额阈值和自动查询间隔。
- 历史记录保留天数和通知冷却时间。
- HTTP 请求 User-Agent。
- 系统代理、不使用代理、自定义 HTTP/HTTPS/SOCKS5 代理。
- 开机自动启动。
- 浅色、深色或跟随系统的外观模式，默认跟随系统。
- 中文或英文界面。
- 配置导入和导出。
- 应用版本、作者、更新日志和更新检查。

## 数据与安全

- 账号、凭据、设置、通知渠道和查询历史保存在本地 SQLite 数据库中。
- 当前版本未使用操作系统凭据保险库，请保护本机用户账号和应用数据目录。
- 配置导出默认不包含凭据；勾选包含凭据后，敏感信息会以明文写入 JSON 文件。
- 导入时不包含凭据的账号会成为禁用占位账号，之后可补充凭据再启用。
- 应用不提供云端同步或多用户服务。

## 下载

请从 [GitHub Releases](https://github.com/zhoushi1/ZZZManager/releases) 下载。每个版本使用 GitHub 自动生成变更记录，安装包位于 Release 的 **Assets** 区域。

| 平台 | 产物 |
| --- | --- |
| Windows | `*_x64.msi`、`*_x64-setup.exe`、`*_x64-portable.zip` |
| macOS Apple Silicon | `*_aarch64.dmg` |
| macOS Intel | `*_x64.dmg` |
| Linux | `*_amd64.AppImage`、`*_amd64.deb`、`*_x86_64.rpm` |

每个版本还会发布 `checksums.txt`，其中包含所有产物的 SHA-256 校验值。

```bash
# Linux
sha256sum -c checksums.txt

# macOS
shasum -a 256 -c checksums.txt
```

```powershell
# Windows：将结果与 checksums.txt 对比
Get-FileHash .\ZZZManager_<版本>_x64.msi -Algorithm SHA256
```

Windows 安装包支持“当前用户”和“所有用户”两种安装范围；免安装 zip 解压后可直接运行。如果安装后的应用启动后立即关闭，请查看 `%TEMP%\zzz-manager-panic.log`。

## 开发

环境要求：

- Node.js 24 和 pnpm 11.7。
- Rust stable 工具链。
- 目标操作系统对应的 [Tauri 2 环境依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
# 安装前端依赖
pnpm install

# 启动桌面开发模式
pnpm tauri dev

# 类型检查并构建前端
pnpm build

# 运行后端测试
cargo test --lib --manifest-path src-tauri/Cargo.toml

# 构建桌面安装包
pnpm tauri build
```

## 发布流程

版本更新提交在 `dev` 分支完成，正式版本 tag 从 `main` 分支创建。

```bash
git switch dev
pnpm version:bump x.x.x

git switch main
git merge dev
pnpm release x.x.x
```

- `pnpm version:bump <版本>` 会更新 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 和两个 lockfile，随后执行检查并创建版本提交。
- `pnpm release <版本>` 会校验各处版本一致，执行检查，推送 `main` 并创建 `v<版本>` tag。
- 两个命令都支持 `--dry-run` 预览和 `--skip-checks` 跳过本地检查；版本更新命令还可使用 `--push` 推送 `dev` 提交。
- 推送 tag 后，GitHub Actions 会构建 Windows、macOS、Linux 产物，生成校验文件，并使用 GitHub 自动生成的变更记录创建 Release。

`src-tauri/migrations/` 下的迁移文件使用 LF 行尾，以保证 sqlx checksum 稳定。迁移发布后不要修改，只能新增迁移。

## 致谢

ZZZManager 的诞生离不开以下开源项目及其社区提供的思路、实践与贡献：

- [CC Switch](https://github.com/farion1231/cc-switch)：为桌面端产品设计和交互方式提供了宝贵参考。
- [New API](https://github.com/QuantumNous/new-api)：推动了开源 AI 网关生态发展，也为 ZZZManager 所服务的账号管理场景提供了基础。
- [Sub2API](https://github.com/Wei-Shaw/sub2api)：为 AI 服务的订阅与额度管理提供了宝贵思路和实践。
- [Tauri](https://github.com/tauri-apps/tauri)：为本项目提供跨平台桌面应用框架。
- [shadcn/ui](https://github.com/shadcn-ui/ui)：为界面组件体系提供基础与设计灵感。

感谢所有维护者和贡献者为开源社区付出的时间与心血。
