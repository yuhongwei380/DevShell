# DevShell GPUI 前端

还原基准固定为 `v26.11.3-rc.3` 的 `web/`，提交 `eb3e173398904883985dff8a86beca79f19c7119`。只更换窗口和绘制技术；配置、SSH、SFTP、密钥、安全锁和更新继续使用原有 Go API。当前尚未完成全部视觉与功能等价，不能作为“100% 还原”验收。

## 构建与运行

```powershell
cargo build --manifest-path native-ui/Cargo.toml --release
$env:DEVSHELL_NATIVE_UI = "native-ui\target\release\devshell-native-ui.exe"
go run .
```

交付目录中将 Rust 程序命名为 `DevShellUI.exe`，与 Go 的 `DevShell.exe` 放在一起。启动时 Go 通过回环地址和一次性 token 拉起原生窗口；窗口退出后服务退出。桌面路径使用 GPUI，不嵌入 WebView。

## 已接入的实现

- 原 SVG 图标、内置字体与 ANSI 色板；UI 缩放、浅色和深色。
- 原版 700px 双栏服务器管理器，320px 分组树、搜索、多选、分组层级、连接与回收站操作、备份和恢复。
- 800px 连接窗口，协议/凭据与分组/代理/备注/自动命令分栏。窗口内新建分组后保留连接草稿；取消或 Escape 返回上一层。
- 设置分层菜单、主题卡片、可拖动字体面板、字体预览与字号、加粗、颜色、导入、改名、删除、许可；曲线颜色、粗细、线型与默认值恢复。
- Ely 原生终端单元格、Alacritty VT 状态、原始字节按键/粘贴/鼠标报告、选择、滚动、IME 与搜索；输出解析后才确认背压。认证 OSC 777 控制命令历史、远程目录跟随和自动命令时机。
- 每个会话独立保留终端、目录、编辑草稿与传输；SSH 主机指纹确认、凭据输入、本地终端、TCP 工具及 Windows RDP 宿主桥接。
- 安全锁授权、密码、TOTP/二维码、闲置锁定、解锁；密钥导入/生成、复制公钥、编辑与删除。
- 原有 SFTP、命令、utilities、字体库和更新 API 的原生入口。接入 API 不代表该页面已经完成原版布局与交互验收。
- SFTP 独立目录树与表格滚动、Ctrl/Shift 多选、右键及键盘菜单、新建/重命名/删除/权限、常用目录/历史路径、符号链接、文件与目录拖放上传、同名预检、跳过或明确覆盖；下载采用流式读取并在成功后原子替换目标文件。
- 原版网络历史与独立 ICMP 采样，网卡选择、共同流量坐标轴、五秒缩轴迟滞、缺失帧断点、单调三次曲线、超时标记、悬停和键盘采样检查；监控样式实时应用。
- 配色方案采用原版列表，支持批量 iTerm 导入、同名覆盖、应用、行内重命名和四秒内两步删除。保持 64 套/256 KiB 限制，保留前景、光标和选区键，忽略背景键。

## 尚未达到原版的部分

| 范围 | 剩余差距 |
| --- | --- |
| 材质 | 原版模态背景模糊和液态玻璃折射/动态光场尚未实现；仅保存主题选项不构成材质还原。 |
| 监控和 SFTP | MTR、完整进程列表、文件编辑器及目录收藏/历史弹窗仍需逐项恢复原版布局与交互；网络采样和文件操作尚需真实 SSH 验收。 |
| 其他对话框 | 安全锁、密钥、字体颜色/在线字体库等仍有通用表单或简化布局；配色方案的批量导入与管理已移植。 |
| 会话和工具 | 标签分离窗口、常用应用全部高级选项等尚未等价；分组排序尚未完整复现原版中文拼音排序。 |
| 实机交互 | 真实 SSH/SFTP、RDP/串口、本地终端全流程和 macOS 实机尚未完整验证。 |

## 验证

```powershell
cargo test --manifest-path native-ui/Cargo.toml --release
go test ./internal/app -run 'TestNativeTerminalInputPreservesVTBytes|TestNativeRDPHost' -count=1 -parallel=1
cargo build --manifest-path native-ui/Cargo.toml --release --features visual-test
pwsh -NoProfile -File scripts/tests/capture-native-scenes.ps1
```

视觉工具需要先导出固定 tag 的 `web/` 到 `build/ui-qa/baseline-v26.11.3-rc.3`，再用 `go run ./scripts/tests/native-reference-fixture` 启动隔离的 Go fixture；不得指向生产会话。`capture-native-reference.mjs` 以固定版本的真实 HTML/CSS/JS 生成原版截图及 DOM 样式/尺寸。`capture-native-scenes.ps1` 使用 GPUI 自己的场景绘制接口生成 1280×800 截图，不读取其他应用窗口，不需要桌面自动化服务。覆盖主界面、服务器管理、连接、设置菜单、主题、曲线、字体、监控、文件面板及其菜单/操作弹窗、配色方案，共 46 个浅色/深色状态，并实际分派菜单按键与文件导航回归。截图是对照证据，不是像素一致性断言。

编译输出、本地预览、临时配置、参考库导出、浏览器依赖、截图及日志都留在已忽略的 `native-ui/target/`、`build/local-*/` 和 `build/ui-qa/`，不提交 Git。`assets/` 中的字体、图标、许可和确定性 JSON 测试输入是源码所需资源。

`visual-test` 只用于场景导出；提供给用户的普通二进制应不带此 feature。当前验证：Rust 39 项通过，46 个场景及菜单/导航回归通过，原始终端输入及 RDP 宿主授权的 Go 定向测试通过。Go 全套测试曾出现 Windows 临时目录清理失败，不能写成全套通过。

没有修改 ApplicationBuild、打 tag 或发布。任何发版仍须用户单独明确确认。
