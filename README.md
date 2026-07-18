# ZZZManager

[简体中文](./README.zh-CN.md)

A local-first desktop application for monitoring balances across AI gateway accounts. ZZZManager combines account management, automatic balance checks, CNY credit conversion, history, and webhook notifications in one workspace.

- Author: `zhoushi1`
- Desktop stack: Tauri 2, Rust, React 19, and SQLite
- Data model: local single-user application, with no app login or cloud synchronization

## Highlights

- **Operational overview**: Account totals, attention items, recent checks, and recent notification deliveries.
- **Account management**: Add, edit, enable, disable, delete, annotate, link, sort, and drag to reorder gateway accounts.
- **Multiple views**: Dense list and table modes, plus filtering by enabled state and sorting by balance.
- **Manual and automatic checks**: Single-account and batch checks, a global automatic-check switch, global defaults, and per-account interval overrides.
- **Credit conversion**: Convert provider-reported USD credits into their CNY purchase value and evaluate low-balance alerts after conversion.
- **History and schedules**: Filter check history and inspect each account's next automatic check and eligibility state.
- **Notifications**: Generic webhook, Feishu, WeCom, and DingTalk delivery with test sends, delivery history, and configurable cooldowns.
- **Desktop integration**: System tray, launch on login, single-instance behavior, and in-app update checking.
- **Configuration portability**: Import and export settings, accounts, hooks, and optional credentials.
- **Appearance and language**: Light, dark, and system-following themes, plus Chinese and English UI.

## Supported Providers

| Provider | Credentials | Balance endpoint |
| --- | --- | --- |
| New API | Access Token + User ID | `GET /api/user/self` |
| Sub2API | API Key | `GET /v1/usage` |
| Custom HTTP | API Key and custom adapter configuration | Configurable GET/POST request |

Custom HTTP adapters support custom headers, an optional POST body, and JSON field paths for remaining, used, total, unit, validity, and message values. Header and body templates support `{{apiKey}}`, `{{baseUrl}}`, and `{{userAgent}}`; templates do not execute JavaScript.

## Credit Conversion

Some gateways credit multiple USD units for each CNY paid. Set the account's conversion rate to the number of USD credits received for `1 CNY`.

Example: with a rate of `12`, a provider balance of `120 USD` is displayed as an actual purchase value of `¥10`. The account threshold is then evaluated against `¥10`, and automatic low-balance notifications use the converted CNY values. Raw provider amounts remain unchanged in check history.

## Automatic Checks and Notifications

Automatic checks run only while the application process is active. Closing the window keeps the process running in the system tray; quitting from the tray stops automatic checks.

Notification events:

- Low balance, using the converted CNY balance when a conversion rate is configured.
- Three consecutive automatic check failures.
- Invalid credentials.

Only automatic checks dispatch notifications. Manual checks update the account snapshot and history without sending webhooks. Notification cooldowns suppress repeated events.

## Settings

- Default balance threshold and automatic-check interval.
- History retention and notification cooldown.
- Request User-Agent.
- System proxy, no proxy, or custom HTTP/HTTPS/SOCKS5 proxy.
- Launch on login.
- Light, dark, or system-following appearance. System mode is the default.
- Chinese or English interface.
- Configuration import and export.
- Application version, author, changelog, and update checking.

## Data and Security

- Accounts, credentials, settings, hooks, and history are stored in a local SQLite database.
- The current release does not use the operating system credential vault. Protect access to the local user account and application data directory.
- Configuration exports exclude credentials by default. Including credentials writes them to the JSON export in plaintext.
- Importing without credentials creates disabled placeholder accounts that can be completed later.
- The application does not provide cloud synchronization or a multi-user service.

## Downloads

Download releases from [GitHub Releases](https://github.com/zhoushi1/ZZZManager/releases). Release notes are generated from GitHub changes, and downloadable files are listed under each release's **Assets** section.

| Platform | Assets |
| --- | --- |
| Windows | `*_x64.msi`, `*_x64-setup.exe`, `*_x64-portable.zip` |
| macOS Apple Silicon | `*_aarch64.dmg` |
| macOS Intel | `*_x64.dmg` |
| Linux | `*_amd64.AppImage`, `*_amd64.deb`, `*_x86_64.rpm` |

Each release also includes `checksums.txt` with SHA-256 checksums for the published assets.

```bash
# Linux
sha256sum -c checksums.txt

# macOS
shasum -a 256 -c checksums.txt
```

```powershell
# Windows: compare the result with checksums.txt
Get-FileHash .\ZZZManager_<version>_x64.msi -Algorithm SHA256
```

Windows installers support current-user and all-users installation. The portable zip can be extracted and run without installation. If the installed application closes immediately on startup, inspect `%TEMP%\zzz-manager-panic.log`.

## Development

Prerequisites:

- Node.js 24 and pnpm 11.7.
- Stable Rust toolchain.
- The [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/) for the target operating system.

```bash
# Install frontend dependencies
pnpm install

# Run the desktop application in development mode
pnpm tauri dev

# Type-check and build the frontend
pnpm build

# Run backend tests
cargo test --lib --manifest-path src-tauri/Cargo.toml

# Build desktop bundles
pnpm tauri build
```

## Release Workflow

Version bumps are committed on `dev`; official tags are created from `main`.

```bash
git switch dev
pnpm version:bump x.x.x

git switch main
git merge dev
pnpm release x.x.x
```

- `pnpm version:bump <version>` updates `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and both lockfiles; it then runs checks and creates the version commit.
- `pnpm release <version>` verifies the synchronized version, runs checks, pushes `main`, and creates the `v<version>` tag.
- Add `--dry-run` to preview either command, `--skip-checks` to skip local checks, or `--push` to push the `dev` version commit.
- Pushing the tag builds Windows, macOS, and Linux bundles in GitHub Actions, publishes checksums, and creates a release with GitHub-generated notes.

Migration files under `src-tauri/migrations/` use LF line endings so sqlx checksums remain stable. Never edit a migration after it has shipped; add a new migration instead.

## Acknowledgements

ZZZManager benefits from the ideas, work, and open-source contributions of these projects and their communities:

- [CC Switch](https://github.com/farion1231/cc-switch) — a valuable reference for desktop product design and interaction patterns.
- [New API](https://github.com/QuantumNous/new-api) — advances the open-source AI gateway ecosystem and the account-management scenarios supported by ZZZManager.
- [Sub2API](https://github.com/Wei-Shaw/sub2api) — provides valuable ideas and practices for AI service subscription and quota management.
- [Tauri](https://github.com/tauri-apps/tauri) — provides the cross-platform desktop application framework.
- [shadcn/ui](https://github.com/shadcn-ui/ui) — provides the foundation and inspiration for the interface component system.

Thank you to every maintainer and contributor who makes open-source software available to the community.
