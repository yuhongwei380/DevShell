# DevShell 原生前端

`native-ui` 是 DevShell 的 GPUI 原生窗口，使用 GPUI Kit 作为主界面组件层，使用 ELY GPUI Components 提供焦点范围、标题/说明和轻量交互控件。它只连接现有 Go 本地 API，不复制配置、SSH、SFTP 或监控业务逻辑。

## 本地运行

先启动 Go API：

```powershell
go run . --native
```

开发时如果前端二进制不在 Go 程序旁边，可显式指定：

```powershell
$env:DEVSHELL_NATIVE_UI = "native-ui\target\debug\devshell-native-ui.exe"
go run . --native
```

`--native` 会先启动 Go 本地服务，再把回环地址和一次性 token 传给原生窗口。窗口退出后 Go 服务随进程退出，不会启动 WebView2。

Windows 和 macOS 的发行脚本会把这个前端编译为 `DevShellUI.exe` 或 `.app/Contents/MacOS/DevShellUI`，并与 Go 后端一起打包。旧安装在尚未带有该文件时仍可走兼容的 Wails 启动路径，便于通过旧版内置更新平滑过渡；新安装默认使用 GPUI 窗口。

## 迁移边界

当前切片已经包含原生窗口、后端握手、服务器/分组摘要、服务器新增、直连密码 SSH 编辑与回收、服务器连接入口、SSH 终端 WebSocket 输入输出、主机指纹确认、SFTP 根目录浏览与文件操作、监控首屏、传输队列、release 更新检查以及配置存储和安全锁状态概览。刷新按钮会重新读取本地 API 状态；设置面板只展示后端信息，不直接复制配置写入逻辑。私钥、Agent、代理配置、更新下载/安装和其余设置页面仍按现有 API 域逐步迁移，所有状态继续由 Go API 管理，避免原生前端和旧页面产生两份配置模型。
