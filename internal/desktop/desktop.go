//go:build desktop

package desktop

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"log"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	goruntime "runtime"
	"strings"
	"sync"
	"time"

	"devshell/internal/app"

	"github.com/gorilla/websocket"
	"github.com/wailsapp/wails/v2"
	"github.com/wailsapp/wails/v2/pkg/options"
	"github.com/wailsapp/wails/v2/pkg/options/assetserver"
	"github.com/wailsapp/wails/v2/pkg/options/linux"
	"github.com/wailsapp/wails/v2/pkg/options/mac"
	"github.com/wailsapp/wails/v2/pkg/options/windows"
	"github.com/wailsapp/wails/v2/pkg/runtime"
)

type Desktop struct {
	ctx                   context.Context
	app                   desktopBackend
	mu                    sync.Mutex
	terminals             map[string]*nativeTerminal
	quitConfirmed         bool
	quitPending           bool
	installerQuitPending  bool
	installerQuitApproved bool
	minimizePending       bool
	minimizeHandled       bool
	trayHidden            bool
	updateInstalling      bool
	lifecycleCancel       context.CancelFunc
	detachedNonce         string
	children              map[string]*exec.Cmd
	pendingWindows        map[string]bool
	pickerBusy            bool
	lastDirs              map[string]string
	windowClosed          bool
	filesPanelMu          sync.Mutex
	filesPanelOpen        bool
	filesPanelGrowth      int
}

type nativeTerminal struct {
	socket  *websocket.Conn
	handoff string
	mu      sync.Mutex
}

type DesktopWindowState struct {
	Platform             string `json:"platform"`
	Frameless            bool   `json:"frameless"`
	Maximised            bool   `json:"maximised"`
	QuitPending          bool   `json:"quitPending"`
	InstallerQuitPending bool   `json:"installerQuitPending"`
}

func (d *Desktop) CaptureWindowState() error {
	if d.detachedNonce != "" || d.ctx == nil || runtime.WindowIsMinimised(d.ctx) {
		return nil
	}
	previous := d.app.Appearance()
	width, height := previous.WindowWidth, previous.WindowHeight
	maximised := runtime.WindowIsMaximised(d.ctx)
	if !maximised && !runtime.WindowIsFullscreen(d.ctx) {
		width, height = runtime.WindowGetSize(d.ctx)
		d.filesPanelMu.Lock()
		width -= d.filesPanelGrowth
		d.filesPanelMu.Unlock()
	}
	if width <= 0 || height <= 0 {
		return nil
	}
	if width == previous.WindowWidth && height == previous.WindowHeight && maximised == previous.WindowMaximised {
		return nil
	}
	return d.app.SaveWindowState(width, height, maximised)
}

func (d *Desktop) ConfirmQuit() error {
	if err := d.CaptureWindowState(); err != nil {
		return err
	}
	d.mu.Lock()
	if len(d.pendingWindows) > 0 {
		d.quitPending = false
		d.mu.Unlock()
		return errors.New("终端正在交接，请稍后再关闭窗口")
	}
	if d.detachedNonce == "" && len(d.children) > 0 {
		if d.installerQuitPending {
			// Keep the backend alive until independent windows have resolved
			// their own dirty editors and exited. The lifecycle loop finishes.
			d.installerQuitApproved = true
			d.quitPending = false
			d.mu.Unlock()
			return nil
		}
		d.windowClosed = true
		d.quitPending = false
		d.mu.Unlock()
		runtime.WindowHide(d.ctx)
		return nil
	}
	d.quitConfirmed = true
	d.quitPending = false
	d.mu.Unlock()
	runtime.Quit(d.ctx)
	return nil
}
func (d *Desktop) CancelQuit() {
	d.mu.Lock()
	d.quitPending = false
	d.installerQuitPending = false
	d.installerQuitApproved = false
	d.mu.Unlock()
}

func (d *Desktop) requestInstallerQuit() {
	d.mu.Lock()
	if d.quitConfirmed || d.installerQuitApproved {
		d.mu.Unlock()
		return
	}
	d.installerQuitPending = true
	d.quitPending = true
	d.mu.Unlock()
	d.RestoreWindow()
	runtime.EventsEmit(d.ctx, "dengshell:confirm-quit", map[string]bool{"installer": true})
}
func (d *Desktop) beforeClose(ctx context.Context) bool {
	d.mu.Lock()
	if d.quitConfirmed {
		d.mu.Unlock()
		return false
	}
	d.quitPending = true
	installer := d.installerQuitPending
	d.mu.Unlock()
	d.RestoreWindow()
	// The frontend de-duplicates visible confirmations. Re-emit on another
	// close request so a lost event or dismissed cover cannot strand the user.
	runtime.EventsEmit(ctx, "dengshell:confirm-quit", map[string]bool{"installer": installer})
	return true
}

func (d *Desktop) WindowState() DesktopWindowState {
	state := DesktopWindowState{Platform: goruntime.GOOS, Frameless: goruntime.GOOS == "windows"}
	d.mu.Lock()
	state.QuitPending = d.quitPending
	state.InstallerQuitPending = d.installerQuitPending
	d.mu.Unlock()
	if d.ctx != nil {
		state.Maximised = runtime.WindowIsMaximised(d.ctx)
	}
	return state
}

func (d *Desktop) WindowAction(action string) (DesktopWindowState, error) {
	if d.ctx == nil {
		return d.WindowState(), errors.New("窗口尚未准备好")
	}
	switch action {
	case "minimise":
		d.requestMinimize()
	case "toggle-maximise":
		runtime.WindowToggleMaximise(d.ctx)
	case "close":
		state := d.WindowState()
		runtime.Quit(d.ctx)
		return state, nil
	default:
		return d.WindowState(), errors.New("未知窗口操作")
	}
	return d.WindowState(), nil
}

func (d *Desktop) OpenTerminal(sessionID string) error {
	nonce := ""
	if remote, ok := d.app.(*remoteDesktopBackend); ok && remote.launch.SessionID == sessionID {
		nonce = d.detachedNonce
	}
	return d.OpenTerminalWithHandoff(sessionID, nonce)
}

// OpenSerialTerminal bridges the serial terminal WebSocket through the same
// native IPC path as SSH terminals. Serial sessions never participate in
// window handoff, so there is no nonce to validate.
func (d *Desktop) OpenSerialTerminal(sessionID string) error {
	return d.openTerminalSocketPath(sessionID, "/api/serial/sessions/"+url.PathEscape(sessionID)+"/terminal", "")
}
func (d *Desktop) openTerminalSocket(sessionID, nonce string) error {
	return d.openTerminalSocketPath(sessionID, "/api/sessions/"+url.PathEscape(sessionID)+"/terminal", nonce)
}
func (d *Desktop) openTerminalSocketPath(sessionID, path, nonce string) error {
	address := strings.Replace(d.app.URL(), "http:", "ws:", 1) + path + "?token=" + url.QueryEscape(d.app.Token())
	if nonce != "" {
		address += "&handoff=" + url.QueryEscape(nonce)
	}
	socket, _, err := websocket.DefaultDialer.DialContext(d.ctx, address, nil)
	if err != nil {
		return err
	}
	terminal := &nativeTerminal{socket: socket, handoff: nonce}
	d.mu.Lock()
	previous := d.terminals[sessionID]
	d.terminals[sessionID] = terminal
	d.mu.Unlock()
	// Do not let a stale reader from an earlier residence close or emit EOF into
	// a newly returned tab with the same session id.
	if previous != nil {
		previous.socket.Close()
	}
	go func() {
		defer func() {
			socket.Close()
			d.mu.Lock()
			current := d.terminals[sessionID] == terminal
			if current {
				delete(d.terminals, sessionID)
			}
			d.mu.Unlock()
			if current {
				runtime.EventsEmit(d.ctx, "cloudshell:terminal:"+sessionID, map[string]any{"kind": 0, "handoff": nonce})
			}
		}()
		for {
			kind, data, err := socket.ReadMessage()
			if err != nil {
				return
			}
			d.mu.Lock()
			current := d.terminals[sessionID] == terminal
			d.mu.Unlock()
			if !current {
				return
			}
			runtime.EventsEmit(d.ctx, "cloudshell:terminal:"+sessionID, map[string]any{"kind": kind, "data": base64.StdEncoding.EncodeToString(data), "handoff": nonce})
		}
	}()
	return nil
}
func (d *Desktop) SendTerminal(sessionID, data string) error {
	d.mu.Lock()
	terminal := d.terminals[sessionID]
	d.mu.Unlock()
	if terminal == nil {
		return errors.New("终端已关闭")
	}
	terminal.mu.Lock()
	defer terminal.mu.Unlock()
	terminal.socket.SetWriteDeadline(time.Now().Add(15 * time.Second))
	return terminal.socket.WriteMessage(websocket.TextMessage, []byte(data))
}
func (d *Desktop) CloseTerminal(sessionID string) {
	d.mu.Lock()
	terminal := d.terminals[sessionID]
	delete(d.terminals, sessionID)
	d.mu.Unlock()
	if terminal != nil {
		terminal.socket.Close()
	}
}

// JSON control calls use the desktop bridge; file bytes stay in Go/SFTP.
func (d *Desktop) Request(method, path, body string) (string, error) {
	if !strings.HasPrefix(path, "/api/") || strings.ContainsAny(path, "\r\n") {
		return "", errors.New("无效请求路径")
	}
	req, err := http.NewRequestWithContext(d.ctx, method, d.app.URL()+path, strings.NewReader(body))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-CloudShell-Token", d.app.Token())
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		return "", err
	}
	defer res.Body.Close()
	data, err := io.ReadAll(io.LimitReader(res.Body, (96<<20)+1))
	if err != nil {
		return "", err
	}
	if len(data) > 96<<20 {
		return "", errors.New("响应内容过大")
	}
	if res.StatusCode >= 400 {
		var response struct {
			Error string `json:"error"`
		}
		json.Unmarshal(data, &response)
		if response.Error != "" {
			return "", errors.New("DENGSHELL_ERROR:" + string(data))
		}
		return "", fmt.Errorf("请求失败 (%d)", res.StatusCode)
	}
	return string(data), nil
}
func Available() bool { return true }
func (d *Desktop) ReadClipboard() (string, error) {
	if err := d.app.RequireUnlocked(); err != nil {
		return "", err
	}
	return runtime.ClipboardGetText(d.ctx)
}
func (d *Desktop) WriteClipboard(text string) error {
	if err := d.app.RequireUnlocked(); err != nil {
		return err
	}
	return runtime.ClipboardSetText(d.ctx, text)
}

// SystemTheme reads the operating-system preference independently of this
// window's current GTK/WebView appearance. An empty result asks the renderer
// to use its native prefers-color-scheme fallback.
func (d *Desktop) SystemTheme() string { return platformSystemTheme() }

func (d *Desktop) SetTheme(theme string) {
	if d.ctx == nil {
		return
	}
	if theme == "system" {
		setPlatformSystemTheme()
		runtime.WindowSetSystemDefaultTheme(d.ctx)
		if platformSystemTheme() == "dark" {
			runtime.WindowSetBackgroundColour(d.ctx, 15, 20, 25, 255)
		} else {
			runtime.WindowSetBackgroundColour(d.ctx, 240, 243, 247, 255)
		}
		return
	}
	switch theme {
	case "dark":
		setPlatformTheme(true)
		runtime.WindowSetDarkTheme(d.ctx)
		runtime.WindowSetBackgroundColour(d.ctx, 15, 20, 25, 255)
	case "light":
		setPlatformTheme(false)
		runtime.WindowSetLightTheme(d.ctx)
		runtime.WindowSetBackgroundColour(d.ctx, 240, 243, 247, 255)
	}
}
func (d *Desktop) ChooseUploads() ([]string, error) {
	if err := d.app.RequireUnlocked(); err != nil {
		return nil, err
	}

	return d.openDialogMulti("upload", func(directory string) ([]string, error) {
		return platformOpenMultipleFilesDialog(d.ctx, runtime.OpenDialogOptions{Title: "选择上传文件", DefaultDirectory: directory})
	})
}
func (d *Desktop) ChooseFolder() (string, error) {
	if err := d.app.RequireUnlocked(); err != nil {
		return "", err
	}

	return d.openFolderDialog("upload-folder", func(directory string) (string, error) {
		return platformOpenDirectoryDialog(d.ctx, runtime.OpenDialogOptions{Title: "选择上传文件夹", DefaultDirectory: directory})
	})
}
func (d *Desktop) ChooseKey() (string, error) {
	if err := d.app.RequireUnlocked(); err != nil {
		return "", err
	}

	return d.openDialog("key", func(directory string) (string, error) {
		return platformOpenFileDialog(d.ctx, runtime.OpenDialogOptions{Title: "选择 SSH 私钥", ShowHiddenFiles: true, DefaultDirectory: directory})
	})
}
func (d *Desktop) ChooseAsset(kind string) (*app.ManagedAsset, error) {
	if err := d.app.RequireUnlocked(); err != nil {
		return nil, err
	}

	var title, label, pattern string
	switch kind {
	case "font":
		title, label, pattern = "添加终端字体", "字体文件", "*.ttf;*.otf;*.woff;*.woff2"
	case "ui-font":
		title, label, pattern = "导入界面字体（重启后生效）", "字体文件", "*.ttf;*.otf;*.woff;*.woff2"
	case "background":
		title, label, pattern = "添加终端背景", "背景图片", "*.png;*.jpg;*.jpeg;*.webp"
	default:
		return nil, errors.New("未知资源类型")
	}
	filename, err := d.openDialog("asset-"+kind, func(directory string) (string, error) {
		return platformOpenFileDialog(d.ctx, runtime.OpenDialogOptions{
			Title:            title,
			Filters:          []runtime.FileFilter{{DisplayName: label, Pattern: pattern}},
			DefaultDirectory: directory,
		})
	})
	if err != nil || filename == "" {
		return nil, err
	}
	if err := d.app.RequireUnlocked(); err != nil {
		return nil, err
	}
	asset, err := d.app.ImportLocalAsset(kind, filename, "")
	if err != nil {
		return nil, err
	}
	return &asset, nil
}
func (d *Desktop) Download(sessionID, remote string) (string, error) {
	if err := d.app.RequireUnlocked(); err != nil {
		return "", err
	}

	destination, err := d.openDialog("download", func(directory string) (string, error) {
		return platformSaveFileDialog(d.ctx, runtime.SaveDialogOptions{Title: "下载文件", DefaultFilename: path.Base(remote), DefaultDirectory: directory})
	})
	if err != nil || destination == "" {
		return "", err
	}
	if err = d.app.RequireUnlocked(); err != nil {
		return "", err
	}
	if err = d.app.DownloadTo(sessionID, remote, destination); err != nil {
		return "", err
	}
	return destination, nil
}
func Run(application *app.App, assets fs.FS, configDir string, icon []byte) error {
	return runDesktopBackend(application, assets, configDir, "", icon)
}
func runDesktopBackend(application desktopBackend, assets fs.FS, configDir, detachedNonce string, icon []byte) error {
	// GTK initialization and Wails' native main loop must share one OS thread.
	goruntime.LockOSThread()
	defer goruntime.UnlockOSThread()
	preparePlatformWindow()
	initial, err := initialPlatformWindow()
	if err != nil {
		return err
	}
	preferences := application.Appearance()
	// A detached terminal opens as a movable window even if the source is maximised.
	if detachedNonce != "" {
		preferences.WindowMaximised = false
	}
	initial = restoreSavedWindowBounds(initial, preferences.WindowWidth, preferences.WindowHeight)
	log.Printf("DevShell initial window: %dx%d logical pixels, position %d,%d", initial.Width, initial.Height, initial.X, initial.Y)
	previouslyReady := runtimePreviouslyReady(configDir)
	var readyOnce, windowOnce sync.Once
	desktop := &Desktop{app: application, terminals: map[string]*nativeTerminal{}, detachedNonce: detachedNonce, children: map[string]*exec.Cmd{}, lastDirs: map[string]string{}}
	if primary, ok := application.(*app.App); ok {
		primary.SetWindowLauncher(desktop.launchDetached)
		defer primary.SetWindowLauncher(nil)
		go sweepDetachedWebviewCaches(configDir)
	}
	nativeHandler := nativeAssetHandler(application, assets)
	return wails.Run(&options.App{
		Title: "DevShell", Width: initial.Width, Height: initial.Height, MinWidth: initial.MinWidth, MinHeight: initial.MinHeight,
		Frameless:        goruntime.GOOS == "windows",
		StartHidden:      goruntime.GOOS == "windows",
		BackgroundColour: options.NewRGB(240, 243, 247),
		AssetServer: &assetserver.Options{Assets: assets, Handler: nativeHandler, Middleware: func(next http.Handler) http.Handler {
			return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Cache-Control", "no-store")
				next.ServeHTTP(w, r)
			})
		}},
		Bind:          []interface{}{desktop},
		OnBeforeClose: desktop.beforeClose,
		OnShutdown: func(context.Context) {
			desktop.mu.Lock()
			cancel := desktop.lifecycleCancel
			desktop.mu.Unlock()
			if cancel != nil {
				cancel()
			}
			platformCloseTray()
		},
		SingleInstanceLock: &options.SingleInstanceLock{UniqueId: detachedInstanceID(configDir, detachedNonce), OnSecondInstanceLaunch: func(options.SecondInstanceData) {
			if desktop.ctx != nil {
				desktop.RestoreWindow()
			}
		}},
		OnStartup: func(ctx context.Context) {
			desktop.ctx = ctx
			desktop.watchParent(ctx)
			runtime.EventsOn(ctx, "cloudshell:ready", func(...interface{}) {
				readyOnce.Do(func() {
					if !previouslyReady {
						log.Printf("DevShell first successful runtime check: %s", platformRuntimeDescription())
						if err := recordRuntimeReady(configDir); err != nil {
							log.Printf("DevShell could not cache runtime check: %v", err)
						}
					}
					log.Print("DevShell frontend and local API ready")
				})
			})
			runtime.OnFileDrop(ctx, func(x, y int, paths []string) {
				if application.RequireUnlocked() != nil {
					return
				}
				payload, _ := json.Marshal(map[string]any{"x": x, "y": y, "paths": paths})
				runtime.WindowExecJS(ctx, "window.cloudshellNativeDrop && window.cloudshellNativeDrop("+string(payload)+")")
			})
		},
		OnDomReady: func(ctx context.Context) {
			windowOnce.Do(func() {
				installPlatformWindowIcon()
				finalizeInitialPlatformWindow(ctx, initial)
				// Only the accepted primary window can persist startup dimensions;
				// a second-instance launch must not change the active store on disk.
				if detachedNonce == "" && (initial.Width != preferences.WindowWidth || initial.Height != preferences.WindowHeight) {
					if err := application.SaveWindowState(initial.Width, initial.Height, preferences.WindowMaximised); err != nil {
						log.Printf("DevShell window size could not be saved: %v", err)
					}
				}
				if preferences.WindowMaximised {
					runtime.WindowMaximise(ctx)
				}
				iconPath := filepath.Join(configDir, "tray-icon.png")
				if err := os.WriteFile(iconPath, icon, 0600); err == nil {
					desktop.startDesktopLifecycle(ctx, iconPath)
				} else {
					desktop.startDesktopLifecycle(ctx, "")
				}
				log.Print("DevShell desktop window ready")
			})
		},
		DragAndDrop: &options.DragAndDrop{EnableFileDrop: true, DisableWebViewDrop: false},
		Linux:       &linux.Options{ProgramName: "dengshell", Icon: icon, WebviewGpuPolicy: linux.WebviewGpuPolicyOnDemand},
		Mac: &mac.Options{
			DisableZoom: true, DisableEscapeExitsFullscreen: true,
			About: &mac.AboutInfo{Title: "DevShell", Message: app.ApplicationDisplayVersion + " · SSH 终端与服务器管理", Icon: icon},
		},
		Windows: &windows.Options{
			WindowClassName: "DevShellWindow", DisableWindowIcon: false,
			IsZoomControlEnabled: false, DisablePinchZoom: true,
			WebviewUserDataPath: desktopWebviewPath(configDir, detachedNonce),
			Messages: &windows.Messages{
				InstallationRequired: "DevShell 需要 Microsoft Edge WebView2 运行时才能显示界面。\n点击“确定”从 Microsoft 官方站点下载安装；已有终端配置不会被修改。\n下载需要联网，请等待安装窗口出现。离线电脑可先安装 WebView2 Evergreen 独立安装包。",
				UpdateRequired:       "DevShell 需要更新 Microsoft Edge WebView2 运行时。\n点击“确定”下载安装官方更新，需要联网。",
				MissingRequirements:  "DevShell · 运行环境检查",
				Webview2NotInstalled: "未安装 WebView2，DevShell 尚未启动。安装 Microsoft Edge WebView2 Evergreen 运行时后重试。",
				Error:                "DevShell · 运行环境错误",
				FailedToInstall:      "WebView2 未能安装成功。请从 https://developer.microsoft.com/microsoft-edge/webview2 下载 Evergreen 运行时，安装后重新启动 DevShell。",
				DownloadPage:         "DevShell 需要 Microsoft Edge WebView2 运行时。点击“确定”打开 Microsoft 官方下载页面。最低版本：",
				PressOKToInstall:     "点击“确定”安装 Microsoft Edge WebView2 运行时。",
				ContactAdmin:         "DevShell 需要 Microsoft Edge WebView2 运行时，请安装 Evergreen 运行时后重试。",
				InvalidFixedWebview2: "指定的 WebView2 运行时不可用，请检查运行时目录及版本。",
				WebView2ProcessCrash: "WebView2 进程已退出，请重新启动 DevShell。若反复出现，请修复 Microsoft Edge WebView2 运行时。",
			},
		},
	})
}
