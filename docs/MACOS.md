# DevShell macOS 预览版

## 安装

支持 macOS 15 或更高版本；兼容与验证范围是最近的几个正式版（macOS 15、26、27），更早版本不再适配。Universal 通用应用同时包含 Apple 芯片 arm64 和 Intel x86_64 原生程序。

打开 `DevShell-macos-universal.dmg`，把 `DevShell.app` 拖到 `Applications` 后启动；也可以解压 ZIP 后拖入应用程序文件夹。SSH 终端和文件管理无需另装 Go、Node.js、WebView2 或 GTK。

当前默认构建采用本地临时签名（ad-hoc），尚未经过 Apple Developer ID 签名和公证。它是供测试的预览版。通过浏览器下载后，macOS 可能阻止第一次打开；核对来源后，可在“系统设置 → 隐私与安全性”中对该应用选择“仍要打开”。无需关闭系统的 Gatekeeper。

## 配置和功能

- 默认配置保存于 `~/Library/Application Support/DevShell/`，包含加密配置、密钥、导入字体和背景。更新或移动应用时保留整个目录。
- 从旧版 DengShell 升级时，首次启动会把原 `~/Library/Application Support/DengShell/` 整体改名为 `DevShell`，配置无需手动迁移。
- `--config /绝对路径` 可指定隔离配置。裸可执行程序仍使用旁边的 `data/`。
- macOS 使用 GPUI 原生窗口、菜单栏和 Dock；支持系统明暗主题和原生文件选择。
- 最小化进入 Dock；当前未提供 macOS 状态栏托盘。
- 独立窗口支持通过“合并到窗口”菜单合并；跨窗口鼠标定位暂不支持。
- 本机 IPv4/IPv6 延迟检测使用系统 ping/ping6。Linux 远端监控仍通过 SSH 执行。
- 本机 MTR 是可选功能：在 Mac 终端中执行 `brew install mtr`，再按 Homebrew 提示配置 `mtr-packet` 权限。Finder 启动也能查找标准 Homebrew 安装位置；DevShell 不自动提权。远端 MTR 继续使用远端 Linux 安装流程。
- 内置更新使用 GitHub Releases：「检查更新」读取 `up-macos-arm64.json` / `up-macos-amd64.json`，下载通用应用包 `DevShell-macos-universal.zip`，校验 SHA-256 与构建号后退出应用、原子替换 `.app` 并自动启动新版本。替换要求当前 `.app` 所在位置可写，因此请先拖入「应用程序」再使用，不要直接从 DMG 卷运行；配置目录 `~/Library/Application Support/DevShell/` 保持不变。
- 第三方编辑器可以选择 `.app`；通过 macOS `open -a` 打开文件。

## 从源码构建

在 Mac 上安装 Xcode Command Line Tools、Rust/Cargo 1.98+、Go 1.27.1（本项目验证版本）、Node.js 22 和 Python 3。应用运行时不需要这些开发工具。

```bash
xcode-select --install
npm ci
npm run vendor
python3 scripts/tools/check-environment.py
python3 scripts/build/build-macos.py --arch universal --jobs 4
python3 scripts/tests/test-macos-app.py build/macos/DevShell.app
```

输出位于 `build/macos/`，包括 `.app`、DMG、ZIP、两份更新清单（`up-macos-arm64.json` / `up-macos-amd64.json`，仅 universal 构建产出，供内置更新使用）、校验清单和构建信息。已有应用不会被覆盖，可通过 `--output 新目录` 保存下一次构建。

构建默认最多使用 4 个并发任务，并按机器实际 CPU 数量收缩。macOS 不使用 Linux 的 `taskset`，因此该设置限制构建并发数，不保证绑定编号 0～3 的 CPU。

脚本使用系统 SDK、Cocoa、CGO、Cargo 和 `lipo` 合并双架构 Go 后端与 GPUI 前端，不需要额外安装 Wails CLI。Ubuntu 能测试共享后端，但不能在缺少 Apple SDK 的情况下构建这个原生桌面应用。

最低系统固定在 macOS 15.0：`MACOSX_DEPLOYMENT_TARGET`、`-mmacosx-version-min` 与 `Info.plist` 的 `LSMinimumSystemVersion` 同源，构建信息里记录为 `minimumMacOS`。脚本先检查所用 SDK 不低于该值，否则直接报错停止（GitHub Actions 因此使用 macOS 15 构建机；更旧的构建机会先失败，而不是产出加载命令与 SDK 不一致的包）。

GitHub Actions 的 `.github/workflows/release.yml` 在推送 tag 时由 macOS 任务在 macOS 环境中构建通用包，产物连同校验清单发布到对应 Release；需要原生启动验证时，可在构建机上运行 `scripts/tests/test-macos-app.py` 验证 `.app`。

## 正式分发签名

有 Apple Developer ID Application 证书时，使用 `--sign-identity 'Developer ID Application: …'` 构建。该选项签名应用并开启 hardened runtime；仍须使用自己的 Apple 开发者账户通过 `xcrun notarytool` 公证，再使用 `xcrun stapler` 装订票据并重新打包 ZIP/DMG、重新计算校验值。

签名证书和 Apple 账户凭据不应放入源码或分发包。没有公证记录的包不标记为“已公证”。

## 技术依据

- [GPUI Kit](https://gpui-kit.com/docs/getting-started/)
- [ELY GPUI Components](https://elygpui.com/)
- [Go 最低系统要求](https://go.dev/wiki/MinimumRequirements)
- [GitHub macOS 构建环境](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
