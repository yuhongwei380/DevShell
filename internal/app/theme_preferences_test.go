package app

import (
	"encoding/json"
	"strings"
	"testing"
)

// 主题管理：主题列表整体替换、字段级校验、重启保留与克隆隔离。
func TestAppearanceThemesPatchAndPersistence(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	raw := `{"themes":[` +
		`{"id":"11111111-1111-4111-8111-111111111111","name":"夜间工作","theme":"dark","fontId":"builtin:jetbrains-mono",` +
		`"ansiPalette":"vivid","backgroundId":"builtin:none","backgroundOpacity":0,"terminalFontSize":16,` +
		`"fontColors":{"builtin:jetbrains-mono":"#ABCDEF"},"chartStyles":{"upload":{"color":"#ff0000","width":3,"dashed":true}},` +
		`"promptUsernameColor":"#AABBCC"},` +
		`{"id":"22222222-2222-4222-8222-222222222222","name":"浅色","theme":"light"}]}`
	saved, err := store.PatchAppearance(preferencePatch(t, raw))
	if err != nil {
		t.Fatal(err)
	}
	if len(saved.Themes) != 2 || saved.Themes[0].Name != "夜间工作" || saved.Themes[1].Theme != "light" {
		t.Fatal("themes not stored", saved.Themes)
	}
	// 颜色统一小写；opacity 为 0 必须保留（与未设置可区分）。
	if saved.Themes[0].FontColors["builtin:jetbrains-mono"] != "#abcdef" || saved.Themes[0].PromptUsernameColor != "#aabbcc" {
		t.Fatal("theme colors not normalized", saved.Themes[0])
	}
	if saved.Themes[0].BackgroundOpacity == nil || *saved.Themes[0].BackgroundOpacity != 0 {
		t.Fatal("zero background opacity lost")
	}
	// 公开结果与存储隔离：改动返回值不得污染配置。
	saved.Themes[0].Name = "改名"
	saved.Themes[0].FontColors["builtin:jetbrains-mono"] = "#000000"
	if got := store.List().Appearance.Themes; got[0].Name != "夜间工作" || got[0].FontColors["builtin:jetbrains-mono"] != "#abcdef" {
		t.Fatal("themes alias configuration", got[0])
	}
	reopened, err := OpenStore(store.dir)
	if err != nil {
		t.Fatal(err)
	}
	if themes := reopened.List().Appearance.Themes; len(themes) != 2 || themes[0].ChartStyles["upload"].Width != 3 || !themes[0].ChartStyles["upload"].Dashed {
		t.Fatal("themes not persisted across restart", themes)
	}
	// 整体替换：第二次提交只留一条；null 清空。
	if saved, err = reopened.PatchAppearance(preferencePatch(t, `{"themes":[{"id":"33333333-3333-4333-8333-333333333333","name":" 空白修剪 ","theme":"system"}]}`)); err != nil {
		t.Fatal(err)
	}
	if len(saved.Themes) != 1 || saved.Themes[0].Name != "空白修剪" {
		t.Fatal("replace or trim failed", saved.Themes)
	}
	if saved, err = reopened.PatchAppearance(preferencePatch(t, `{"themes":null}`)); err != nil {
		t.Fatal(err)
	}
	if saved.Themes != nil {
		t.Fatal("null should clear themes", saved.Themes)
	}
}

func TestAppearanceThemesValidation(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	invalid := []struct{ name, raw, want string }{
		{"空名称", `{"themes":[{"id":"a1","name":"  "}]}`, "主题名称"},
		{"重复名称", `{"themes":[{"id":"a1","name":"同"},{"id":"a2","name":"同"}]}`, "名称重复"},
		{"重复标识", `{"themes":[{"id":"a1","name":"甲"},{"id":"a1","name":"乙"}]}`, "标识重复"},
		{"名称含控制符", `{"themes":[{"id":"a1","name":"坏\n名"}]}`, "主题名称"},
		{"无效明暗", `{"themes":[{"id":"a1","name":"甲","theme":"sepia"}]}`, "明暗"},
		{"无效 ANSI", `{"themes":[{"id":"a1","name":"甲","ansiPalette":"retro"}]}`, "ANSI"},
		{"背景强度越界", `{"themes":[{"id":"a1","name":"甲","backgroundOpacity":1.5}]}`, "背景强度"},
		{"字号越界", `{"themes":[{"id":"a1","name":"甲","terminalFontSize":6}]}`, "字号"},
		{"字号步进", `{"themes":[{"id":"a1","name":"甲","terminalFontSize":14.3}]}`, "字号"},
		{"字体颜色格式", `{"themes":[{"id":"a1","name":"甲","fontColors":{"builtin:jetbrains-mono":"red"}}]}`, "字体颜色"},
		{"字体颜色键", `{"themes":[{"id":"a1","name":"甲","fontColors":{"坏键":"#123456"}}]}`, "字体颜色"},
		{"界面颜色格式", `{"themes":[{"id":"a1","name":"甲","uiTextColors":{"text":"#12"}}]}`, "界面颜色"},
		{"曲线样式", `{"themes":[{"id":"a1","name":"甲","chartStyles":{"upload":{"width":99}}}]}`, "曲线"},
		{"提示符颜色", `{"themes":[{"id":"a1","name":"甲","promptHostnameColor":"#zz"}]}`, "提示符颜色"},
		{"空标识", `{"themes":[{"id":"","name":"甲"}]}`, "标识无效"},
	}
	for _, item := range invalid {
		if _, err := store.PatchAppearance(preferencePatch(t, item.raw)); err == nil || !strings.Contains(err.Error(), item.want) {
			t.Fatalf("%s: expected error containing %q, got %v", item.name, item.want, err)
		}
	}
	if store.List().Appearance.Themes != nil {
		t.Fatal("rejected patches must not store themes")
	}
	if _, err := store.PatchAppearance(preferencePatch(t, `{"theme":"dark"}`)); err != nil {
		t.Fatal(err)
	}
	if got := store.List().Appearance; got.Theme != "dark" || got.Themes != nil {
		t.Fatal("unrelated appearance patch disturbed themes", got.Theme, got.Themes)
	}
}

// 空主题列表是合法的（全部删除后），并且不得让序列化配置出现 themes 键。
func TestAppearanceThemesEmptyIsQuiet(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if saved, err := store.PatchAppearance(preferencePatch(t, `{"themes":[]}`)); err != nil || len(saved.Themes) != 0 {
		t.Fatal("empty list should be accepted", saved.Themes, err)
	}
	data, err := json.Marshal(store.List().Appearance)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(data), `"themes"`) {
		t.Fatal("empty themes should be omitted from config JSON")
	}
}
