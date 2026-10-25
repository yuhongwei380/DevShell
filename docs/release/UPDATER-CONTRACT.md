# DevShell 更新器约定

当前协议统一维护于 [在线更新协议](ONLINE-UPDATE-DESIGN.md)，签名细节见 [发行签名](SIGNED-UPDATES.md)，避免两份协议出现不一致。

- Windows：固定 `/up.exe.json` 与 `/up.exe`。
- macOS（构建 20261022140 起）：固定 `/up-macos-arm64.json` 与 `/up-macos-amd64.json`，两份描述都指向同一个 `/DevShell-macos-universal.zip`；平台字段分别为 `darwin-arm64` / `darwin-amd64`，跨架构描述互不接受。载荷格式 `app-zip`：解压后校验 `Info.plist`、可执行位与包内程序 `executableSHA256`，再做目录级 rename 原子替换 `.app`，旧包留 `<bundle>.update-old` 供回滚；安装位置不可写（例如仍从 DMG 卷运行）在旧进程退出前即拒绝，全程不提权。
- Debian/Ubuntu：固定 `/up.deb.json` 与 `/up.deb`。
- Arch pacman（R28 起）：固定 `/up.pkg.tar.zst.json` 与 `/up.pkg.tar.zst`，签名中的平台为 `linux-amd64-pacman`。
- 发版策略（2026-10-20）：仅发布 Windows 与 macOS；Linux 载荷 `up.deb` / `up.pkg.tar.zst` 及其描述文件不再发布，端点名保留仅供已部署的 Linux 客户端探测——返回 404 时客户端提示「Linux 版已停止发布更新」，不再误报网络故障。
- 来源固定为 `https://github.com/yuhongwei380/DevShell` 的 Releases 下载（仅允许 GitHub 官方资产主机 `objects.githubusercontent.com` 与 `release-assets.githubusercontent.com` 跳转），不接受其他跨域更新下载或描述文件指定的命令。
- 描述文件可不带签名（信任锚为 GitHub Releases 与 HTTPS）；一旦携带任何签名字段，则必须通过 Ed25519 验证。
- 版本号规则 `vX.Y.Z`：X 为年份后两位，Y 为月份（不补零），Z 为当月第几个版本（从 0 起）；`build` 为 `YYYYMMDD` + 两位当日序号（2026-10-24 起；此前为三位序号，历史十一位构建号继续有效，已安装旧构建需手动安装新构建一次），每次发布递增，禁止回退。
- schema 2；新版强制验证 SHA-256、大小、版本与构建高水位，描述文件携带签名字段时额外强制 Ed25519 验证与有效期；Arch 独立验证包格式、名称、架构和 pkgrel。
- 手动安装更高构建后，旧收据的软件版本下限由当前程序取代；收据构建高水位仍受保护。无构建号、同构建或更高构建的收据继续限制软件版本，候选更新必须高于当前及收据构建。
- 启动检查，用户确认后下载和安装；Linux 使用系统授权和对应包管理器，macOS 由内置助手在应用退出后替换应用包、无需系统授权。安装后继续使用原配置目录，重启程序，SSH 会话断开。
- 旧 Arch 版须先手动安装 R28 或更新版；不添加 pacman 软件仓库。GitHub 发布不会自动部署官网。
