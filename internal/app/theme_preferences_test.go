package app

import (
	"encoding/json"
	"os"
	"path/filepath"
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
		`{"id":"11111111-1111-4111-8111-111111111111","name":"夜间工作","theme":"dark","uiStyle":"frosted","fontId":"builtin:jetbrains-mono",` +
		`"ansiPalette":"vivid","terminalFontSize":16,` +
		`"fontColors":{"builtin:jetbrains-mono":"#ABCDEF"},"chartStyles":{"upload":{"color":"#ff0000","width":3,"dashed":true}},` +
		`"promptUsernameColor":"#AABBCC"},` +
		`{"id":"22222222-2222-4222-8222-222222222222","name":"浅色","theme":"light"}]}`
	saved, err := store.PatchAppearance(preferencePatch(t, raw))
	if err != nil {
		t.Fatal(err)
	}
	if len(saved.Themes) != 2 || saved.Themes[0].Name != "夜间工作" || saved.Themes[1].Theme != "light" || saved.Themes[0].UIStyle != "frosted" {
		t.Fatal("themes not stored", saved.Themes)
	}
	// 颜色统一小写。
	if saved.Themes[0].FontColors["builtin:jetbrains-mono"] != "#abcdef" || saved.Themes[0].PromptUsernameColor != "#aabbcc" {
		t.Fatal("theme colors not normalized", saved.Themes[0])
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
		{"无效工作台样式", `{"themes":[{"id":"a1","name":"甲","uiStyle":"paper"}]}`, "工作台样式"},
		{"无效 ANSI", `{"themes":[{"id":"a1","name":"甲","ansiPalette":"retro"}]}`, "ANSI"},
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

// 全局工作台样式：默认经典、两套枚举合法、已移除的 polar/twilight 整笔
// 拒绝且不动已存值；旧配置文件里的旧值在加载时迁回 classic。
func TestAppearanceUIStyle(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if got := store.List().Appearance.UIStyle; got != "classic" {
		t.Fatal("default style should be classic", got)
	}
	for _, style := range []string{"frosted", "classic"} {
		if saved, err := store.PatchAppearance(preferencePatch(t, `{"uiStyle":"`+style+`"}`)); err != nil || saved.UIStyle != style {
			t.Fatalf("style %s rejected: %v %+v", style, err, saved)
		}
	}
	for _, style := range []string{"polar", "twilight", "vapor"} {
		if _, err := store.PatchAppearance(preferencePatch(t, `{"uiStyle":"`+style+`"}`)); err == nil || !strings.Contains(err.Error(), "工作台样式") {
			t.Fatalf("removed or unknown style %s accepted", style)
		}
	}
	if got := store.List().Appearance.UIStyle; got != "classic" {
		t.Fatal("rejected style changed stored value", got)
	}
	if saved, err := reopenedSaveWithEmptyStyle(store); err != nil || saved.UIStyle != "classic" {
		t.Fatal("empty style should normalize back to classic", saved.UIStyle, err)
	}
}

// 旧版本保存过 polar/twilight 的配置与主题快照：重启加载即迁回 classic，
// 不会让后续任何外观保存被枚举校验拒绝。
func TestAppearanceUIStyleRetiredMigration(t *testing.T) {
	dir := t.TempDir()
	raw := `{"appearance":{"uiStyle":"twilight","themes":[{"id":"a1","name":"夜幕快照","uiStyle":"polar"}]}}`
	if err := os.WriteFile(filepath.Join(dir, "config.json"), []byte(raw), 0600); err != nil {
		t.Fatal(err)
	}
	store, err := OpenStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	got := store.List().Appearance
	if got.UIStyle != "classic" {
		t.Fatal("retired live style should migrate to classic", got.UIStyle)
	}
	if len(got.Themes) != 1 || got.Themes[0].UIStyle != "classic" {
		t.Fatal("retired snapshot style should migrate to classic", got.Themes)
	}
	if _, err := store.PatchAppearance(preferencePatch(t, `{"terminalFontSize":16}`)); err != nil {
		t.Fatal("appearance patch after migration should save", err)
	}
}

// reopenedSaveWithEmptyStyle 模拟旧配置缺字段后整体重存时回退默认样式。
func reopenedSaveWithEmptyStyle(store *Store) (Appearance, error) {
	value := store.List().Appearance
	value.UIStyle = ""
	return store.saveAppearanceLocked(value)
}
