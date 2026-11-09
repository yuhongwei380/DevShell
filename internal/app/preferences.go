package app

import (
	"encoding/json"
	"errors"
	"math"
	"net/http"
	"strconv"
	"strings"
	"unicode"
)

// Appearance is stored alongside connection settings, so native and browser
// windows use the same selection and custom assets survive application upgrades.
type Appearance struct {
	UIFontID         string                    `json:"uiFontId"`
	UITextColors     map[string]string         `json:"uiTextColors"`
	FontID           string                    `json:"fontId"`
	FontColors       map[string]string         `json:"fontColors,omitempty"`
	FontBold         map[string]bool           `json:"fontBold"`
	ChartStyles      map[string]ChartLineStyle `json:"chartStyles"`
	UIScale          float64                   `json:"uiScale"`
	TerminalFontSize float64                   `json:"terminalFontSize"`
	TerminalBold     bool                      `json:"terminalBold,omitempty"` // Legacy input only; migrated to the selected font.
	// UIStyle selects the global workbench style drawn in CSS (chrome,
	// panels, dialogs and accents — not just a background picture). Each
	// style paints its own light and dark variant and stays orthogonal to
	// Theme, fonts, ANSI colours and layout.
	UIStyle     string `json:"uiStyle"` // classic (default) | frosted
	Theme       string `json:"theme"`
	AnsiPalette string `json:"ansiPalette,omitempty"` // soft (default) | vivid | bright | custom
	// AnsiSchemes holds the named ANSI colour schemes imported from iTerm2
	// Color Schemes (.itermcolors), managed under 「设置 → 外观配置 →
	// 配色方案」. Each scheme carries the 16 standard ANSI colours and the
	// name taken from the imported file. AnsiScheme names the active one
	// while AnsiPalette is custom; presets keep the list stored but inert.
	AnsiSchemes         []AnsiScheme `json:"ansiSchemes,omitempty"`
	AnsiScheme          string       `json:"ansiScheme,omitempty"`
	WindowWidth         int          `json:"windowWidth"`
	WindowHeight        int          `json:"windowHeight"`
	WindowMaximised     bool         `json:"windowMaximised"`
	OnboardingCompleted bool         `json:"onboardingCompleted"`
	MonitorVisible      bool         `json:"monitorVisible"`
	FilesPosition       string       `json:"filesPosition"`
	FollowTerminal      bool         `json:"followTerminal"`
	PromptUsernameColor string       `json:"promptUsernameColor"`
	PromptHostnameColor string       `json:"promptHostnameColor"`
	MinimizeAction      string       `json:"minimizeAction"`
	ExternalEditor      string       `json:"externalEditor"`
	// UpdateMirror is the optional GitHub accelerator origin (ghproxy-style
	// URL prefix) used for update manifest and artifact fetches. Empty means
	// direct connections; signature, build and SHA-256 checks still apply.
	UpdateMirror string `json:"updateMirror,omitempty"`
	// AutoCommandsEnabled gates the connect-time auto command feature (issue
	// #5) for every profile. The per-profile autoRun flag only takes effect
	// while this global switch stays on; it lives under Settings → Other
	// features. Default on: profiles without commands behave as before.
	AutoCommandsEnabled bool `json:"autoCommandsEnabled"`
	// LocalShellEnabled gates the local shell feature (issue #4 slice A):
	// when on, the session tab “+” opens a local terminal with the user's
	// login shell at home instead of the start-tab page. macOS only — the
	// feature was removed on Windows, where enabling it is rejected. Lives
	// under Settings → Other features. Default off on supported platforms.
	LocalShellEnabled bool `json:"localShellEnabled"`
	// TcpToolsEnabled gates the candidate-bar TCP tools: the tcping probe and
	// telnet terminal sessions (both implemented in-app, Windows and macOS).
	// Lives under Settings → Other features. Default on: the commands extend
	// the existing local-prompt command set rather than changing behaviour.
	TcpToolsEnabled bool                       `json:"tcpToolsEnabled"`
	Layout          map[string]json.RawMessage `json:"layout"`
}

// AnsiScheme is one saved iTerm2-derived ANSI colour scheme: the 16 standard
// ANSI colours keyed black…brightWhite, under a name taken from the imported
// .itermcolors file. Applying a scheme sets AnsiPalette to custom and points
// AnsiScheme at it; the list itself never changes the live appearance.
type AnsiScheme struct {
	Name   string            `json:"name"`
	Colors map[string]string `json:"colors"`
}

func defaultAppearance() Appearance {
	return Appearance{UIFontID: defaultUIFontID, UITextColors: map[string]string{}, FontID: "builtin:jetbrains-mono", FontColors: map[string]string{}, FontBold: map[string]bool{}, ChartStyles: map[string]ChartLineStyle{}, UIScale: 1, TerminalFontSize: 14, UIStyle: "classic", Theme: "system", MonitorVisible: true, FilesPosition: "right", FollowTerminal: true, MinimizeAction: "ask", AutoCommandsEnabled: true, TcpToolsEnabled: true, Layout: map[string]json.RawMessage{}}
}

// retiredUIStyle names the polar/twilight workbench styles removed in
// November 2026; saved values migrate to classic on load.
func retiredUIStyle(value string) bool { return value == "polar" || value == "twilight" }

func (value *Appearance) UnmarshalJSON(data []byte) error {
	type plain Appearance
	next := plain(defaultAppearance())
	if err := json.Unmarshal(data, &next); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	if next.UIFontID == "" || retiredBuiltinUIFont(next.UIFontID) {
		next.UIFontID = defaultUIFontID
	}
	// polar 与 twilight 已从全局主题中移除：旧配置与旧主题快照里的选择
	// 在加载时迁回 classic，否则之后的任何外观保存都会被枚举校验拒绝。
	if retiredUIStyle(next.UIStyle) {
		next.UIStyle = "classic"
	}
	if strings.HasPrefix(next.FontID, "builtin:") && !builtinTerminalFont(next.FontID) {
		next.FontID = "builtin:jetbrains-mono"
	}
	if next.UITextColors == nil {
		next.UITextColors = map[string]string{}
	}
	if next.FontColors == nil {
		next.FontColors = map[string]string{}
	}
	if next.FontBold == nil {
		next.FontBold = map[string]bool{}
	}
	if next.ChartStyles == nil {
		next.ChartStyles = map[string]ChartLineStyle{}
	}
	if next.Layout == nil {
		next.Layout = map[string]json.RawMessage{}
	}
	if _, perFont := fields["fontBold"]; !perFont && next.TerminalBold {
		next.FontBold[next.FontID] = true
	}
	next.TerminalBold = false
	*value = Appearance(next)
	return nil
}

func cloneAppearance(value Appearance) Appearance {
	uiColors := make(map[string]string, len(value.UITextColors))
	for key, color := range value.UITextColors {
		uiColors[key] = color
	}
	value.UITextColors = uiColors
	styles := make(map[string]ChartLineStyle, len(value.ChartStyles))
	for id, style := range value.ChartStyles {
		styles[id] = style
	}
	value.ChartStyles = styles
	colors := make(map[string]string, len(value.FontColors))
	for id, color := range value.FontColors {
		colors[id] = color
	}
	value.FontColors = colors
	bold := make(map[string]bool, len(value.FontBold))
	for id, enabled := range value.FontBold {
		bold[id] = enabled
	}
	value.FontBold = bold
	if value.AnsiSchemes != nil {
		schemes := make([]AnsiScheme, len(value.AnsiSchemes))
		for index, scheme := range value.AnsiSchemes {
			schemes[index] = cloneAnsiScheme(scheme)
		}
		value.AnsiSchemes = schemes
	}
	layout := make(map[string]json.RawMessage, len(value.Layout))
	for key, item := range value.Layout {
		layout[key] = append(json.RawMessage(nil), item...)
	}
	value.Layout = layout
	return value
}

func cloneAnsiScheme(scheme AnsiScheme) AnsiScheme {
	if scheme.Colors != nil {
		colors := make(map[string]string, len(scheme.Colors))
		for key, color := range scheme.Colors {
			colors[key] = color
		}
		scheme.Colors = colors
	}
	return scheme
}

// printableToken keeps stored ids and map keys on one printable line so
// portable config diffs stay readable and injection into prompts is moot.
func printableToken(value string, limit int) bool {
	return value != "" && len(value) <= limit && !strings.ContainsFunc(value, unicode.IsControl)
}

// normalizeAnsiSchemes validates the saved ANSI colour scheme list in place:
// unique names and a complete 16-key palette per scheme. Colours are
// lowercased; everything else is stored verbatim so a scheme imported from a
// .itermcolors file round-trips unchanged.
func normalizeAnsiSchemes(schemes []AnsiScheme) error {
	seenNames := make(map[string]struct{}, len(schemes))
	for index := range schemes {
		scheme := &schemes[index]
		scheme.Name = strings.TrimSpace(scheme.Name)
		if scheme.Name == "" || len([]rune(scheme.Name)) > 48 || strings.ContainsFunc(scheme.Name, unicode.IsControl) {
			return errors.New("配色方案名称应为 1–48 个字符，且不能包含换行")
		}
		if _, duplicate := seenNames[scheme.Name]; duplicate {
			return errors.New("配色方案名称重复")
		}
		seenNames[scheme.Name] = struct{}{}
		if err := normalizeAnsiColors(scheme.Colors); err != nil {
			return errors.New("配色方案「" + scheme.Name + "」" + err.Error())
		}
	}
	return nil
}

func validTerminalColor(value string) bool {
	if len(value) != 7 || value[0] != '#' {
		return false
	}
	_, err := strconv.ParseUint(value[1:], 16, 24)
	return err == nil
}

// ansiColorKeys lists the 16 standard ANSI colour slots xterm applies in
// order (normal 0–7, bright 8–15). A custom palette must cover all of them.
var ansiColorKeys = []string{
	"black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
	"brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
}

// normalizeAnsiColors validates a custom ANSI palette in place and lowercases
// the hex values. Empty maps are the caller's business: they normalize to nil
// before this runs, and a custom palette without one fails with the same
// missing-key error.
func normalizeAnsiColors(colors map[string]string) error {
	if len(colors) != len(ansiColorKeys) {
		return errors.New("自定义 ANSI 颜色必须包含全部 16 个标准色键")
	}
	for _, key := range ansiColorKeys {
		color, ok := colors[key]
		if !ok || !validTerminalColor(color) {
			return errors.New("自定义 ANSI 颜色必须包含全部 16 个标准色键，且取值应为 #RRGGBB 格式")
		}
		colors[key] = strings.ToLower(color)
	}
	return nil
}

func (s *Store) SaveAppearance(value Appearance) (Appearance, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.saveAppearanceLocked(value)
}

// UpdateMirror returns the persisted, normalized GitHub accelerator origin for
// update traffic; empty means direct connections.
func (s *Store) UpdateMirror() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.config.Appearance.UpdateMirror
}

// Caller holds s.mu across reading, validation and committing the new value.
func (s *Store) saveAppearanceLocked(value Appearance) (Appearance, error) {
	if value.UIFontID == "" {
		value.UIFontID = defaultUIFontID
	}
	if err := validateUIAppearance(value, s.config.Assets); err != nil {
		return Appearance{}, err
	}
	if validBuiltinAssetID(value.FontID) && !builtinTerminalFont(value.FontID) {
		return Appearance{}, errors.New("所选内置终端字体不存在，请从在线字体库下载或重新选择")
	}
	if err := validateChartStyles(value.ChartStyles); err != nil {
		return Appearance{}, err
	}
	if len(value.ExternalEditor) > 4096 || strings.ContainsAny(value.ExternalEditor, "\x00\r\n") {
		return Appearance{}, errors.New("外部编辑器路径无效")
	}
	mirror, err := normalizeUpdateMirror(value.UpdateMirror)
	if err != nil {
		return Appearance{}, err
	}
	value.UpdateMirror = mirror
	if value.FilesPosition == "" {
		value.FilesPosition = "right"
	}
	if value.MinimizeAction == "" {
		value.MinimizeAction = "ask"
	}
	if value.FilesPosition != "top" && value.FilesPosition != "bottom" && value.FilesPosition != "right" {
		return Appearance{}, errors.New("文件面板位置应为 top、bottom 或 right")
	}
	if value.MinimizeAction != "ask" && value.MinimizeAction != "minimize" && value.MinimizeAction != "tray" {
		return Appearance{}, errors.New("最小化行为应为 ask、minimize 或 tray")
	}
	for _, color := range []*string{&value.PromptUsernameColor, &value.PromptHostnameColor} {
		if *color != "" && !validTerminalColor(*color) {
			return Appearance{}, errors.New("提示符颜色应为空或 #RRGGBB 格式")
		}
		*color = strings.ToLower(*color)
	}
	if value.Theme == "" {
		value.Theme = "system"
	}
	if value.Theme != "system" && value.Theme != "light" && value.Theme != "dark" {
		return Appearance{}, errors.New("主题应为 system、light 或 dark")
	}
	if value.UIStyle == "" {
		value.UIStyle = "classic"
	}
	switch value.UIStyle {
	case "classic", "frosted":
	default:
		return Appearance{}, errors.New("工作台样式应为 classic 或 frosted")
	}
	switch value.AnsiPalette {
	case "", "soft", "vivid", "bright", "custom":
	default:
		return Appearance{}, errors.New("终端 ANSI 颜色预设只能是 soft、vivid、bright 或 custom")
	}
	// 空列表归一为 nil；预设（soft/vivid/bright）下已导入的方案保持
	// 存储但不生效。custom 必须命中列表中的方案名，否则回退 soft，
	// 悬空引用不会进入终端。
	if len(value.AnsiSchemes) == 0 {
		value.AnsiSchemes = nil
	}
	if len(value.AnsiSchemes) > 64 {
		return Appearance{}, errors.New("配色方案最多 64 套，请先删除部分方案")
	}
	if value.AnsiSchemes != nil {
		if err := normalizeAnsiSchemes(value.AnsiSchemes); err != nil {
			return Appearance{}, err
		}
	}
	value.AnsiScheme = strings.TrimSpace(value.AnsiScheme)
	if value.AnsiPalette == "custom" {
		found := false
		for _, scheme := range value.AnsiSchemes {
			if scheme.Name == value.AnsiScheme {
				found = true
				break
			}
		}
		if !found {
			value.AnsiPalette, value.AnsiScheme = "soft", ""
		}
	} else {
		value.AnsiScheme = ""
	}
	if err := validateSavedLayout(value.Layout); err != nil {
		return Appearance{}, err
	}
	if math.IsNaN(value.UIScale) || math.IsInf(value.UIScale, 0) || value.UIScale < .5 || value.UIScale > 2 {
		return Appearance{}, errors.New("界面缩放应为 50%–200%")
	}
	if math.IsNaN(value.TerminalFontSize) || math.IsInf(value.TerminalFontSize, 0) || value.TerminalFontSize < 8 || value.TerminalFontSize > 40 || math.Trunc(value.TerminalFontSize*2) != value.TerminalFontSize*2 {
		return Appearance{}, errors.New("终端字号应为 8–40，步进为 0.5 px")
	}
	if len(value.FontColors) > 1024 {
		return Appearance{}, errors.New("已保存的字体颜色过多")
	}
	if len(value.FontBold) > 1024 {
		return Appearance{}, errors.New("已保存的字体样式过多")
	}
	value = cloneAppearance(value)
	for key, color := range value.UITextColors {
		value.UITextColors[key] = strings.ToLower(color)
	}
	value.TerminalBold = false
	for id, color := range value.FontColors {
		if (!validBuiltinAssetID(id) && !validAssetID(id)) || !validTerminalColor(color) {
			return Appearance{}, errors.New("字体颜色应为 #RRGGBB 格式")
		}
		value.FontColors[id] = strings.ToLower(color)
	}
	for id := range value.FontBold {
		if !validBuiltinAssetID(id) && !validAssetID(id) {
			return Appearance{}, errors.New("字体样式引用了无效的字体")
		}
	}
	if !validBuiltinAssetID(value.FontID) {
		found := false
		for _, asset := range s.config.Assets {
			if asset.ID == value.FontID && asset.Kind == "font" {
				found = true
				break
			}
		}
		if !found {
			return Appearance{}, errors.New("所选字体不存在，请重新选择")
		}
	}
	old := s.config.Appearance
	// Native resize callbacks are authoritative. A previously fetched frontend
	// snapshot must never revert more recently saved window geometry.
	value.WindowWidth, value.WindowHeight, value.WindowMaximised = old.WindowWidth, old.WindowHeight, old.WindowMaximised
	s.config.Appearance = value
	if err := s.writeLocked(); err != nil {
		s.config.Appearance = old
		return Appearance{}, err
	}
	return cloneAppearance(value), nil
}

func validSavedLayoutKey(key string) bool {
	switch key {
	case "cloudshell.layout", "dengshell.workspace", "dengshell.appearance-palette", "dengshell.server-groups.collapsed", "dengshell.server-manager", "dengshell.pinned.v1":
		return true
	}
	for _, prefix := range []string{"dengshell.history.", "dengshell.nic."} {
		if strings.HasPrefix(key, prefix) && len(key) > len(prefix) && len(key) <= len(prefix)+128 {
			for _, c := range key[len(prefix):] {
				if (c < 'a' || c > 'z') && (c < 'A' || c > 'Z') && (c < '0' || c > '9') && c != '_' && c != '-' {
					return false
				}
			}
			return true
		}
	}
	return false
}

func validateSavedLayout(layout map[string]json.RawMessage) error {
	if len(layout) > 4096 {
		return errors.New("界面偏好项目过多")
	}
	for key, value := range layout {
		if !validSavedLayoutKey(key) || !json.Valid(value) {
			return errors.New("界面偏好包含无效项目")
		}
	}
	data, err := json.Marshal(layout)
	if err != nil || len(data) > 1<<20 {
		return errors.New("界面偏好和命令历史合计不能超过 1 MiB")
	}
	return nil
}

func (s *Store) SaveWindowState(width, height int, maximised bool) error {
	if width < 320 || height < 240 || width > 16384 || height > 16384 {
		return errors.New("保存的窗口尺寸无效")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	old := s.config.Appearance
	if old.WindowWidth == width && old.WindowHeight == height && old.WindowMaximised == maximised {
		return nil
	}
	s.config.Appearance.WindowWidth, s.config.Appearance.WindowHeight, s.config.Appearance.WindowMaximised = width, height, maximised
	if err := s.writeLocked(); err != nil {
		s.config.Appearance = old
		return err
	}
	return nil
}

func (a *App) Appearance() Appearance { return a.store.List().Appearance }
func (a *App) SaveWindowState(width, height int, maximised bool) error {
	return a.store.SaveWindowState(width, height, maximised)
}

func validBuiltinAssetID(id string) bool {
	if !strings.HasPrefix(id, "builtin:") || len(id) <= 8 || len(id) > 100 {
		return false
	}
	for _, c := range id[8:] {
		if c != '-' && c != '_' && (c < 'a' || c > 'z') && (c < '0' || c > '9') {
			return false
		}
	}
	return true
}

func (a *App) registerSettingsHTTP(mux *http.ServeMux) {
	mux.HandleFunc("POST /api/appearance/patch", a.patchAppearanceHTTP)
	mux.HandleFunc("POST /api/appearance", func(w http.ResponseWriter, r *http.Request) {
		var input Appearance
		if !decode(w, r, &input) {
			return
		}
		value, err := a.store.saveFrontendAppearance(input)
		if err == nil {
			a.uiFontRuntime()
		}
		respond(w, value, err)
	})
	mux.HandleFunc("POST /api/proxies", func(w http.ResponseWriter, r *http.Request) {
		var input ManagedProxy
		if !decode(w, r, &input) {
			return
		}
		value, err := a.store.SaveProxy(input)
		respond(w, value, err)
	})
	mux.HandleFunc("DELETE /api/proxies/{id}", func(w http.ResponseWriter, r *http.Request) {
		respond(w, map[string]bool{"ok": true}, a.store.DeleteProxy(r.PathValue("id")))
	})
	mux.HandleFunc("POST /api/assets", a.importAssetHTTP)
	mux.HandleFunc("POST /api/assets/{id}", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Name string `json:"name"`
		}
		if !decode(w, r, &input) {
			return
		}
		value, err := a.store.RenameAsset(r.PathValue("id"), input.Name)
		respond(w, value, err)
	})
	mux.HandleFunc("DELETE /api/assets/{id}", func(w http.ResponseWriter, r *http.Request) {
		respond(w, map[string]bool{"ok": true}, a.store.DeleteAsset(r.PathValue("id")))
	})
	mux.HandleFunc("GET /api/assets/{id}/data", func(w http.ResponseWriter, r *http.Request) {
		value, err := a.store.AssetData(r.PathValue("id"))
		respond(w, value, err)
	})
}
