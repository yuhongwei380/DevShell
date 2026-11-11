package app

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"
)

const fullAnsiColors = `{"black":"#000000","red":"#C0392B","green":"#27AE60","yellow":"#F39C12","blue":"#2980B9","magenta":"#8E44AD","cyan":"#16A085","white":"#BDC3C7","brightBlack":"#7F8C8D","brightRed":"#E74C3C","brightGreen":"#2ECC71","brightYellow":"#F1C40F","brightBlue":"#3498DB","brightMagenta":"#9B59B6","brightCyan":"#1ABC9C","brightWhite":"#ECF0F1"}`

// alternateAnsiColors 与 fullAnsiColors 仅在 red 上不同，便于断言列表整体替换。
const alternateAnsiColors = `{"black":"#000000","red":"#111111","green":"#27AE60","yellow":"#F39C12","blue":"#2980B9","magenta":"#8E44AD","cyan":"#16A085","white":"#BDC3C7","brightBlack":"#7F8C8D","brightRed":"#E74C3C","brightGreen":"#2ECC71","brightYellow":"#F1C40F","brightBlue":"#3498DB","brightMagenta":"#9B59B6","brightCyan":"#1ABC9C","brightWhite":"#ECF0F1"}`

// schemeListPatch 生成 count 套命名方案的 ansiSchemes 整体替换补丁。
func schemeListPatch(count int) string {
	var builder strings.Builder
	builder.WriteString(`{"ansiSchemes":[`)
	for index := 0; index < count; index++ {
		if index > 0 {
			builder.WriteString(",")
		}
		fmt.Fprintf(&builder, `{"name":"方案 %d","colors":%s}`, index, fullAnsiColors)
	}
	builder.WriteString("]}")
	return builder.String()
}

// 导入配色方案：列表整体写入、名称修剪、色值小写归一、custom 按名应用、重启保留。
func TestAnsiSchemesImportAndApply(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	raw := `{"ansiSchemes":[{"name":"  Solarized Dark ","colors":` + fullAnsiColors + `},{"name":"另 一套","colors":` + alternateAnsiColors + `}]}`
	saved, err := store.PatchAppearance(preferencePatch(t, raw))
	if err != nil {
		t.Fatal(err)
	}
	if len(saved.AnsiSchemes) != 2 || saved.AnsiSchemes[0].Name != "Solarized Dark" || saved.AnsiSchemes[1].Name != "另 一套" {
		t.Fatal("schemes not stored with trimmed names", saved.AnsiSchemes)
	}
	// 导入本身不改变当前外观。
	if saved.AnsiPalette != "" || saved.AnsiScheme != "" {
		t.Fatal("import should not change the live palette", saved.AnsiPalette, saved.AnsiScheme)
	}
	// 颜色统一小写。
	if saved.AnsiSchemes[0].Colors["red"] != "#c0392b" || saved.AnsiSchemes[0].Colors["brightWhite"] != "#ecf0f1" {
		t.Fatal("scheme colors not normalized", saved.AnsiSchemes[0].Colors)
	}
	// 公开结果与存储隔离：改动返回值不得污染配置。
	saved.AnsiSchemes[0].Colors["red"] = "#000000"
	saved.AnsiSchemes[0].Name = "改名"
	if got := store.List().Appearance.AnsiSchemes[0]; got.Name != "Solarized Dark" || got.Colors["red"] != "#c0392b" {
		t.Fatal("scheme list aliases configuration", got)
	}
	// 按名应用。
	if saved, err = store.PatchAppearance(preferencePatch(t, `{"ansiPalette":"custom","ansiScheme":"Solarized Dark"}`)); err != nil {
		t.Fatal(err)
	}
	if saved.AnsiPalette != "custom" || saved.AnsiScheme != "Solarized Dark" {
		t.Fatal("scheme apply failed", saved.AnsiPalette, saved.AnsiScheme)
	}
	// 切到预设：方案列表保持存储，但引用名清空。
	if saved, err = store.PatchAppearance(preferencePatch(t, `{"ansiPalette":"vivid"}`)); err != nil {
		t.Fatal(err)
	}
	if saved.AnsiPalette != "vivid" || len(saved.AnsiSchemes) != 2 || saved.AnsiScheme != "" {
		t.Fatal("preset switch should keep schemes and clear the active name", saved.AnsiPalette, len(saved.AnsiSchemes), saved.AnsiScheme)
	}
	// 切回 custom 但没给出方案名：回退 soft，悬空引用不落库。
	if saved, err = store.PatchAppearance(preferencePatch(t, `{"ansiPalette":"custom"}`)); err != nil {
		t.Fatal(err)
	}
	if saved.AnsiPalette != "soft" || saved.AnsiScheme != "" {
		t.Fatal("custom without a scheme name should fall back to soft", saved.AnsiPalette, saved.AnsiScheme)
	}
	// 应用不存在的方案同样回退 soft。
	if _, err = store.PatchAppearance(preferencePatch(t, `{"ansiPalette":"custom","ansiScheme":"不存在"}`)); err != nil {
		t.Fatal(err)
	}
	if got := store.List().Appearance; got.AnsiPalette != "soft" || got.AnsiScheme != "" {
		t.Fatal("unknown scheme should fall back to soft", got.AnsiPalette, got.AnsiScheme)
	}
	// 删除当前方案（列表整体替换）：同样回退 soft。
	if _, err = store.PatchAppearance(preferencePatch(t, `{"ansiSchemes":[{"name":"Solarized Dark","colors":`+fullAnsiColors+`},{"name":"另 一套","colors":`+alternateAnsiColors+`}]}`)); err != nil {
		t.Fatal(err)
	}
	if _, err = store.PatchAppearance(preferencePatch(t, `{"ansiPalette":"custom","ansiScheme":"另 一套"}`)); err != nil {
		t.Fatal(err)
	}
	if saved, err = store.PatchAppearance(preferencePatch(t, `{"ansiSchemes":[{"name":"Solarized Dark","colors":`+fullAnsiColors+`}]}`)); err != nil {
		t.Fatal(err)
	}
	if saved.AnsiPalette != "soft" || saved.AnsiScheme != "" || len(saved.AnsiSchemes) != 1 {
		t.Fatal("deleting the active scheme should fall back to soft", saved.AnsiPalette, saved.AnsiScheme, len(saved.AnsiSchemes))
	}
	// null 清空列表。
	if saved, err = store.PatchAppearance(preferencePatch(t, `{"ansiSchemes":null}`)); err != nil {
		t.Fatal(err)
	}
	if saved.AnsiSchemes != nil {
		t.Fatal("null should clear schemes", saved.AnsiSchemes)
	}
	// 重启保留：重新导入一套并应用。
	if _, err = store.PatchAppearance(preferencePatch(t, `{"ansiSchemes":[{"name":"Solarized Dark","colors":`+fullAnsiColors+`}]}`)); err != nil {
		t.Fatal(err)
	}
	if _, err = store.PatchAppearance(preferencePatch(t, `{"ansiPalette":"custom","ansiScheme":"Solarized Dark"}`)); err != nil {
		t.Fatal(err)
	}
	reopened, err := OpenStore(store.dir)
	if err != nil {
		t.Fatal(err)
	}
	if got := reopened.List().Appearance; got.AnsiPalette != "custom" || got.AnsiScheme != "Solarized Dark" || len(got.AnsiSchemes) != 1 || got.AnsiSchemes[0].Colors["brightCyan"] != "#1abc9c" {
		t.Fatal("schemes not persisted across restart", got.AnsiPalette, got.AnsiScheme, got.AnsiSchemes)
	}
}

func TestAnsiSchemesValidation(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	invalid := []struct{ name, raw, want string }{
		{"名称重复", `{"ansiSchemes":[{"name":"甲","colors":` + fullAnsiColors + `},{"name":" 甲 ","colors":` + alternateAnsiColors + `}]}`, "名称重复"},
		{"名称为空", `{"ansiSchemes":[{"name":"  ","colors":` + fullAnsiColors + `}]}`, "1–48 个字符"},
		{"名称过长", `{"ansiSchemes":[{"name":"` + strings.Repeat("名", 49) + `","colors":` + fullAnsiColors + `}]}`, "1–48 个字符"},
		{"缺色板", `{"ansiSchemes":[{"name":"甲"}]}`, "配色方案「甲」"},
		{"残缺色键", `{"ansiSchemes":[{"name":"甲","colors":{"red":"#ff0000"}}]}`, "16 个标准色键"},
		{"多余色键", `{"ansiSchemes":[{"name":"甲","colors":` + strings.Replace(fullAnsiColors, `"white":"#BDC3C7",`, `"white":"#BDC3C7","extra":"#111111",`, 1) + `}]}`, "16 个标准色键"},
		{"色值格式", `{"ansiSchemes":[{"name":"甲","colors":` + strings.Replace(fullAnsiColors, `"red":"#C0392B"`, `"red":"red"`, 1) + `}]}`, "#RRGGBB"},
		{"未知预设", `{"ansiPalette":"neon"}`, "soft、vivid、bright 或 custom"},
		{"列表超上限", schemeListPatch(65), "最多 64 套"},
		{"列表不是数组", `{"ansiSchemes":{"name":"甲"}}`, "格式无效"},
	}
	for _, item := range invalid {
		if _, err := store.PatchAppearance(preferencePatch(t, item.raw)); err == nil || !strings.Contains(err.Error(), item.want) {
			t.Fatalf("%s: want %q, got %v", item.name, item.want, err)
		}
	}
	// 失败提交不得留下半套配置。
	if got := store.List().Appearance; got.AnsiPalette != "" || got.AnsiSchemes != nil || got.AnsiScheme != "" {
		t.Fatal("failed patches leaked into configuration", got.AnsiPalette, got.AnsiSchemes, got.AnsiScheme)
	}
	// 恰好 64 套可保存。
	if saved, err := store.PatchAppearance(preferencePatch(t, schemeListPatch(64))); err != nil || len(saved.AnsiSchemes) != 64 {
		t.Fatal("64 schemes should be accepted", err, len(saved.AnsiSchemes))
	}
	// ansiScheme 是标量字符串，null 被拒绝。
	if _, err = store.PatchAppearance(preferencePatch(t, `{"ansiScheme":null}`)); err == nil || !strings.Contains(err.Error(), "不能为 null") {
		t.Fatal("null ansiScheme should be rejected", err)
	}
}

// 配置加载兼容：旧配置带 ansiColors/themes 等已废弃键时按缺省处理。
func TestAnsiSchemesLoadLegacyConfig(t *testing.T) {
	var value Appearance
	legacy := `{"ansiPalette":"vivid","ansiColors":` + fullAnsiColors + `,"themes":[{"id":"11111111-1111-4111-8111-111111111111","name":"旧主题"}]}`
	if err := json.Unmarshal([]byte(legacy), &value); err != nil {
		t.Fatal(err)
	}
	if value.AnsiPalette != "vivid" || value.AnsiSchemes != nil || value.AnsiScheme != "" {
		t.Fatal("legacy config should load with scheme defaults", value.AnsiPalette, value.AnsiSchemes, value.AnsiScheme)
	}
}

// 可选非 ANSI 键（前景/光标/选区/背景）随方案保存并小写归一，颜色可带
// Alpha 字节（#RRGGBBAA）；未知键与非法取值拒绝。
func TestAnsiSchemesExtraColorKeys(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	withExtras := strings.Replace(fullAnsiColors, `"brightWhite":"#ECF0F1"`, `"brightWhite":"#ECF0F1","foreground":"#F8F8F2","cursor":"#F1FA8C","cursorAccent":"#282A36","selectionBackground":"#44475A","selectionForeground":"#F8F8F2","background":"#282A36E6"`, 1)
	saved, err := store.PatchAppearance(preferencePatch(t, `{"ansiSchemes":[{"name":"Dracula","colors":`+withExtras+`}]}`))
	if err != nil {
		t.Fatal(err)
	}
	colors := saved.AnsiSchemes[0].Colors
	if colors["foreground"] != "#f8f8f2" || colors["cursor"] != "#f1fa8c" || colors["cursorAccent"] != "#282a36" || colors["selectionBackground"] != "#44475a" || colors["selectionForeground"] != "#f8f8f2" {
		t.Fatal("extra colour keys not stored lowercased", colors)
	}
	if colors["background"] != "#282a36e6" {
		t.Fatal("background colour with alpha not stored lowercased", colors)
	}
	if colors["red"] != "#c0392b" {
		t.Fatal("standard keys must survive alongside extras", colors)
	}
	invalid := []struct{ name, raw, want string }{
		{"可选键取值非法", `{"ansiSchemes":[{"name":"甲","colors":` + strings.Replace(fullAnsiColors, `"brightWhite":"#ECF0F1"`, `"brightWhite":"#ECF0F1","foreground":"white"`, 1) + `}]}`, "#RRGGBB"},
		{"可选键透明度位数非法", `{"ansiSchemes":[{"name":"甲","colors":` + strings.Replace(fullAnsiColors, `"brightWhite":"#ECF0F1"`, `"brightWhite":"#ECF0F1","background":"#282A366"`, 1) + `}]}`, "#RRGGBB"},
	}
	for _, item := range invalid {
		if _, err := store.PatchAppearance(preferencePatch(t, item.raw)); err == nil || !strings.Contains(err.Error(), item.want) {
			t.Fatalf("%s: want %q, got %v", item.name, item.want, err)
		}
	}

}

func TestAnsiSchemesItermModesAndDecorations(t *testing.T) {
	root := t.TempDir()
	store, err := OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	colors := strings.Replace(fullAnsiColors, `"brightWhite":"#ECF0F1"`, `"brightWhite":"#ECF0F1","bold":"#ABCDEF","link":"#112233","underline":"#445566","tab":"#123456","match":"#FEDCBA","cursorGuide":"#11111140","badge":"#FF000080"`, 1)
	patch := `{"ansiPalette":"custom","ansiScheme":"明暗主题","ansiSchemes":[{"name":"明暗主题","colors":` + colors + `,"lightColors":` + fullAnsiColors + `,"darkColors":` + alternateAnsiColors + `}]}`
	saved, err := store.PatchAppearance(preferencePatch(t, patch))
	if err != nil {
		t.Fatal(err)
	}
	for key, expected := range map[string]string{"bold": "#abcdef", "link": "#112233", "underline": "#445566", "tab": "#123456", "match": "#fedcba", "cursorGuide": "#11111140", "badge": "#ff000080"} {
		if saved.AnsiSchemes[0].Colors[key] != expected {
			t.Fatalf("%s lost in preset: %v", key, saved.AnsiSchemes[0])
		}
	}
	// Returned and cloned snapshots cannot mutate either mode in the Store.
	saved.AnsiSchemes[0].LightColors["red"] = "#000000"
	saved.AnsiSchemes[0].DarkColors["red"] = "#000000"
	reopened, err := OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	for _, snapshot := range []Appearance{store.List().Appearance, reopened.List().Appearance} {
		scheme := snapshot.AnsiSchemes[0]
		if scheme.LightColors["red"] != "#c0392b" || scheme.DarkColors["red"] != "#111111" || scheme.Colors["bold"] != "#abcdef" || snapshot.AnsiScheme != "明暗主题" {
			t.Fatal("mode colors changed or were lost on reload", snapshot)
		}
	}
	for _, bad := range []string{
		strings.Replace(patch, `"lightColors":`+fullAnsiColors, `"lightColors":{}`, 1),
		strings.Replace(patch, `"darkColors":`+alternateAnsiColors, `"darkColors":{"red":"#ff0000"}`, 1),
		strings.Replace(patch, `"bold":"#ABCDEF"`, `"bold":"red"`, 1),
	} {
		if _, err := store.PatchAppearance(preferencePatch(t, bad)); err == nil {
			t.Fatal("invalid mode or decoration accepted", bad)
		}
	}
	if got := store.List().Appearance.AnsiSchemes[0]; got.LightColors["red"] != "#c0392b" || got.Colors["bold"] != "#abcdef" {
		t.Fatal("failed patch changed the saved scheme", got)
	}
}

func TestAnsiSchemesRetiredBackgroundPreference(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	var legacy Appearance
	if err := json.Unmarshal([]byte(`{"ansiSchemeBackground":true,"ansiPalette":"custom","ansiScheme":"旧主题","ansiSchemes":[{"name":"旧主题","colors":`+fullAnsiColors+`}]}`), &legacy); err != nil {
		t.Fatal(err)
	}
	saved, err := store.SaveAppearance(legacy)
	if err != nil {
		t.Fatal(err)
	}
	data, err := json.Marshal(saved)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(data), "ansiSchemeBackground") || saved.AnsiScheme != "旧主题" {
		t.Fatal("retired preference survived or changed the theme", string(data))
	}
	if _, err := store.PatchAppearance(preferencePatch(t, `{"ansiSchemeBackground":true}`)); err == nil {
		t.Fatal("retired background feature can still be enabled")
	}
}
