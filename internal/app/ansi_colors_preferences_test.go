package app

import (
	"encoding/json"
	"strings"
	"testing"
)

const fullAnsiColors = `{"black":"#000000","red":"#C0392B","green":"#27AE60","yellow":"#F39C12","blue":"#2980B9","magenta":"#8E44AD","cyan":"#16A085","white":"#BDC3C7","brightBlack":"#7F8C8D","brightRed":"#E74C3C","brightGreen":"#2ECC71","brightYellow":"#F1C40F","brightBlue":"#3498DB","brightMagenta":"#9B59B6","brightCyan":"#1ABC9C","brightWhite":"#ECF0F1"}`

func validCustomPatch() string {
	return `{"ansiPalette":"custom","ansiColors":` + fullAnsiColors + `}`
}

// 导入 iTerm 配色：custom 色板整体写入、大小写归一、重启保留；
// 切回预设仅停用不清除，切回 custom 即恢复。
func TestAnsiCustomPalettePatchAndPersistence(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	saved, err := store.PatchAppearance(preferencePatch(t, validCustomPatch()))
	if err != nil {
		t.Fatal(err)
	}
	if saved.AnsiPalette != "custom" || len(saved.AnsiColors) != 16 {
		t.Fatal("custom palette not stored", saved.AnsiPalette, saved.AnsiColors)
	}
	// 颜色统一小写。
	if saved.AnsiColors["red"] != "#c0392b" || saved.AnsiColors["brightWhite"] != "#ecf0f1" {
		t.Fatal("ansi colors not normalized", saved.AnsiColors)
	}
	// 公开结果与存储隔离：改动返回值不得污染配置。
	saved.AnsiColors["red"] = "#000000"
	if got := store.List().Appearance.AnsiColors["red"]; got != "#c0392b" {
		t.Fatal("ansi colors alias configuration", got)
	}
	// 切到预设：色板保持存储但不生效；切回 custom 自动恢复。
	if saved, err = store.PatchAppearance(preferencePatch(t, `{"ansiPalette":"vivid"}`)); err != nil {
		t.Fatal(err)
	}
	if saved.AnsiPalette != "vivid" || len(saved.AnsiColors) != 16 {
		t.Fatal("preset switch should keep ansiColors", saved.AnsiPalette, len(saved.AnsiColors))
	}
	if saved, err = store.PatchAppearance(preferencePatch(t, `{"ansiPalette":"custom"}`)); err != nil {
		t.Fatal(err)
	}
	if saved.AnsiPalette != "custom" || saved.AnsiColors["green"] != "#27ae60" {
		t.Fatal("custom switch did not restore palette", saved.AnsiColors)
	}
	// 键级合并：只改单项不影响其余。
	if saved, err = store.PatchAppearance(preferencePatch(t, `{"ansiColors":{"red":"#FF0000"}}`)); err != nil {
		t.Fatal(err)
	}
	if saved.AnsiColors["red"] != "#ff0000" || len(saved.AnsiColors) != 16 || saved.AnsiColors["blue"] != "#2980b9" {
		t.Fatal("ansi color merge failed", saved.AnsiColors)
	}
	reopened, err := OpenStore(store.dir)
	if err != nil {
		t.Fatal(err)
	}
	if got := reopened.List().Appearance; got.AnsiPalette != "custom" || got.AnsiColors["brightCyan"] != "#1abc9c" {
		t.Fatal("custom palette not persisted across restart", got.AnsiPalette, got.AnsiColors)
	}
}

func TestAnsiCustomPaletteValidation(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	invalid := []struct{ name, raw, want string }{
		{"custom 缺色板", `{"ansiPalette":"custom"}`, "16 个标准色键"},
		{"缺色键", `{"ansiPalette":"custom","ansiColors":{"red":"#ff0000"}}`, "16 个标准色键"},
		{"多余色键", `{"ansiPalette":"custom","ansiColors":` + strings.Replace(fullAnsiColors, `"white":"#BDC3C7",`, `"white":"#BDC3C7","extra":"#111111",`, 1) + `}`, "16 个标准色键"},
		{"色值格式", `{"ansiPalette":"custom","ansiColors":` + strings.Replace(fullAnsiColors, `"red":"#C0392B"`, `"red":"red"`, 1) + `}`, "#RRGGBB"},
		{"未知预设", `{"ansiPalette":"neon"}`, "soft、vivid、bright 或 custom"},
		{"预设下残缺色板", `{"ansiColors":{"red":"#ff0000"}}`, "16 个标准色键"},
	}
	for _, item := range invalid {
		if _, err := store.PatchAppearance(preferencePatch(t, item.raw)); err == nil || !strings.Contains(err.Error(), item.want) {
			t.Fatalf("%s: want %q, got %v", item.name, item.want, err)
		}
	}
	// 失败提交不得留下半套配置。
	if got := store.List().Appearance; got.AnsiPalette != "" || got.AnsiColors != nil {
		t.Fatal("failed patches leaked into configuration", got.AnsiPalette, got.AnsiColors)
	}
	// 合法导入后按 null 删除单个色键会破坏完整性，应被拒绝。
	if _, err = store.PatchAppearance(preferencePatch(t, validCustomPatch())); err != nil {
		t.Fatal(err)
	}
	if _, err = store.PatchAppearance(preferencePatch(t, `{"ansiColors":{"red":null}}`)); err == nil || !strings.Contains(err.Error(), "16 个标准色键") {
		t.Fatal("dropping one ansi key should fail", err)
	}
	// null 整个字段不是键值对象。
	if _, err = store.PatchAppearance(preferencePatch(t, `{"ansiColors":null}`)); err == nil {
		t.Fatal("null ansiColors should be rejected")
	}
}

// 主题快照携带 custom 色板：整体替换保存、小写归一、克隆隔离与重启保留。
func TestAnsiCustomPaletteInThemes(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	raw := `{"themes":[{"id":"11111111-1111-4111-8111-111111111111","name":"iTerm","ansiPalette":"custom","ansiColors":` + fullAnsiColors + `}]}`
	saved, err := store.PatchAppearance(preferencePatch(t, raw))
	if err != nil {
		t.Fatal(err)
	}
	if len(saved.Themes) != 1 || saved.Themes[0].AnsiPalette != "custom" || len(saved.Themes[0].AnsiColors) != 16 {
		t.Fatal("theme ansi palette not stored", saved.Themes)
	}
	if saved.Themes[0].AnsiColors["magenta"] != "#8e44ad" {
		t.Fatal("theme ansi colors not normalized", saved.Themes[0].AnsiColors)
	}
	// 主题快照与存储隔离。
	saved.Themes[0].AnsiColors["magenta"] = "#000000"
	if got := store.List().Appearance.Themes[0].AnsiColors["magenta"]; got != "#8e44ad" {
		t.Fatal("theme ansi colors alias configuration", got)
	}
	reopened, err := OpenStore(store.dir)
	if err != nil {
		t.Fatal(err)
	}
	if got := reopened.List().Appearance.Themes; len(got) != 1 || got[0].AnsiColors["brightYellow"] != "#f1c40f" {
		t.Fatal("theme ansi palette not persisted across restart", got)
	}
	// 主题内 custom 缺色板或色值无效都要拒绝。
	invalid := []struct{ name, raw, want string }{
		{"主题 custom 缺色板", `{"themes":[{"id":"a1","name":"甲","ansiPalette":"custom"}]}`, "16 个标准色键"},
		{"主题色值格式", `{"themes":[{"id":"a1","name":"甲","ansiPalette":"custom","ansiColors":{"black":"#000000","red":"red","green":"#27AE60","yellow":"#F39C12","blue":"#2980B9","magenta":"#8E44AD","cyan":"#16A085","white":"#BDC3C7","brightBlack":"#7F8C8D","brightRed":"#E74C3C","brightGreen":"#2ECC71","brightYellow":"#F1C40F","brightBlue":"#3498DB","brightMagenta":"#9B59B6","brightCyan":"#1ABC9C","brightWhite":"#ECF0F1"}}]}`, "16 个标准色键"},
		{"主题未知预设", `{"themes":[{"id":"a1","name":"甲","ansiPalette":"retro"}]}`, "soft、vivid、bright 或 custom"},
	}
	for _, item := range invalid {
		if _, err := store.PatchAppearance(preferencePatch(t, item.raw)); err == nil || !strings.Contains(err.Error(), item.want) {
			t.Fatalf("%s: want %q, got %v", item.name, item.want, err)
		}
	}
}

// 配置加载兼容：旧配置没有 ansiColors 字段时按缺省处理。
func TestAnsiCustomPaletteLoadDefaults(t *testing.T) {
	var value Appearance
	if err := json.Unmarshal([]byte(`{"ansiPalette":"vivid"}`), &value); err != nil {
		t.Fatal(err)
	}
	if value.AnsiPalette != "vivid" || value.AnsiColors != nil {
		t.Fatal("legacy config should load without ansiColors", value.AnsiPalette, value.AnsiColors)
	}
}
