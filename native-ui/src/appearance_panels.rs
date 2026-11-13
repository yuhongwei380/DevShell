//! Layouts transcribed from the pinned web/css/{ui-styles,ui-appearance,chart-styles}.css.
use super::*;
use crate::{
    forms::string,
    workspace::{Palette, icon_tool},
};
use gpui_kit::{
    Div,
    component::{Sizable, color_picker::ColorSelect, select::Select},
};

fn text(value: impl Into<gpui_kit::SharedString>, size: f32) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size * 1.4))
        .child(value.into())
}
fn action(id: impl Into<gpui_kit::ElementId>, label: &str) -> KitButton {
    KitButton::new(id)
        .ghost()
        .text_color(gpui_kit::rgb(if crate::ui_scale::dark() {
            0xe5e5e5
        } else {
            0x344f62
        }))
        .child(text(label.to_owned(), 12.))
}

impl DevShellWindow {
    fn open_font_action(
        &mut self,
        kind: &'static str,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = self.form.take();
        let mut original = self.snapshot.config["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| a["id"].as_str() == Some(id))
            .cloned()
            .unwrap_or_else(
                || serde_json::json!({"id":id,"name":"Alibaba PuHuiTi 3.0 55 Regular"}),
            );
        if kind == "font-info" {
            original["licenseText"] = Value::from(include_str!(
                "../../web/assets/ui-fonts/alibaba-puhuiti-3-55-regular-LICENSE.txt"
            ));
        }
        self.open_native_form(kind, original, window, cx);
        self.form.as_mut().unwrap().return_to = previous.map(Box::new);
    }
    pub(super) fn original_appearance_dialog(
        &self,
        p: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let f = self.form.as_ref().unwrap();
        if f.kind == "ui-style" {
            return self.style_cards(p, cx);
        }
        if f.kind == "charts" {
            return self.chart_controls(p, window, cx);
        }
        self.original_font_panel(f, p, window, cx)
    }

    fn style_cards(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let style = self.snapshot.appearance["uiStyle"]
            .as_str()
            .unwrap_or("classic");
        let theme = self.snapshot.appearance["theme"]
            .as_str()
            .unwrap_or("system");
        let dark = self.ui.dark;
        let label = if style == "frosted" && dark {
            "液态玻璃"
        } else {
            "经典"
        };
        div().absolute().inset_0().occlude().bg(gpui_kit::rgba(0x1f354655)).flex().items_center().justify_center().child(div().w(px(620.)).p(px(24.)).rounded(px(16.)).bg(gpui_kit::rgb(p.surface)).text_color(gpui_kit::rgb(p.text)).shadow_lg().occlude().flex().flex_col()
            .child(div().mb(px(22.)).flex().items_center().justify_between()
                .child(div().child(text("全局主题",17.).line_height(px(27.2))).child(text("整套界面材质即时切换；字体、ANSI 与布局保持不变。",12.).line_height(px(19.2)).mt(px(7.)).text_color(gpui_kit::rgb(p.muted))))
                .child(icon_tool("style-close","close","关闭全局主题").size(px(30.)).on_click(cx.listener(|state,_,_,cx|{state.form=None;cx.notify();}))))
            .child(text("明暗模式",12.).font_weight(gpui_kit::FontWeight::SEMIBOLD).mb(px(10.)))
            .child(div().mb(px(24.)).p(px(3.)).gap(px(3.)).rounded(px(9.)).bg(gpui_kit::rgb(if dark{0x131a21}else{0xedf1f5})).flex().children([("light","浅色"),("dark","深色"),("system","跟随系统")].map(|(mode,label)|{
                action(format!("style-mode-{mode}"),label).flex_1().h(px(32.)).rounded(px(7.)).when(theme==mode,|b|b.bg(gpui_kit::rgb(p.surface)).shadow_sm()).on_click(cx.listener(move|state,_,window,cx|{
                    state.ui.dark=if mode=="system"{matches!(window.appearance(),gpui_kit::WindowAppearance::Dark|gpui_kit::WindowAppearance::VibrantDark)}else{mode=="dark"};
                    workspace::apply_theme(state.ui.dark,Some(window),cx);state.save_appearance(serde_json::json!({"theme":mode}),cx);cx.notify();
                }))
            })))
            .child(text("工作台样式",12.).font_weight(gpui_kit::FontWeight::SEMIBOLD).mb(px(10.)))
            .child(div().flex().items_stretch().gap(px(16.)).mb(px(24.)).children([("classic","经典","沿用熟悉的蓝灰配色，面板不透明。","不透明面板 · 经典配色"),("frosted","液态玻璃","经典配色原样保留，面板化作半透明折射玻璃；边缘聚光高光，底层光色流动。","经典配色 · 玻璃材质")].map(|(id,name,description,meta)|{
                div().id(format!("style-card-{id}")).flex_1().min_w_0().p(px(10.)).h(px(231.)).flex().flex_col().justify_center().border_2().border_color(if style==id{gpui_kit::rgb(0x3e6884)}else{gpui_kit::rgba(0x00000000)}).rounded(px(12.)).bg(gpui_kit::rgb(if dark{0x131a21}else{0xf1f4f7})).cursor_pointer()
                    .child(style_swatch(dark,id=="frosted"))
                    .child(div().flex().justify_between().mt(px(12.)).mx(px(2.)).mb(px(6.)).child(text(name,14.).font_weight(gpui_kit::FontWeight::SEMIBOLD)).when(style==id,|d|d.child(text("✓",14.).text_color(gpui_kit::rgb(0x3e6884)))))
                    .child(text(description,11.).line_height(px(18.7)).mx(px(2.)).text_color(gpui_kit::rgb(p.muted)))
                    .child(text(meta,11.).line_height(px(18.7)).mx(px(2.)).mt(px(8.)).text_color(gpui_kit::rgb(p.muted)))
                    .on_click(cx.listener(move|state,_,_,cx|state.save_appearance(serde_json::json!({"uiStyle":id}),cx)))
            })))
            .child(text(format!("当前：{label} · {}{}",if theme=="system"{"跟随系统"}else if dark{"深色"}else{"浅色"},if style=="frosted"&&!dark{"（液态玻璃暂不支持浅色主题）"}else{""}),12.).min_h(px(18.)).text_color(gpui_kit::rgb(p.muted))))
    }

    fn chart_controls(&self, p: Palette, window: &Window, cx: &mut Context<Self>) -> Div {
        let f = self.form.as_ref().unwrap();
        let dark = self.ui.dark;
        div().absolute().inset_0().occlude().bg(gpui_kit::rgba(0x1f354655)).flex().items_center().justify_center().child(div().w(px(440.)).max_h(window.viewport_size().height-px(40.)).p(px(28.)).py(px(25.)).rounded(px(19.)).bg(gpui_kit::rgb(p.surface)).occlude().shadow_lg().flex().flex_col()
            .child(div().flex().items_center().justify_between().mb(px(20.)).child(text("曲线样式",17.)).child(icon_tool("charts-close","close","关闭曲线设置").size(px(30.)).on_click(cx.listener(|state,_,_,cx|{state.form=None;cx.notify();}))))
            .child(text("分别设置每条曲线，调整时预览，保存后重启仍有效。",11.).line_height(px(19.8)).mb(px(20.)).text_color(gpui_kit::rgb(p.muted)))
            .child(div().id("chart-fields").min_h_0().overflow_y_scroll().flex().flex_col().gap(px(12.)).children([("upload","上行网速",if dark{0xf5b36c}else{0xac5418},2.3,false),("download","下行网速",if dark{0x73d2ec}else{0x087b99},1.8,true),("latency","延迟",if dark{0x9bc4b3}else{0x77a88f},1.65,false)].map(|(key,label,default_color,default_width,default_dashed)|{
                let hex=f.text(&format!("{key}Color"),cx);let color=if f.flag(&format!("{key}Theme")){default_color}else{appearance::color(&Value::from(hex)).unwrap_or(default_color)};
                let width=f.text(&format!("{key}Width"),cx).parse::<f32>().unwrap_or(default_width).clamp(0.5,6.);
                let dashed=f.selected(&format!("{key}Dashed"),cx)=="dashed";
                div().relative().pt(px(15.)).pb(px(10.)).px(px(12.)).mt(px(7.)).border_1().border_color(gpui_kit::rgb(p.border)).rounded(px(10.))
                    .child(text(label,12.).absolute().top(px(-9.)).left(px(12.)).px(px(6.)).bg(gpui_kit::rgb(p.surface)))
                    .child(chart_preview(color,width,dashed).mb(px(10.)))
                    .child(div().flex().gap(px(12.))
                        .child(div().flex_1().min_w_0().child(text("颜色",11.).mb(px(7.))).child(div().relative().h(px(34.)).child(ColorSelect::new(&f.colors[&format!("{key}Color")]).w_full().h(px(34.))).child(div().absolute().inset_0().p(px(5.)).border_1().border_color(gpui_kit::rgb(p.border)).rounded(px(8.)).bg(gpui_kit::rgb(if dark{0x131a21}else{0xf9fbfc})).child(div().size_full().bg(gpui_kit::rgb(color))))))
                        .child(div().flex_1().min_w_0().child(text("粗细（px）",11.).mb(px(7.))).child(gpui_kit::Styled::h(Input::new(&f.inputs[&format!("{key}Width")]),px(36.)).text_size(px(12.)).px(px(10.)).bg(gpui_kit::rgb(if dark{0x131a21}else{0xf9fbfc})).rounded(px(8.)).border_color(gpui_kit::rgb(p.border))))
                        .when(key!="latency",|d|d.child(div().flex_1().min_w_0().child(text("线型",11.).mb(px(7.))).child(Select::new(&f.choices[&format!("{key}Dashed")]).h(px(34.)).text_size(px(11.)).appearance(false).rounded(px(8.)).bg(gpui_kit::rgb(p.inset))))))
                    .child(div().flex().items_center().justify_between().mt(px(10.)).child(f.toggle(match key{"upload"=>"uploadTheme","download"=>"downloadTheme",_=>"latencyTheme"},"颜色跟随明暗主题",cx))
                        .child(action(format!("reset-chart-{key}"),"恢复默认").on_click(cx.listener(move|state,_,window,cx|{
                            if let Some(f)=state.form.as_mut(){f.flags.insert(format!("{key}Theme"),true);f.inputs[&format!("{key}Width")].update(cx,|input,cx|input.set_value(default_width.to_string(),window,cx));f.choices[&format!("{key}Dashed")].update(cx,|select,cx|select.set_selected_index(Some(gpui_kit::component::IndexPath::new(usize::from(default_dashed))),window,cx));}cx.notify();
                        }))))
            })))
            .when(f.error.is_some(),|d|d.child(f.error_panel()))
            .child(div().flex().justify_end().gap(px(12.)).mt(px(22.)).child(action("charts-cancel","取消").on_click(cx.listener(|state,_,_,cx|{state.form=None;cx.notify();}))).child(action("charts-save","保存设置").primary().h(px(36.)).px(px(18.)).rounded(px(8.)).on_click(cx.listener(|state,_,_,cx|state.save_appearance_form(cx))))))
    }

    pub(super) fn original_font_panel(
        &self,
        f: &forms::NativeForm,
        p: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let ui = f.kind == "ui-fonts";
        let kind = if ui { "ui-font" } else { "font" };
        let width =
            700f32.min(f32::from(window.viewport_size().width) / crate::ui_scale::current() - 16.);
        let height =
            740f32.min(f32::from(window.viewport_size().height) / crate::ui_scale::current() - 16.);
        let selected = self.snapshot.appearance[if ui { "uiFontId" } else { "fontId" }]
            .as_str()
            .unwrap_or(if ui {
                "builtin:ui-alibaba-puhuiti-3-55"
            } else {
                appearance::FONTS[0].0
            });
        let mut fonts = if ui {
            vec![(
                "builtin:ui-alibaba-puhuiti-3-55".to_string(),
                "Alibaba PuHuiTi 3.0 55 Regular".to_string(),
            )]
        } else {
            [0, 1, 3, 2, 4]
                .iter()
                .map(|i| {
                    let (id, name, _) = appearance::FONTS[*i];
                    (id.to_string(), name.to_string())
                })
                .collect::<Vec<_>>()
        };
        fonts.extend(
            self.snapshot.config["assets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| string(a, "kind") == kind)
                .map(|a| (string(a, "id"), string(a, "name"))),
        );
        let family = self
            .fonts
            .families
            .get(selected)
            .cloned()
            .unwrap_or(appearance::FONTS[0].2.into());
        div().absolute().inset_0().child(div().absolute().left(px(f.position.map(|v|v.0).unwrap_or((f32::from(window.viewport_size().width)/crate::ui_scale::current()-width-18.).max(8.)))).top(px(f.position.map(|v|v.1).unwrap_or(52f32.min((f32::from(window.viewport_size().height)/crate::ui_scale::current()-height).max(8.))))).w(px(width)).h(px(height)).rounded(px(17.)).bg(gpui_kit::rgb(p.surface)).occlude().shadow_lg().flex().flex_col()
            .child(div().px(px(20.)).pt(px(20.)).pb(px(12.)).on_mouse_down(gpui_kit::MouseButton::Left,cx.listener(move|state,event:&gpui_kit::MouseDownEvent,window,cx|{let scale=crate::ui_scale::current();if let Some(f)=state.form.as_mut(){let position=f.position.unwrap_or(((f32::from(window.viewport_size().width)/scale-width-18.).max(8.),52f32.min((f32::from(window.viewport_size().height)/scale-height).max(8.))));f.position=Some(position);f.drag_offset=Some((f32::from(event.position.x)/scale-position.0,f32::from(event.position.y)/scale-position.1));}cx.notify();})).flex().justify_between().items_start().child(div().child(text(if ui{"字体设置 · 界面字体"}else{"字体设置 · Shell 字体"},21.)).child(text(if ui{"调整软件界面的标题、正文和说明文字。"}else{"每款字体单独设置加粗和颜色，点击字体即可应用。"},13.).line_height(px(20.8)).mt(px(8.)).text_color(gpui_kit::rgb(p.muted))))
                .child(icon_tool("font-panel-close","close","关闭字体设置").size(px(30.)).on_click(cx.listener(|state,_,_,cx|{state.form=None;cx.notify();}))))
            .child(div().mx(px(20.)).mb(px(12.)).p(px(4.)).h(px(42.)).rounded(px(10.)).bg(gpui_kit::rgb(p.inset)).flex().children([("ui-fonts","界面字体"),("shell-fonts","Shell 字体")].map(|(id,label)|action(format!("font-tab-{id}"),label).flex_1().h_full().rounded(px(7.)).when(f.kind==id,|b|b.bg(gpui_kit::rgb(p.surface)).shadow_sm()).on_click(cx.listener(move|state,_,window,cx|state.open_appearance(id,window,cx))))))
            .child(div().flex().justify_center().mb(px(12.)).child(KitButton::new("font-online-original").primary().child(text("在线字体",14.).font_weight(gpui_kit::FontWeight::SEMIBOLD)).h(px(44.)).px(px(16.)).rounded(px(10.)).child(div()).on_click(cx.listener(move|state,_,window,cx|state.open_font_library(kind,window,cx)))))
            .child(div().id("installed-fonts-body").min_h_0().flex_1().overflow_y_scroll().px(px(20.)).pb(px(20.)).flex().flex_col()
                .child(div().flex().items_center().justify_between().mb(px(12.)).child(text("已安装字体",if ui{16.}else{14.}).when(ui,|d|d.line_height(px(25.6)).font_weight(gpui_kit::FontWeight::SEMIBOLD))).child(action("import-font-original","＋ 导入字体").h(px(28.)).bg(gpui_kit::rgb(p.inset)).rounded(px(8.)).on_click(cx.listener(move|state,_,_,cx|state.import_font(kind,cx)))))
                .when(ui,|d|d.child(text("下载后即用；手动导入需重启。支持 TTF / OTF / WOFF / WOFF2，最大 64 MiB。",13.).line_height(px(22.1)).mb(px(14.)).text_color(gpui_kit::rgb(p.muted))))
                .when(!ui,|d|d.child(div().h(px(151.)).flex_shrink_0().px(px(13.)).py(px(11.)).mb(px(12.)).rounded(px(13.)).bg(gpui_kit::rgb(0x192d3b)).text_color(gpui_kit::rgb(0xd3e0e8)).child(text(fonts.iter().find(|(id,_)|id==selected).map(|(_,name)|name.clone()).unwrap_or_default(),10.).text_color(gpui_kit::rgb(0x9bb4c7)))
                    .child(text("DevShell ~ $ echo \"你好，世界！\"\n中文目录 /home/用户  ┌─────┐\n繁體中文 ABC 012345  │ SSH │",14.).font_family(family).line_height(px(21.)).mt(px(7.)).mb(px(10.)))
                    .child(div().flex().items_center().gap(px(10.)).child(text("字号",13.)).child(div().w(px(66.)).h(px(30.)).child(Select::new(&f.choices["terminalFontSize"]).h(px(30.)).px(px(7.)).text_size(px(11.)).icon(gpui_kit::component::Icon::new(gpui_kit::component::IconName::ChevronDown).size(px(10.))).appearance(false).bg(gpui_kit::rgb(0x2c4454)).text_color(gpui_kit::rgb(0xd7e1e8)))))))
                .children(fonts.chunks(2).enumerate().map(|(row,fonts)|div().flex().gap(px(12.)).mb(px(if ui{0.}else{12.})).children(fonts.iter().map(|(id,name)|{
                    let id=id.clone();let asset_action=id.clone();let asset_edit=id.clone();let asset_delete=id.clone();let choose=id.clone();let bold_id=id.clone();let color_id=id.clone();let active=selected==id;
                    let color=appearance::color(&self.snapshot.appearance["fontColors"][&id]).unwrap_or(0xd3e0e8);let bold=self.snapshot.appearance["fontBold"][&id].as_bool().unwrap_or(false);
                    let family=self.fonts.families.get(&id).cloned().unwrap_or("Alibaba PuHuiTi 3.0".into());
                    div().id(format!("font-card-{row}-{id}")).w(px((width-52.)/2.)).flex_shrink_0().min_w_0().px(px(12.)).py(px(10.)).min_h(px(91.)).rounded(px(12.)).when(ui,|d|d.border_1().border_color(gpui_kit::rgb(if active{0x3e6884}else{p.border}))).bg(gpui_kit::rgb(if ui{p.surface}else if active{if self.ui.dark{0x213948}else{0xe8f0f6}}else if self.ui.dark{0x131a21}else{0xf4f7fa}))
                        .child(div().flex().items_center().justify_between().gap(px(8.)).min_h(px(30.)).child(KitButton::new(format!("font-choose-{id}")).ghost().text_color(gpui_kit::rgb(p.text)).child(text(name.clone(),if ui{15.}else{14.}).when(ui,|d|d.font_weight(gpui_kit::FontWeight::SEMIBOLD))).when(ui,|b|b.max_w(px(225.))).px(px(7.)).py(px(3.)).h_auto().border_1().border_color(gpui_kit::rgb(p.border)).rounded(px(7.)).on_click(cx.listener(move|state,_,_,cx|state.save_appearance(if ui{serde_json::json!({"uiFontId":choose})}else{serde_json::json!({"fontId":choose})},cx))))
                            .child(text(if active{"正在使用 ✓"}else if id.starts_with("builtin:"){"内置"}else{"自定义"},11.).text_color(gpui_kit::rgb(p.muted))))
                        .when(ui,|d|d.child(text(if id.starts_with("builtin:"){"默认界面字体 · 阿里巴巴普惠体，清晰的中英文混排"}else{"导入的界面字体"},12.).line_height(px(18.)).mt(px(7.)).text_color(gpui_kit::rgb(p.muted))).child(text("简体中文 · 繁體中文 · English 012345",16.).font_family(family.clone()).line_height(px(24.)).mt(px(7.))))
                        .when(!ui,|d|d.child(div().flex().items_center().gap(px(8.)).pt(px(7.)).min_h(px(41.))
                            .child(action(format!("font-sample-{id}"),"").justify_start().flex_1().min_w_0().h(px(30.)).px(px(6.)).rounded(px(7.)).bg(gpui_kit::rgb(if self.ui.dark{0x2b434f}else{0x304b58})).child(text("Aa 012 中文",13.).w_full().text_left().font_family(family).text_color(gpui_kit::rgb(color)).font_weight(if bold{gpui_kit::FontWeight::BOLD}else{gpui_kit::FontWeight::NORMAL})).on_click(cx.listener(move|state,_,_,cx|state.save_appearance(serde_json::json!({"fontId":id}),cx))))
                            .child(gpui_kit::component::checkbox::Checkbox::new(format!("font-bold-{bold_id}")).small().checked(bold).child(text("加粗",12.)).on_click(cx.listener(move|state,value:&bool,_,cx|{let mut values=state.snapshot.appearance["fontBold"].as_object().cloned().unwrap_or_default();values.insert(bold_id.clone(),(*value).into());state.save_appearance(serde_json::json!({"fontBold":values}),cx);})))
                            .child(action(format!("font-color-{color_id}"),"颜色").child(div().size(px(12.)).rounded_full().border_1().border_color(gpui_kit::rgba(0x7695ac66)).bg(gpui_kit::rgb(color))).h(px(28.)).px(px(6.)).rounded(px(8.)).bg(gpui_kit::rgb(p.surface)).on_click(cx.listener(move|state,_,window,cx|{let previous=state.form.take();state.open_appearance("font-color",window,cx);if let Some(f)=state.form.as_mut(){f.return_to=previous.map(Box::new);f.original["_fontId"]=Value::from(color_id.clone());f.inputs["color"].update(cx,|input,cx|input.set_value(format!("#{color:06x}"),window,cx));}})))))
                        .when(!asset_action.starts_with("builtin:"),|d|d.child(div().flex().justify_end().gap(px(14.)).pt(px(7.))
                            .child(action(format!("asset-rename-{asset_action}"),"改名").on_click(cx.listener(move|state,_,window,cx|state.open_font_action("asset-rename",&asset_edit,window,cx))))
                            .child(action(format!("asset-delete-{asset_action}"),"删除").on_click(cx.listener(move|state,_,window,cx|state.open_font_action("asset-delete",&asset_delete,window,cx))))))
                        .when(ui&&asset_action.starts_with("builtin:"),|d|d.child(action(format!("font-license-{asset_action}"),"开源许可 · OFL 1.1").justify_start().mt(px(10.)).on_click(cx.listener(move|state,_,window,cx|state.open_font_action("font-info",&asset_action,window,cx)))))
                })).when(fonts.len()==1,|d|d.child(div().flex_1()))))
                .when(ui,|d|d.child(div().mt(px(20.)).pt(px(16.)).border_t_1().border_color(gpui_kit::rgb(p.border)).child(action("toggle-ui-colors",if f.expanded.contains("ui-colors"){"▾ 文字颜色 · 浅色 / 深色"}else{"▸ 文字颜色 · 浅色 / 深色"}).justify_start().p_0().on_click(cx.listener(|state,_,_,cx|{if let Some(f)=state.form.as_mut(){if !f.expanded.remove("ui-colors"){f.expanded.insert("ui-colors".into());}}cx.notify();}))).when(f.expanded.contains("ui-colors"),|d|d.child(f.field("uiLight","浅色模式")).child(f.field("uiDark","深色模式")).child(action("ui-colors-save","保存颜色").on_click(cx.listener(|state,_,_,cx|state.save_appearance_form(cx)))))))))
    }
}

fn style_swatch(dark: bool, frosted: bool) -> Div {
    let background = if dark { 0x0f1419 } else { 0xe7edf2 };
    let pane = if dark { 0x24303a } else { 0xffffff };
    div()
        .relative()
        .h(px(104.))
        .rounded(px(7.))
        .overflow_hidden()
        .bg(gpui_kit::rgb(background))
        .child(
            div()
                .absolute()
                .left(px(7.))
                .top(px(8.))
                .bottom(px(8.))
                .w(gpui_kit::relative(0.24))
                .rounded(px(4.))
                .bg(if frosted {
                    gpui_kit::rgba(if dark { 0x181f26b3 } else { 0xffffff80 })
                } else {
                    gpui_kit::rgb(pane)
                }),
        )
        .child(
            div()
                .absolute()
                .left(gpui_kit::relative(0.34))
                .right(px(7.))
                .top(px(8.))
                .h(gpui_kit::relative(0.21))
                .rounded(px(4.))
                .bg(gpui_kit::rgb(pane)),
        )
        .child(
            div()
                .absolute()
                .left(gpui_kit::relative(0.34))
                .right(px(7.))
                .bottom(px(8.))
                .top(gpui_kit::relative(0.35))
                .rounded(px(4.))
                .bg(gpui_kit::rgb(if dark { 0x0f161d } else { 0x263d4b })),
        )
}

fn chart_preview(color: u32, width: f32, dashed: bool) -> Div {
    div().h(px(30.)).w_full().child(
        gpui_kit::canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let scale = crate::ui_scale::current();
                let inset = (f32::from(bounds.size.width) - 260. * scale) / 2.;
                let point = |x: f32, y: f32| {
                    gpui_kit::point(
                        bounds.origin.x + gpui_kit::px(inset + x * scale),
                        bounds.origin.y + px(y),
                    )
                };
                let mut builder = gpui_kit::PathBuilder::stroke(px(width));
                if dashed {
                    builder = builder.dash_array(&[px(5.), px(4.)]);
                }
                builder.move_to(point(3., 24.));
                let curves = [
                    (3., 24., 28., 24., 28., 8., 53., 8.),
                    (53., 8., 78., 8., 78., 22., 103., 22.),
                    (103., 22., 128., 22., 128., 5., 153., 5.),
                    (153., 5., 188., 5., 188., 24., 213., 24.),
                    (213., 24., 238., 24., 238., 12., 257., 12.),
                ];
                for (_, _, x1, y1, x2, y2, x3, y3) in curves {
                    builder.cubic_bezier_to(point(x3, y3), point(x1, y1), point(x2, y2));
                }
                if let Ok(path) = builder.build() {
                    window.paint_path(path, gpui_kit::rgb(color));
                }
            },
        )
        .size_full(),
    )
}
