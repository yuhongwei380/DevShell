use super::*;
use gpui_kit::component::Sizable;
use gpui_kit::{Div, SharedString};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum FilesPosition {
    #[default]
    Bottom,
    Top,
    Right,
}

impl FilesPosition {
    pub fn from_value(value: &str) -> Self {
        match value {
            "right" => Self::Right,
            "top" => Self::Top,
            _ => Self::Bottom,
        }
    }

}

// Dimensions come from style.css / appearance-tools.js. Both rendering and PTY
// sizing consume this calculation, so toggling a panel cannot clip the terminal.
#[derive(Debug)]
pub(super) struct ShellLayout {
    pub monitor: bool,
    pub files_right: bool,
    pub files_extent: f32,
    pub terminal_width: f32,
    pub terminal_height: f32,
}

impl ShellLayout {
    pub fn new(
        width: f32,
        height: f32,
        monitor: bool,
        files: bool,
        position: FilesPosition,
        extent: Option<f32>,
    ) -> Self {
        let monitor = monitor && width >= 1000.;
        let body_width = (width - 24. - if monitor { 316. } else { 0. }).max(0.);
        let body_height = (height - 66.).max(0.);
        let files_right = files && position == FilesPosition::Right;
        let files_extent = if !files {
            0.
        } else if files_right {
            extent
                .unwrap_or(420.)
                .clamp(240., (body_width - 330. - 8.).max(240.))
        } else {
            extent
                .unwrap_or((height * 0.25).clamp(200., 300.))
                .clamp(170., (body_height - 220. - 8.).max(170.))
        };
        Self {
            monitor,
            files_right,
            files_extent,
            terminal_width: (body_width - if files_right { files_extent + 8. } else { 0. })
                .max(0.),
            terminal_height: (body_height
                - if files && !files_right {
                    files_extent + 8.
                } else {
                    0.
                })
            .max(0.),
        }
    }
}

pub(super) struct WorkspaceUi {
    pub connections_visible: bool,
    pub viewport:(f32,f32),
    pub pinned_visible:bool,
    pub tab_list_visible:bool,
    pub tab_menu:Option<(String,gpui_kit::Point<gpui_kit::Pixels>)>,
    pub manager: server_manager::ManagerUi,
    pub settings: settings::SettingsUi,
    pub rdp: remote_desktop::RdpUi,
    pub monitor_visible: bool,
    pub concise: bool,
    pub(super) files_visible: bool,
    pub files_pane: &'static str,
    pub dark: bool,
    pub about_visible: bool,
    pub close_visible: bool,
    pub font_size: f32,
    pub command_visible: bool,
    pub files_position: FilesPosition,
    pub(super) files_extent: Option<f32>,
    resizing_files: bool,
    pub settings_view: &'static str,
    history_visible: bool,
    pub search: Entity<InputState>,
    terminal_focus: FocusHandle,
    pub terminal_search:Entity<InputState>,
    pub terminal_search_visible:bool,
    pub terminal_search_focus:bool,
    pub session_order: Vec<String>,
    tabs_scroll: gpui_kit::ScrollHandle,
    pub start_tabs: Vec<u64>,
    pub active_start: Option<u64>,
    pub next_start: u64,
    pub closing_sessions: Vec<String>,
    pub command_group: Option<String>,
    pub command_form: bool,
    pub command_editing: Option<String>,
    pub command_deleting: bool,
    pub command_name: Entity<InputState>,
    pub command_group_input: Entity<InputState>,
    pub command_body: Entity<TextareaState>,
    pub utility: Option<workspace_tools::UtilityDialog>,
    pub utility_serial: u64,
    pub file_filter: Entity<InputState>,
    pub file_path: Entity<InputState>,
    file_path_synced: String,
    pub file_sort: &'static str,
    pub file_sort_desc: bool,
}

impl WorkspaceUi {
    pub fn restore(&mut self, appearance: &Value, dark: bool) {
        self.dark = dark;
        self.concise=appearance["layout"]["conciseMode"].as_bool().unwrap_or(true);
        self.command_visible=!self.concise;
        self.monitor_visible = appearance
            .get("monitorVisible")
            .and_then(Value::as_bool)
            .unwrap_or(!self.concise);
        self.font_size = appearance
            .get("terminalFontSize")
            .and_then(Value::as_f64)
            .unwrap_or(14.)
            .clamp(8., 40.) as f32;
        self.files_visible=!appearance["layout"]["filesHidden"].as_bool().unwrap_or(true);
        self.files_extent=appearance["layout"]["files"].as_f64().filter(|v|v.is_finite()).map(|v|v as f32);
        self.files_position = FilesPosition::from_value(
            appearance
                .get("filesPosition")
                .and_then(Value::as_str)
                .unwrap_or("bottom"),
        );
    }
    pub fn new(window: &mut Window, cx: &mut Context<DevShellWindow>) -> Self {
        let terminal_search=cx.new(|cx|InputState::new(window,cx).placeholder("搜索终端"));
        cx.subscribe_in(&terminal_search,window,|state,input,event:&gpui_kit::base::input::InputEvent,_,cx|{
            if let Some(view)=state.workspace.terminal.as_ref().and_then(|t|t.view.clone()) {
                let query=input.read(cx).value().to_string();
                view.update(cx,|terminal,cx|{if let gpui_kit::base::input::InputEvent::PressEnter{shift,..}=event {terminal.step(!shift,cx);}else if matches!(event,gpui_kit::base::input::InputEvent::Change){let _=terminal.find(&query,Default::default(),cx);}});
            }
        }).detach();
        Self {
            viewport:(1280.,800.),
            connections_visible: false,
            pinned_visible:false,
            tab_list_visible:false,
            tab_menu:None,
            manager: server_manager::ManagerUi::default(),
            settings: settings::SettingsUi::new(window,cx),
            rdp: remote_desktop::RdpUi::default(),
            monitor_visible: false,
            concise:true,
            files_visible: false,
            files_pane: "files",
            dark: true,
            about_visible: false,
            close_visible: false,
            font_size: 14.,
            command_visible: false,
            files_position: FilesPosition::Bottom,
            files_extent: None,
            resizing_files: false,
            settings_view: "main",
            history_visible: false,
            terminal_focus: cx.focus_handle(),
            terminal_search,terminal_search_visible:false,terminal_search_focus:false,
            session_order: Vec::new(),
            tabs_scroll: gpui_kit::ScrollHandle::default(),
            start_tabs: vec![],
            active_start: None,
            next_start: 2,
            closing_sessions: Vec::new(),
            command_group: None,
            command_form: false,
            command_editing: None,
            command_deleting: false,
            command_name: cx.new(|cx| InputState::new(window, cx).placeholder("命令名称")),
            command_group_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("分组")
                    .default_value("常用命令")
            }),
            command_body: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("命令内容")
                    .auto_grow(3, 8)
            }),
            utility: None,
            utility_serial: 0,
            file_filter: cx.new(|cx| InputState::new(window, cx).placeholder("筛选文件")),
            file_path: cx.new(|cx| InputState::new(window, cx).placeholder("远程目录路径")),
            file_path_synced: String::new(),
            file_sort: "name",
            file_sort_desc: false,
            search: cx
                .new(|cx| InputState::new(window, cx).placeholder("搜索服务器名称、地址或分组")),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub background: u32,
    pub surface: u32,
    pub inset: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub terminal: u32,
    pub terminal_meta: u32,
}
impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                background: 0x0f1419,
                surface: 0x181f26,
                inset: 0x131a21,
                border: 0x2d3944,
                text: 0xe5e5e5,
                muted: 0xa3a3a3,
                terminal: 0x0f161d,
                terminal_meta: 0x121b23,
            }
        } else {
            Self {
                background: 0xf0f3f7,
                surface: 0xffffff,
                inset: 0xeaf1f6,
                border: 0xe7edf2,
                text: 0x243b4e,
                muted: 0x486174,
                terminal: 0x223746,
                terminal_meta: 0x1a3040,
            }
        }
    }
}

fn tool(id: impl Into<gpui_kit::ElementId>, label: impl Into<SharedString>) -> KitButton {
    let label:SharedString=label.into();
    KitButton::new(id).ghost().small().accessibility_label(label.clone())
        .when(!label.is_empty(),|b|b.child(div().text_size(px(11.)).line_height(px(15.4)).font_weight(gpui_kit::FontWeight::NORMAL).child(label)))
}

pub(super) fn icon(name: &str, size: f32) -> gpui_kit::Svg {
    gpui_kit::svg()
        .path(format!("shell/{name}.svg"))
        .size(px(size))
        .text_color(gpui_kit::rgb(crate::ui_scale::icon_color()))
}

pub(super) fn icon_tool(id: impl Into<gpui_kit::ElementId>, name: &str, title: &str) -> KitButton {
    tool(id, "")
        .size(px(28.))
        .p_0()
        .rounded(px(8.))
        .accessibility_label(title.to_owned())
        .tooltip(title.to_owned())
        .child(icon(name, 15.).when(matches!(name,"sun"|"moon"),|icon|icon.text_color(gpui_kit::rgb(if crate::ui_scale::dark(){0xdbc69f}else{0x77551e}))).when(name=="settings",|icon|icon.text_color(gpui_kit::rgb(if crate::ui_scale::dark(){0xbdc6e1}else{0x445b85}))))
}

fn card(p: Palette) -> Div {
    div()
        .rounded(px(16.))
        .bg(gpui_kit::rgb(p.surface))
        .border_1()
        .border_color(gpui_kit::rgb(p.border))
}

pub(super) fn apply_theme(dark: bool, window: Option<&mut Window>, cx: &mut App) {
    use gpui_kit::component::theme::{Theme, ThemeMode};
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        window,
        cx,
    );
    let p = Palette::new(dark);
    // Updating the colors directly leaves GPUI Kit's background tokens behind.
    // Theme::update reconciles both representations and refreshes Base inputs.
    Theme::update(cx, |theme| {
        theme.font_family = "Alibaba PuHuiTi 3.0".into();
        theme.font_size = px(13.);
        theme.radius = px(8.);
        theme.colors.background = gpui_kit::rgb(p.surface).into();
        theme.colors.foreground = gpui_kit::rgb(p.text).into();
        theme.colors.border = gpui_kit::rgb(p.border).into();
        theme.colors.input = gpui_kit::rgb(p.border).into();
        theme.colors.muted = gpui_kit::rgb(p.inset).into();
        theme.colors.muted_foreground = gpui_kit::rgb(p.muted).into();
        theme.colors.primary = gpui_kit::rgb(if dark {0x658daf}else{0x3e6884}).into();
        theme.colors.primary_foreground = gpui_kit::rgb(0xffffff).into();
        theme.colors.button_primary = theme.colors.primary;
        theme.colors.button_primary_foreground = theme.colors.primary_foreground;
        theme.colors.button_secondary = gpui_kit::rgb(p.inset).into();
        theme.colors.button_secondary_foreground = gpui_kit::rgb(p.text).into();
    });
}

impl Render for DevShellWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ui.viewport=(f32::from(window.viewport_size().width),f32::from(window.viewport_size().height));
        let scale=crate::ui_scale::effective_for(self.snapshot.appearance["uiScale"].as_f64().unwrap_or(1.) as f32,f32::from(window.viewport_size().width),f32::from(window.viewport_size().height),self.ui.files_visible&&self.ui.files_position==FilesPosition::Right);
        crate::ui_scale::set(scale);window.set_rem_size(gpui_kit::px(16.*scale));
        if self.settings_visible{self.ensure_preferences(window,cx);}
        self.refresh_connection_groups(window,cx);
        if self.ui.files_visible{self.ensure_file_tree(cx);}
        if let Some((kind,value))=self.pending_dialog.take(){self.open_native_form(kind,value,window,cx);}
        if self.ui.terminal_search_focus{use gpui_kit::Focusable;self.ui.terminal_search_focus=false;window.focus(&self.ui.terminal_search.read(cx).focus_handle(cx),cx);}
        if let Some(server)=self.pending_credential.take(){self.open_native_form("credentials",server.raw,window,cx);}
        if self.ui.settings.clear_unlock {self.ui.settings.clear_unlock=false;self.ui.settings.unlock.update(cx,|input,cx|input.set_value("",window,cx));}
        if let Some(form)=self.form.as_mut().filter(|f|f.kind=="security") {
            if form.original["_clearProof"]==true{form.original["_clearProof"]=false.into();form.inputs["authValue"].update(cx,|input,cx|input.set_value("",window,cx));}
            if form.original["_refreshEnrollment"]==true{form.original["_refreshEnrollment"]=false.into();let secret=crate::forms::string(&form.original["_enrollment"],"secret");form.inputs["enrollmentSecret"].update(cx,|input,cx|input.set_value(secret,window,cx));form.inputs["code"].update(cx,|input,cx|input.set_value("",window,cx));}
        }
        if let Some(text) = self.workspace.preview_sync.take() {
            if let Some(editor)=self.workspace.editor.clone(){self.preview_input=editor;}
            else{let language=self.workspace.preview.as_ref().map(|p|editor_language(&p.path)).unwrap_or("Plain Text");self.preview_input=cx.new(|cx|ely_gpui_component::editor::CodeEditor::new(text.clone(),window,cx).language(language));self.workspace.editor=Some(self.preview_input.clone());}
            if self.preview_input.read(cx).text()!=text{self.preview_input.update(cx,|editor,cx|editor.set_text(text,cx));}
        }
        if let Some(files) = &self.workspace.files {
            if self.ui.file_path_synced != files.path {
                self.ui.file_path_synced = files.path.clone();
                self.ui.file_path.update(cx, |input, cx| {
                    input.set_value(files.path.clone(), window, cx)
                });
            }
        }
        self.load_native_fonts(cx);
        let mut p = Palette::new(self.ui.dark);
        crate::ui_scale::set_dark(self.ui.dark);
        crate::ui_scale::set_icon_color(if self.ui.dark {0xa3a3a3}else{0x344f62});
        if let Some(color)=crate::appearance::color(&self.snapshot.appearance["uiTextColors"][if self.ui.dark{"dark"}else{"light"}]){p.text=color;p.muted=color;}
        let ui_font=self.fonts.families.get(self.snapshot.appearance["uiFontId"].as_str().unwrap_or("builtin:ui-alibaba-puhuiti-3-55")).cloned().unwrap_or("Alibaba PuHuiTi 3.0".into());
        {use gpui_kit::component::theme::Theme;let text=gpui_kit::Hsla::from(gpui_kit::rgb(p.text));if Theme::global(cx).colors.foreground!=text||Theme::global(cx).font_family.as_ref()!=ui_font {Theme::update(cx,|theme|{theme.colors.foreground=text;theme.colors.muted_foreground=gpui_kit::rgb(p.muted).into();theme.font_family=ui_font.clone().into();});}}

        if let Some(terminal_view)=self.workspace.terminal.as_ref().and_then(|t|t.view.clone()) {
            let mut appearance=crate::appearance::terminal_appearance(&self.snapshot.appearance,self.ui.font_size,p.terminal);
            if let Some(family)=self.snapshot.appearance["fontId"].as_str().and_then(|id|self.fonts.families.get(id)){appearance.family=family.clone().into();}
            terminal_view.update(cx,|terminal,cx|terminal.set_remote_appearance(appearance,cx));
        }
        self.ensure_monitor_charts(window,cx);
        self.sync_rdp_viewport(window,cx);
        let is_rdp=self.workspace.rdp.is_some();
        let layout = ShellLayout::new(
            f32::from(window.viewport_size().width)/scale,
            f32::from(window.viewport_size().height)/scale,
            self.ui.monitor_visible&&!is_rdp,
            self.ui.files_visible&&!is_rdp,
            self.ui.files_position,
            self.ui.files_extent,
        );
        let show_monitor = layout.monitor;
        let show_files = self.ui.files_visible&&!is_rdp;
        if let Some(terminal) = self.workspace.terminal.as_ref().filter(|terminal|terminal.view.is_none()) {
            let width = layout.terminal_width - 48.;
            let height =
                layout.terminal_height - 105. - if self.ui.history_visible { 180. } else { 0. };
            if let Ok(mut screen) = terminal.screen.lock() {
                if screen.resize(
                    (width / (self.ui.font_size * 0.62)).max(20.) as usize,
                    (height / (self.ui.font_size * 1.4)).max(5.) as usize,
                ) {
                    if let Some(sender) = &terminal.input_tx {
                        let _ = sender.send(TerminalInput::Resize(screen.columns, screen.rows));
                    }
                }
            }
        }
        div()
            .id("devshell-workspace")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(gpui_kit::rgb(p.background))
            .text_color(gpui_kit::rgb(p.text))
            .text_size(px(13.))
            .font_family(ui_font)
            .capture_key_down(cx.listener(|state,_,_,_|state.note_activity()))
            .on_mouse_up(gpui_kit::MouseButton::Left, cx.listener(|state, _, _, cx| {
                if state.ui.resizing_files{state.persist_files_layout(cx);}
                state.ui.resizing_files = false;
                if let Some(f)=state.form.as_mut(){f.drag_offset=None;}
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|state, event: &gpui_kit::MouseMoveEvent, window, cx| {
                state.note_activity();
                if let Some(f)=state.form.as_mut(){if let Some((dx,dy))=f.drag_offset {if event.pressed_button==Some(gpui_kit::MouseButton::Left){let scale=crate::ui_scale::current();f.position=Some(((f32::from(event.position.x)/scale-dx).clamp(8.,(f32::from(window.viewport_size().width)/scale-100.).max(8.)),(f32::from(event.position.y)/scale-dy).clamp(8.,(f32::from(window.viewport_size().height)/scale-60.).max(8.))));cx.notify();}}}
                if state.ui.resizing_files && event.pressed_button == Some(gpui_kit::MouseButton::Left) {
                    let value = match state.ui.files_position {
                        FilesPosition::Right => f32::from(window.viewport_size().width - event.position.x)/crate::ui_scale::current() - 12.,
                        FilesPosition::Top => f32::from(event.position.y)/crate::ui_scale::current() - 54.,
                        FilesPosition::Bottom => f32::from(window.viewport_size().height - event.position.y)/crate::ui_scale::current() - 12.,
                    };
                    state.ui.files_extent = Some(value.max(170.));
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|state, event: &gpui_kit::KeyDownEvent, _, cx| {
                if state.snapshot.locked {cx.stop_propagation();return;}
                if event.keystroke.key == "escape" {
                    if state.form.is_some(){state.dismiss_native_form(cx);cx.stop_propagation();return;}
                    if state.workspace.file_ui.menu.take().is_some(){cx.notify();cx.stop_propagation();return;}
                    state.settings_visible = false;
                    state.ui.connections_visible = false;
                    state.ui.about_visible = false;state.ui.pinned_visible=false;state.ui.tab_menu=None;
                    state.ui.history_visible = false;
                    state.ui.utility = None;
                    state.form=None;
                    state.pending_host_key = None;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .h(px(54.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(
                        div()
                            .when(show_monitor, |bar| bar.w(px(304.)).mr(px(4.)))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(8.)).when(!show_monitor,|b|b.pr(px(9.)).mr(px(2.)).border_r_1().border_color(gpui_kit::rgb(p.border)))
                            .child(
                                tool("about", "")
                                    .size(px(28.)).p_0().child(gpui_kit::img("devshell.svg").size(px(26.)))
                                    .on_click(cx.listener(|state, _, _, cx| {
                                        state.ui.about_visible = !state.ui.about_visible;
                                        cx.notify();
                                    })),
                            )
                            .child(icon_tool("settings", "settings", "设置").on_click(cx.listener(
                                |state, _, _, cx| {
                                    state.settings_visible = !state.settings_visible;
                                    state.ui.settings_view = "main";state.ui.settings.preferences=None;
                                    state.ui.connections_visible = false;
                                    cx.notify();
                                },
                            )))
                            .child(icon_tool("github", "github", "GitHub 项目仓库").on_click(|_, _, cx| {
                                cx.open_url("https://github.com/yuhongwei380/DevShell")
                            }))
                            .when(show_monitor,|bar|bar.child(div().flex_1()))
                            .child(
                                icon_tool("theme", if self.ui.dark { "sun" } else { "moon" }, "切换明暗模式").rounded(px(9.)).bg(gpui_kit::rgb(if self.ui.dark{0x42392d}else{0xf8edda})).on_click(
                                    cx.listener(|state, _, window, cx| {
                                        state.ui.dark = !state.ui.dark;
                                        apply_theme(state.ui.dark, Some(window), cx);
                                        state.save_appearance(serde_json::json!({"theme": if state.ui.dark { "dark" } else { "light" }}), cx);
                                        cx.notify();
                                    }),
                                ),
                            )
                            .child(icon_tool("security-status", "lock", "立即锁定").disabled(!self.snapshot.security["enabled"].as_bool().unwrap_or(false)).on_click(cx.listener(|state,_,_,cx|state.security_call("lock",serde_json::json!({}),cx)))),
                    )
                    .child(
                        icon_tool("servers", "server", "服务器管理").size(px(30.)).on_click(cx.listener(|state, _, _, cx| {
                            state.ui.connections_visible = !state.ui.connections_visible;
                            state.settings_visible = false;
                            cx.notify();
                        })),
                    )
                    .child(icon_tool("pinned-connections","bookmark","收藏夹").size(px(30.)).on_click(cx.listener(|state,_,_,cx|{state.ui.pinned_visible=!state.ui.pinned_visible;cx.notify();})))
                    .when(!self.ui.start_tabs.is_empty()||self.workspace.terminal.is_some()||!self.parked.is_empty(),|bar|bar.child(self.session_tabs(p,cx)))
                    .child(
                        icon_tool("add-session", "plus", "新会话").size(px(30.)).on_click(cx.listener(|state, _, _, cx| {
                            state.new_start_tab(cx);
                            cx.notify();
                        })),
                    )
                    .child(div().flex_1().h_full().on_mouse_down(gpui_kit::MouseButton::Left, |_,window,_|window.start_window_move()))
                    .child(div().h(px(30.)).flex().items_center().gap(px(2.))
                        .child(tool("tabs-previous","").w(px(26.)).h(px(30.)).p_0().child(icon("chevron",15.).with_transformation(gpui_kit::Transformation::rotate(gpui_kit::radians(std::f32::consts::PI)))).disabled(self.ui.tabs_scroll.offset().x>=gpui_kit::px(0.)).on_click(cx.listener(|state,_,_,cx|{let offset=state.ui.tabs_scroll.offset();state.ui.tabs_scroll.set_offset(gpui_kit::point((offset.x+px(160.)).min(gpui_kit::px(0.)),gpui_kit::px(0.)));cx.notify();})))
                        .child(tool("tabs-next","").w(px(26.)).h(px(30.)).p_0().child(icon("chevron",15.)).disabled(self.ui.tabs_scroll.max_offset().x<=-self.ui.tabs_scroll.offset().x).on_click(cx.listener(|state,_,_,cx|{let offset=state.ui.tabs_scroll.offset();state.ui.tabs_scroll.set_offset(gpui_kit::point(offset.x-px(160.),gpui_kit::px(0.)));cx.notify();})))
                        .child(tool("tabs-list","").w(px(26.)).h(px(30.)).p_0().child(icon("chevron",15.).with_transformation(gpui_kit::Transformation::rotate(gpui_kit::radians(std::f32::consts::FRAC_PI_2)))).disabled(self.workspace.terminal.is_none()&&self.parked.is_empty()&&self.ui.start_tabs.is_empty()).on_click(cx.listener(|state,_,_,cx|{state.ui.pinned_visible=false;state.ui.tab_list_visible=!state.ui.tab_list_visible;cx.notify();}))))
                    .when(f32::from(window.viewport_size().width)/crate::ui_scale::current()<1000.,|bar|bar.child(icon_tool("monitor-toggle","cpu","显示或隐藏监控").on_click(cx.listener(|state,_,_,cx|{state.ui.monitor_visible=!state.ui.monitor_visible;state.save_appearance(serde_json::json!({"monitorVisible":state.ui.monitor_visible}),cx);cx.notify();}))))
                    .child(div().h(px(32.)).ml(px(4.)).px(px(2.)).py(px(2.)).flex().items_center().gap(px(3.)).rounded(px(11.)).bg(if self.ui.dark{gpui_kit::rgba(0x131a21ff)}else{gpui_kit::rgba(0xa7bed218)})
                    .child(
                        icon_tool("window-minimize", "minimise", "最小化").w(px(34.))
                            .on_click(|_, window, _| window.minimize_window()),
                    )
                    .child(
                        icon_tool("window-maximize", "maximise", "最大化 / 还原").w(px(34.)).on_click(|_, window, _| window.zoom_window()),
                    )
                    .child(
                        icon_tool("window-close", "close", "关闭窗口").w(px(34.)).on_click(cx.listener(|state, _, window, cx| state.request_window_close(window,cx))),
                    )
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px_3()
                    .pb(px(12.))
                    .flex()
                    .gap_3()
                    .when(show_monitor, |body| body.child(self.original_monitor_sidebar(p,cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .when(layout.files_right, |body| body.flex_row())
                            .when(show_files && self.ui.files_position == FilesPosition::Top, |body| {
                                body.child(self.files_sidebar(p, &layout, cx)).child(self.files_splitter(p, false, cx))
                            })
                            .child(if is_rdp {self.rdp_workspace(p,cx)}else{self.terminal_workspace(p,cx)})
                            .when(show_files && self.ui.files_position != FilesPosition::Top, |body| {
                                body.child(self.files_splitter(p, layout.files_right, cx)).child(self.files_sidebar(p, &layout, cx))
                            })

                    ),
            )
            .when_some(self.feedback.clone(), |root,message|root.child(
                div().absolute().bottom(px(16.)).left(px(340.)).right(px(24.)).px_3().py_2().rounded_lg().bg(gpui_kit::rgb(p.surface)).border_1().border_color(gpui_kit::rgb(p.border)).text_size(px(11.)).child(message)
            ))
            .when(
                self.ui.connections_visible
                    || self.ui.pinned_visible
                    || self.settings_visible
                    || self.ui.about_visible
                    || self.pending_host_key.is_some()
                    || self.form.is_some()
                    || self.ui.settings.keys_visible
                    || self.ui.utility.is_some()
                    || self.ui.close_visible,
                |root| {
                    root.child(
                        div()
                            .absolute()
                            .inset_0()
                            .bg(gpui_kit::rgba(if self.settings_visible && !self.ui.connections_visible && self.ui.utility.is_none() {0x00000000} else if self.ui.settings.keys_visible||self.form.as_ref().is_some_and(|f|matches!(f.kind,"file-action"|"file-upload"|"key"|"key-public"|"delete-key"|"ansi"|"security")){0x1f354655}else if self.ui.dark { 0x00000066 } else { 0x172b4033 }))
                            .occlude()
                            .on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(|state, _, _, cx| {
                                state.settings_visible = false;state.ui.pinned_visible=false;
                                state.ui.connections_visible = false;
                                state.ui.about_visible = false;
                                state.ui.close_visible = false;
                                            state.form=None;
                                state.ui.settings.keys_visible=false;
                                state.ui.utility = None;
                                cx.notify();
                            })),
                    )
                },
            )
            .when(self.ui.pinned_visible,|root|root.child(self.pinned_menu(p,cx)))
            .when(self.ui.tab_list_visible,|root|root.child(self.tabs_list(p,cx)))
            .when(self.ui.tab_menu.is_some(),|root|root.child(self.tab_context_menu(p,window,cx)))
            .when(self.ui.connections_visible, |root| {
                root.child(self.server_manager(p,window,cx))
            })
            .when(self.settings_visible, |root| {
                root.child(self.settings_dialog(p, cx))
            })
            .when(self.ui.about_visible, |root| {
                root.child(self.about_dialog(p, cx))
            })
            .when(self.ui.utility.is_some(),|root|root.child(self.utility_dialog(p,cx)))
            .when(self.ui.settings.keys_visible,|root|root.child(self.original_keys_manager(p,cx)))
            .when(self.ui.manager.menu.is_some(),|root|root.child(self.server_context_menu(p,window,cx)))
            .when(self.workspace.charts.hover.is_some(),|root|root.child(self.monitor_chart_tooltip(p,window)))
            .when(self.workspace.file_ui.menu.is_some(),|root|root.child(self.file_context_menu(p,window,cx)))
            .when(self.form.is_some(),|root|root.child(self.native_form_dialog(p,window,cx)))
            .when_some(self.workspace.preview.clone(),|root,preview|root.child(file_preview_panel(p,preview,self.preview_input.clone(),cx.entity())))
            .when(self.ui.close_visible&&!self.snapshot.locked, |root| {
                root.child(
                    overlay(
                        p,
                        "退出 DevShell",
                        tool("close-cancel", "取消").on_click(cx.listener(|state, _, _, cx| {
                            state.ui.close_visible = false;
                            cx.notify();
                        })),
                    )
                    .child(
                        div()
                            .p_6()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child("退出会断开 SSH 会话并中止传输，请先保存文件。")
                            .child(
                                KitButton::new("close-confirm")
                                    .danger()
                                    .label("退出")
                                    .on_click(|_, _, cx| cx.quit()),
                            ),
                    ),
                )
            })
            .when_some(self.pending_host_key.clone(), |root, pending| {
                root.child(
                    overlay(
                        p,
                        "主机指纹确认",
                        tool("host-key-close", "取消").on_click(cx.listener(|state, _, _, cx| {
                            state.pending_host_key = None;
                            cx.notify();
                        })),
                    )
                    .child(
                        div()
                            .p_5()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(paragraph(format!(
                                "请核实 {} 的主机指纹后确认连接。",
                                pending.challenge.host
                            )))
                            .child(paragraph(format!(
                                "{} · {}",
                                pending.challenge.algorithm, pending.challenge.fingerprint
                            )))
                            .when(
                                !pending.challenge.previous_fingerprint.is_empty(),
                                |panel| {
                                    panel.child(paragraph(format!(
                                        "原指纹：{}",
                                        pending.challenge.previous_fingerprint
                                    )))
                                },
                            )
                            .child(KitButton::new("host-key-confirm").primary().label("确认指纹并连接").disabled(self.connecting.is_some()).on_click(cx.listener(|state,_,_,cx|{
                                if let Some(pending)=state.pending_host_key.clone(){if let Some(server)=state.find_profile(&pending.profile_id){state.connect_profile_with_secret(server,pending.secret,cx);}}
                            }))),
                    ),
                )
            })
            .when(self.snapshot.locked,|root|root.child(self.lock_screen(p,window,cx)))
    }
}

impl DevShellWindow {
    pub(super) fn save_appearance(&mut self, patch: Value, cx: &mut Context<Self>) {
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { post_json(&base, &token, "/api/appearance/patch", patch) })
                .await;
            let _ = this.update(cx, |state, cx| {
                match result {
                    Ok(value) => {
                        state.snapshot.appearance =
                            value.get("appearance").cloned().unwrap_or(value)
                    }
                    Err(error) => state.feedback = Some(format!("保存外观失败：{error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn session_tabs(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let active_id = self
            .workspace
            .terminal
            .as_ref()
            .map(|t| t.session_id.as_str());
        let mut terminals = self
            .parked
            .iter()
            .filter_map(|session| session.terminal.as_ref())
            .collect::<Vec<_>>();
        if let Some(terminal) = self.workspace.terminal.as_ref() {
            terminals.push(terminal);
        }
        terminals.sort_by_key(|terminal| {
            self.ui
                .session_order
                .iter()
                .position(|id| id == &terminal.session_id)
        });
        div()
            .id("session-tabs")
            .max_w(gpui_kit::relative(0.65))
            .min_w_0()
            .flex()
            .gap(px(8.))
            .overflow_x_scroll()
            .track_scroll(&self.ui.tabs_scroll)
            .children(self.ui.start_tabs.iter().map(|&index| {
                div()
                    .id(format!("start-tab-{index}"))
                    .h(px(35.))
                    .min_w(px(148.))
                    .rounded(px(10.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(self.ui.active_start == Some(index), |tab| {
                        tab.bg(gpui_kit::rgb(p.surface))
                    })
                    .child(
                        tool(format!("start-select-{index}"), format!("新选项卡 {index}"))
                            .on_click(cx.listener(move |state, _, _, cx| {
                                state.activate_start_tab(index, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        icon_tool(format!("start-close-{index}"), "close", "关闭选项卡").on_click(
                            cx.listener(move |state, _, _, cx| {
                                state.ui.start_tabs.retain(|&id| id != index);
                                if state.ui.active_start == Some(index) {
                                    state.ui.active_start = state.ui.start_tabs.last().copied();
                                    if state.ui.active_start.is_none() {
                                        state.workspace = state.parked.pop().unwrap_or_default();
                                    }
                                }
                                cx.notify();
                            }),
                        ),
                    )
            }))
            .children(terminals.into_iter().map(|terminal| {
                let id = terminal.session_id.clone();
                let close_id = id.clone();
                let menu_id=id.clone();let drop_id=id.clone();let drag=crate::sessions::TabDrag(id.clone(),terminal.name.clone());
                let active = active_id == Some(id.as_str());
                div()
                    .id(SharedString::from(format!("tab-{id}")))
                    .pl(px(9.))
                    .pr(px(7.))
                    .h(px(33.))
                    .max_w(px(if active {300.} else {240.}))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(gpui_kit::rgb(if self.ui.dark { if active {0x6b899a} else {0x415865} } else if active {0x86a7bb} else {0xafc1ce}))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .flex_shrink_0()
                    .bg(gpui_kit::rgb(if self.ui.dark {if active {0x2c414f} else {0x233540}} else if active {0xf8fbfd} else {0xe7eef4}))
                    .text_color(gpui_kit::rgb(if self.ui.dark {if active {0xe1edf4} else {0xa4bac8}} else if active {0x2e5269} else {0x5a7284}))
                    .cursor_pointer()
                    .on_drag(drag,|drag,_,_,cx|cx.new(|_|drag.clone()))
                    .on_drop(cx.listener(move|state,drag:&crate::sessions::TabDrag,_,cx|{if drag.0!=drop_id {if let Some(from)=state.ui.session_order.iter().position(|id|id==&drag.0){let id=state.ui.session_order.remove(from);if let Some(to)=state.ui.session_order.iter().position(|id|id==&drop_id){state.ui.session_order.insert(to,id);}else{state.ui.session_order.push(id);}}}cx.notify();}))
                    .on_mouse_down(gpui_kit::MouseButton::Right,cx.listener(move|state,event:&gpui_kit::MouseDownEvent,_,cx|{state.ui.tab_menu=Some((menu_id.clone(),event.position));cx.notify();}))
                    .on_click(cx.listener(move |state, _, _, cx| {
                        state.switch_session(&id, cx);
                        cx.notify();
                    }))
                    .child(div().size(px(8.)).flex_shrink_0().mr(px(4.)).rounded_full().bg(gpui_kit::rgb(
                        if terminal.input_tx.is_some() || (terminal.protocol=="rdp"&&terminal.status=="connected") {
                            0x6bba91
                        } else {
                            0xcc716d
                        },
                    )))
                    .child(div().min_w_0().overflow_hidden().text_size(px(12.)).font_weight(if active {gpui_kit::FontWeight::SEMIBOLD} else {gpui_kit::FontWeight::MEDIUM}).child(terminal.name.clone()))
                    .child(
                        icon_tool(format!("close-{close_id}"), "close", "关闭选项卡").size(px(23.)).on_click(
                            cx.listener(move |state, _, _, cx| {
                                cx.stop_propagation();
                                let view = cx.entity();
                                if state
                                    .session_mut(&close_id)
                                    .and_then(|s| s.terminal.as_ref())
                                    .is_some_and(|t| t.input_tx.is_some() || t.protocol=="rdp")
                                {
                                    state.ui.closing_sessions.push(close_id.clone());
                                    disconnect_session(view, close_id.clone(), cx);
                                } else {
                                    state.parked.retain(|session| {
                                        !session
                                            .terminal
                                            .as_ref()
                                            .is_some_and(|t| t.session_id == close_id)
                                    });
                                    if state
                                        .workspace
                                        .terminal
                                        .as_ref()
                                        .is_some_and(|t| t.session_id == close_id)
                                    {
                                        state.workspace = state.parked.pop().unwrap_or_default();
                                        if state.workspace.terminal.is_none() {
                                            state.ui.active_start =
                                                state.ui.start_tabs.last().copied();
                                        }
                                    }
                                    cx.notify();
                                }
                            }),
                        ),
                    )
            }))
    }

    fn new_start_tab(&mut self, cx: &mut Context<Self>) {
        if self.snapshot.appearance["localShellEnabled"].as_bool().unwrap_or(false){self.start_auxiliary("local",serde_json::json!({}),cx);return;}
        let index = self.ui.next_start;
        self.ui.next_start += 1;
        self.ui.start_tabs.push(index);
        self.activate_start_tab(index, cx);
    }

    pub(super) fn activate_start_tab(&mut self, index: u64, cx: &App) {
        if let Some(preview) = &mut self.workspace.preview {
            preview.text = self.preview_input.read(cx).text().to_string();
        }
        self.park_session();
        self.workspace = SessionWorkspace::default();
        self.ui.active_start = Some(index);
    }

    fn terminal_workspace(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let terminal = self.workspace.terminal.clone();
        let connected = terminal.as_ref().is_some_and(|t| t.input_tx.is_some());
        let title = terminal
            .as_ref()
            .map(|t| t.host.clone())
            .unwrap_or_else(|| "SSH 终端".into());
        div().rounded(px(16.))
            .bg(if self.ui.dark {gpui_kit::Background::from(gpui_kit::rgb(p.terminal))}else{gpui_kit::linear_gradient(115.,gpui_kit::linear_color_stop(gpui_kit::rgb(0x273f50),0.),gpui_kit::linear_color_stop(gpui_kit::rgb(0x203342),1.))})
            .text_color(gpui_kit::rgb(0xd4e1e8))
            .flex_1()
            .min_h_0()
            .min_w_0()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .when(!self.ui.concise && terminal.is_some(), |body|body.child(
                div()
                    .h(px(38.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(gpui_kit::rgb(p.terminal_meta))
                    .border_b_1()
                    .border_color(gpui_kit::rgb(p.border))
                    .child(
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(gpui_kit::rgb(if connected { 0x6bba91 } else { 0xcc716d })),
                    )
                    .child(title)
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui_kit::rgb(p.muted))
                            .child("UTF-8 │ xterm-256color"),
                    )
                    .child(
                        icon_tool("disconnect", "disconnect", "断开连接，保留终端内容")
                            .disabled(!connected)
                            .on_click(cx.listener(|state, _, _, cx| {
                                if let Some(id) = state
                                    .workspace
                                    .terminal
                                    .as_ref()
                                    .map(|t| t.session_id.clone())
                                {
                                    disconnect_session(cx.entity(), id, cx);
                                }
                            })),
                    )
                    .child(
                        icon_tool("clear-terminal", "erase", "清除终端滚屏")
                            .disabled(terminal.is_none())
                            .on_click(cx.listener(|state, _, _, cx| {
                                if let Some(terminal) = &mut state.workspace.terminal {
                                    if let Ok(mut screen) = terminal.screen.lock() {screen.clear();}
                                    if let Some(view)=&terminal.view {view.update(cx,|term,cx|term.clear(cx));}
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        tool("toggle-sftp", "SFTP")
                            .text_color(gpui_kit::rgb(0x8ca8bb))
                            .child(icon("folder", 14.))
                            .on_click(cx.listener(|state, _, _, cx| {
                                state.ui.files_visible = !state.ui.files_visible;state.persist_files_layout(cx);
                                cx.notify();
                            })),
                    ),
            ))
            .when(self.ui.terminal_search_visible,|body|body.child(div().h(px(36.)).px_2().flex_shrink_0().flex().items_center().gap_2().bg(gpui_kit::rgb(p.terminal_meta))
                .child(div().flex_1().child(Input::new(&self.ui.terminal_search).small()))
                .children([(false,"↑"),(true,"↓")].map(|(forward,label)|tool(format!("terminal-find-{forward}"),label).on_click(cx.listener(move|state,_,_,cx|{if let Some(view)=state.workspace.terminal.as_ref().and_then(|t|t.view.clone()){view.update(cx,|term,cx|term.step(forward,cx));}}))))
                .child(icon_tool("terminal-find-close","close","关闭搜索").on_click(cx.listener(|state,_,_,cx|{state.ui.terminal_search_visible=false;if let Some(view)=state.workspace.terminal.as_ref().and_then(|t|t.view.clone()){view.update(cx,|term,cx|{let _=term.find("",Default::default(),cx);});}cx.notify();})))))
            .child(
                div()
                    .id("terminal-output")
                    .track_focus(&self.ui.terminal_focus)
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .p_0()
                    .when(terminal.is_some(), |d| d.px(px(if self.ui.viewport.0 <= 1100. {12.} else {20.})).pt(px(5.)).pb(px(8.)))
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|state, _, window, cx| {
                            if state.workspace.terminal.as_ref().is_none_or(|t|t.view.is_none()) {window.focus(&state.ui.terminal_focus, cx)}
                        }),
                    )
                    .on_key_down(cx.listener(|state, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if state.workspace.terminal.as_ref().is_some_and(|t|t.view.is_some()) {return;}
                        if let Some(sender) = state
                            .workspace
                            .terminal
                            .as_ref()
                            .and_then(|t| t.input_tx.as_ref())
                        {
                            let key = &event.keystroke;
                            let text = if key.modifiers.control && key.key == "v" {
                                cx.read_from_clipboard().and_then(|item| item.text())
                            } else {
                                screen::key_input(key)
                            };
                            if let Some(text) = text {
                                let _ = sender.send(TerminalInput::Text(text));
                                cx.stop_propagation();
                            }
                        }
                    }))
                    .text_color(gpui_kit::rgb(0xd3e0e9))
                    .text_size(px(self.ui.font_size))
                    .line_height(gpui_kit::relative(1.4))
                    .font_family("Consolas")
                    .when_some(terminal.clone(), |body, terminal| {
                        if let Some(view)=terminal.view {body.child(view)} else {body.child(ely_gpui_component::terminal::AnsiText::new(terminal.screen.lock().map(|screen|screen.ansi()).unwrap_or_default()))}
                    })
                    .when(terminal.is_none(), |body| {
                        body.child(
                            div()
                                .size_full()
                                .font_family("Alibaba PuHuiTi 3.0")
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_0()
                                .child(div().flex().gap(px(0.5)).pr(px(0.5)).text_size(px(18.)).line_height(px(28.8)).mb(px(6.)).font_weight(gpui_kit::FontWeight(450.)).children("你的服务器工作台".chars().map(|ch|div().child(ch.to_string()))))
                                .child(div().text_size(px(12.)).line_height(px(19.2)).mb(px(12.)).text_color(gpui_kit::rgb(0x91a9b8)).child("连接服务器，终端、文件与运行状态尽在眼前。"))
                                .child(KitButton::new("welcome-quick").ghost().h(px(39.2)).w(px(83.234)).p_0().mb(px(14.)).rounded(px(10.)).bg(gpui_kit::rgba(0x769bb33b)).text_color(gpui_kit::rgb(0xc4dae7)).child(div().text_size(px(12.)).line_height(px(19.2)).child("快速连接")).on_click(cx.listener(|state,_,window,cx|state.open_native_form("quick-connect",serde_json::json!({}),window,cx))))
                                .child(KitButton::new("welcome-connect").ghost().h(px(39.2)).w(px(157.469)).p_0().rounded(px(10.)).bg(gpui_kit::rgba(0x769bb33b)).text_color(gpui_kit::rgb(0xc4dae7)).child(div().flex().items_center().gap(px(14.)).text_size(px(12.)).line_height(px(19.2)).child("添加或选择服务器").child(icon("chevron",13.).text_color(gpui_kit::rgb(0xc4dae7)))).on_click(cx.listener(|state,_,_,cx|{state.ui.connections_visible=true;cx.notify();})))

                        )
                    }),
            )
            .when(self.ui.history_visible, |body| {
                body.child(
                    div()
                        .id("history-popover")
                        .max_h(px(180.))
                        .overflow_y_scroll()
                        .mx_3()
                        .p_3()
                        .rounded_lg()
                        .bg(gpui_kit::rgb(p.terminal_meta))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_size(px(11.)).child("历史命令"))
                        .when(self.workspace.command_history.is_empty(), |panel| {
                            panel.child("暂无历史命令")
                        })
                        .children(
                            self.workspace
                                .command_history
                                .iter()
                                .rev()
                                .take(30)
                                .enumerate()
                                .map(|(index, command)| {
                                    let command = command.clone();
                                    tool(format!("history-{index}"), command.clone()).on_click(
                                        cx.listener(move |state, _, window, cx| {
                                            state.command_input.update(cx, |input, cx| {
                                                input.set_value(command.clone(), window, cx)
                                            });
                                            state.ui.history_visible = false;
                                            cx.notify();
                                        }),
                                    )
                                }),
                        ),
                )
            })
            .when(self.ui.command_visible,|body|body.child(
                div()
                    .mx(px(12.))
                    .mb(px(8.))
                    .mt(px(3.))
                    .px_2()
                    .h(px(44.))
                    .rounded(px(11.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(gpui_kit::rgb(p.terminal_meta))
                    .child(
                        icon_tool("command-toggle", "terminal", "展开 / 收起命令输入条").on_click(
                            cx.listener(|state, _, _, cx| {
                                state.ui.command_visible = !state.ui.command_visible;
                                cx.notify();
                            }),
                        ),
                    )
                    .when(self.ui.command_visible, |bar| {
                        bar.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .capture_key_down(cx.listener(
                                    |state, event: &gpui_kit::KeyDownEvent, window, cx| {
                                        if matches!(event.keystroke.key.as_str(), "up" | "down")
                                            && !state.workspace.command_history.is_empty()
                                        {
                                            let count = state.workspace.command_history.len();
                                            if state.workspace.history_cursor.is_none() {
                                                state.workspace.history_draft = state
                                                    .command_input
                                                    .read(cx)
                                                    .value()
                                                    .to_string();
                                            }
                                            let index = match (
                                                event.keystroke.key.as_str(),
                                                state.workspace.history_cursor,
                                            ) {
                                                ("up", Some(i)) => i.saturating_sub(1),
                                                ("up", None) => count - 1,
                                                (_, Some(i)) => (i + 1).min(count),
                                                _ => count,
                                            };
                                            state.workspace.history_cursor =
                                                if index == count { None } else { Some(index) };
                                            let text = if index == count {
                                                state.workspace.history_draft.clone()
                                            } else {
                                                state.workspace.command_history[index].clone()
                                            };
                                            state.command_input.update(cx, |input, cx| {
                                                input.set_value(text, window, cx)
                                            });
                                            cx.stop_propagation();
                                            cx.notify();
                                        }
                                    },
                                ))
                                .child(
                                    Input::new(&self.command_input)
                                        .appearance(false)
                                        .bordered(false)
                                        .small(),
                                ),
                        )
                    })
                    .child(
                        icon_tool("command-history", "history", "历史命令").on_click(cx.listener(
                            |state, _, _, cx| {
                                state.ui.history_visible = !state.ui.history_visible;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        tool("font-down", "A−").on_click(cx.listener(|state, _, _, cx| {
                            state.ui.font_size = (state.ui.font_size - 0.5).max(8.);
                            state.save_appearance(
                                serde_json::json!({"terminalFontSize": state.ui.font_size}),
                                cx,
                            );
                            cx.notify();
                        })),
                    )
                    .child(
                        icon_tool("font-up", "font-increase", "放大终端字体 0.5 px").on_click(
                            cx.listener(|state, _, _, cx| {
                                state.ui.font_size = (state.ui.font_size + 0.5).min(40.);
                                state.save_appearance(
                                    serde_json::json!({"terminalFontSize": state.ui.font_size}),
                                    cx,
                                );
                                cx.notify();
                            }),
                        ),
                    ),
            ))
            .when(!self.ui.command_visible,|body|body.child(div().flex_shrink_0().ml(px(12.)).mt(px(3.)).mb(px(8.)).w(px(47.)).h(px(34.)).pl(px(11.)).pr(px(8.)).py(px(3.)).rounded(px(11.)).bg(if self.ui.dark{gpui_kit::Hsla::from(gpui_kit::rgb(p.terminal_meta))}else{gpui_kit::rgba(0x192d3be0).into()}).child(icon_tool("command-expand","terminal","展开命令输入条").on_click(cx.listener(|state,_,_,cx|{state.ui.command_visible=true;cx.notify();})))))
    }

    pub(super) fn send_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.command_input.read(cx).value().to_string();
        if text.is_empty() {
            return;
        }
        if self.workspace.terminal.as_ref().is_none_or(|t|t.input_tx.is_none()) {
            if text.trim_start().starts_with("ssh "){self.quick_connect_command(text,"password".into(),String::new(),String::new(),cx);}
            else if let Some((host,port))=crate::sessions::tcp_target(&text){if self.snapshot.appearance["tcpToolsEnabled"].as_bool().unwrap_or(true){self.start_auxiliary("tcp",serde_json::json!({"host":host,"port":port}),cx);}}
            self.command_input.update(cx,|input,cx|input.set_value("",window,cx));return;
        }
        if let Some(sender) = self
            .workspace
            .terminal
            .as_ref()
            .and_then(|terminal| terminal.input_tx.as_ref())
        {
            if sender
                .send(TerminalInput::Text(format!("{text}\n")))
                .is_ok()
            {
                self.workspace.integration.at_prompt=false;
                self.command_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
            }
        }
    }

    fn files_splitter(
        &self,
        p: Palette,
        vertical: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("files-splitter")
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .when(vertical, |s| s.w(px(8.)).h_full())
            .when(!vertical, |s| s.h(px(8.)).w_full().cursor_row_resize())
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(move|state, _, _, cx| {
                    state.ui.resizing_files = !vertical;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .rounded_full()
                    .bg(gpui_kit::rgb(p.border))
                    .w(px(if vertical { 0. } else { 30. }))
                    .h(px(if vertical { 30. } else { 3. })),
            )
    }

    fn files_sidebar(&self, p: Palette, layout: &ShellLayout, cx: &mut Context<Self>) -> Div {
        let view = cx.entity();
        card(p).on_drop::<gpui_kit::ExternalPaths>(cx.listener(|state,paths:&gpui_kit::ExternalPaths,_,cx|{if let (Some(terminal),Some(files))=(&state.workspace.terminal,&state.workspace.files){let owner=terminal.session_id.clone();let directory=files.path.clone();state.queue_local_selection(owner,directory,paths.paths().to_vec(),cx);}}))
            .when(layout.files_right, |panel| {
                panel.w(px(layout.files_extent)).h_full()
            })
            .when(!layout.files_right, |panel| {
                panel.h(px(layout.files_extent)).w_full()
            })
            .flex_shrink_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .min_h(px(36.)).py(px(5.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(gpui_kit::rgb(p.border))
                    .gap_1()
                    .children(
                        [
                            ("files", "folder", "文件"),
                            ("commands", "code", "命令"),
                            ("apps", "layout", "常用应用"),
                            ("transfers", "upload", "传输任务"),
                        ]
                        .map(|(pane, image, label)| {
                            KitButton::new(format!("files-tab-{pane}")).ghost().child(div().flex().items_center().gap(px(7.)).child(icon(image,14.)).child(div().text_size(px(11.)).line_height(px(15.4)).child(label)))
                                .h(px(26.)).px(px(9.)).rounded(px(7.)).when(self.ui.files_pane == pane, |tab| tab.bg(gpui_kit::rgb(p.inset)))
                                .on_click(cx.listener(move |state, _, _, cx| {
                                    state.ui.files_pane = pane;
                                    cx.notify();
                                }))
                        }),
                    )
                    .child(div().flex_1())
                    .child(
                        icon_tool("files-close", "close", "关闭 SFTP 面板").on_click(cx.listener(
                            |state, _, _, cx| {
                                state.ui.files_visible = false;state.persist_files_layout(cx);
                                cx.notify();
                            },
                        )),
                    ).when(!layout.files_right&&self.ui.viewport.0>1100.,|d|d.child(div().text_size(px(9.)).text_color(gpui_kit::rgb(p.muted)).child("拖入文件或文件夹上传"))),
            )
            .child(
                div()
                    .id("files-scroll")
                    .flex_1()
                    .min_h_0()
                    .when(self.ui.files_pane!="files",|body|body.overflow_y_scroll().p_2())
                    .when(self.ui.files_pane == "transfers", |body| {
                        body.child(transfer_panel(
                            p,
                            self.workspace.transfers.clone(),
                            view.clone(),
                        ))
                    })
                    .when(self.ui.files_pane == "commands", |body| {
                        body.child(self.quick_commands(p, cx))
                    })
                    .when(self.ui.files_pane == "apps", |body| {
                        body.child(self.common_apps(p, cx))
                    })
                    .when(self.ui.files_pane == "files", |body|body.child(self.original_file_browser(p,if layout.files_right{layout.files_extent}else{layout.terminal_width},cx))),
            )
    }

    fn settings_dialog(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        if matches!(self.ui.settings_view, "proxy" | "updates") {
            return overlay(
                p,
                if self.ui.settings_view == "proxy" {
                    "代理管理器"
                } else {
                    "更新日志与检查更新"
                },
                icon_tool("settings-close", "close", "关闭设置").on_click(cx.listener(
                    |state, _, _, cx| {
                        state.settings_visible = false;
                        cx.notify();
                    },
                )),
            )
            .child(
                div()
                    .id("settings-scroll")
                    .max_h(px(520.))
                    .overflow_y_scroll()
                    .p_3()
                    .child(settings_panel(
                        p,
                        self.snapshot.clone(),
                        cx.entity(),
                        self.update_status.clone(),
                        self.update_hash.clone(),
                        self.update_checking,
                        self.update_download.clone(),
                        self.proxy_form_open,
                        self.proxy_saving,
                        self.proxy_name_input.clone(),
                        self.proxy_host_input.clone(),
                        self.proxy_port_input.clone(),
                        self.proxy_user_input.clone(),
                        self.proxy_password_input.clone(),
                        self.proxy_kind.clone(),
                        self.pending_proxy_delete.clone(),
                        self.proxy_editing_id.clone(),
                        self.ui.settings_view,
                    )),
            );
        }
        let section = self.ui.settings_view;
        div().rounded(px(16.)).bg(gpui_kit::rgb(p.surface)).occlude().absolute().top(px(47.)).left(px(48.)).w(px(282.)).shadow_lg().p(px(6.)).flex().flex_col().gap_0()
            .when(section=="main",|body|body.child(div().px(px(10.)).pt(px(6.)).pb(px(8.)).text_size(px(10.)).line_height(px(14.)).text_color(gpui_kit::rgb(p.muted)).child("DevShell 设置")))
            .when(section!="main",|body|body.child(div().px(px(8.)).pt(px(5.)).pb(px(11.)).mb(px(5.)).border_b_1().border_color(gpui_kit::rgb(p.border)).flex().items_center().gap(px(10.)).child(tool("settings-back","‹ 返回设置").on_click(cx.listener(|state,_,_,cx|{state.ui.settings_view="main";cx.notify();}))).child(div().text_size(px(12.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child(if section=="appearance"{"外观配置"}else{"其他功能"}))))
            .when(section=="main",|body|body
                .child(self.setting_entry("security","lock","安全锁定","密码与 2FA",cx))
                .child(self.setting_entry("keys","key","密钥管理器","私钥与公钥",cx))
                .child(self.setting_entry("appearance","sun","外观配置","字体与布局",cx))
                .child(self.setting_entry("proxy","network","代理管理器","连接路径",cx))
                .child(self.setting_entry("other","settings","其他功能","",cx))
                .child(self.setting_entry("help","help-book","使用手册与功能引导","帮助",cx))
                .child(self.setting_entry("updates","history","更新日志","当前版本",cx))
                .child(self.setting_entry("config","folder","连接配置文档","升级与备份",cx))
                .child(self.preference_row("concise","简洁模式"))
                .child(self.preference_row("monitorVisible","监控面板"))
                .child(self.preference_row("filesPosition","SFTP面板"))
                .child(self.preference_row("followTerminal","跟随终端目录"))
                .child(self.preference_row("uiScale","界面缩放"))
                .child(self.setting_entry("updates","refresh","检查更新","手动",cx).mt(px(4.)))
                .child(self.setting_entry("mirror","network","更新加速域名","GitHub 加速",cx)))
            .when(section=="appearance",|body|body
                .child(self.setting_entry("ui-fonts","type","界面字体与文字颜色","界面字体",cx))
                .child(self.setting_entry("shell-fonts","type","Shell 字体与颜色","终端字体",cx))
                .child(self.setting_entry("ui-style","layout","全局主题","整套界面材质",cx))
                .child(self.setting_entry("charts","network","曲线样式","网速与延迟",cx))
                .child(self.setting_entry("ansi","swatch","配色方案","导入与管理 iTerm 配色",cx))
                .child(self.setting_entry("prompt-colors","terminal","提示符颜色","",cx))
                .child(tool("layout-reset","恢复默认布局").on_click(cx.listener(|state,_,_,cx|{state.ui.monitor_visible=true;state.ui.files_position=FilesPosition::Right;state.ui.files_extent=None;state.ui.font_size=14.;state.ui.concise=false;state.ui.settings.preferences=None;state.save_appearance(serde_json::json!({"monitorVisible":true,"filesPosition":"right","terminalFontSize":14,"layout":{"conciseMode":false}}),cx);cx.notify();}))))
            .when(section=="other",|body|body.children([("autoCommandsEnabled","连接后自动执行命令",true),("localShellEnabled","本机终端（macOS）",false),("tcpToolsEnabled","候选栏 TCP 工具",true)].map(|(key,label,default)|{
                let enabled=self.snapshot.appearance.get(key).and_then(Value::as_bool).unwrap_or(default);
                div().px_2().py_2().flex().items_center().justify_between().child(label)
                    .child(tool(format!("feature-{key}"),if enabled {"开启"} else {"关闭"}).on_click(cx.listener(move|state,_,_,cx|{state.snapshot.appearance[key]=Value::Bool(!enabled);let mut patch=serde_json::Map::new();patch.insert(key.into(),Value::Bool(!enabled));state.save_appearance(Value::Object(patch),cx);cx.notify();})))
            })))
            .when(section=="security",|body|body.child(div().p_3().text_size(px(12.)).child(format!("安全锁定：{}",self.snapshot.security_summary.as_deref().unwrap_or("未知")))))
            .when(section=="keys",|body|body
                .child(div().p_3().text_size(px(12.)).child("连接可选择私钥文件，或使用 SSH Agent 登录。"))
                .children(self.snapshot.keys.iter().enumerate().map(|(index,key)| {
                    let public=key.get("publicKey").and_then(Value::as_str).unwrap_or_default().to_owned();
                    div().p_2().flex().flex_col().gap_1().text_size(px(11.))
                        .child(key.get("name").and_then(Value::as_str).unwrap_or("私钥").to_owned())
                        .child(div().text_size(px(10.)).child(key.get("fingerprint").and_then(Value::as_str).unwrap_or_default().to_owned()))
                        .child(tool(format!("key-copy-{index}"),"复制公钥").on_click(move|_,_,cx|cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(public.clone()))))
                })))
            .when(section=="help",|body|body.child(div().p_3().text_size(px(12.)).flex().flex_col().gap_3()
                .child("从服务器管理新建或选择连接；标签可切换会话。")
                .child("SFTP 按钮展开文件面板，双击目录进入、双击文件打开。")
                .child("命令输入条回车发送；终端支持 Ctrl+V 粘贴。")
                .child(tool("help-repository","查看项目文档").on_click(|_,_,cx|cx.open_url("https://github.com/yuhongwei380/DevShell")))))
    }

    pub(super) fn setting_entry(
        &self,
        section: &'static str,
        image: &str,
        label: &str,
        detail: &str,
        cx: &mut Context<Self>,
    ) -> KitButton {
        tool(format!("setting-{section}-{label}"), "").w_full().h(px(35.)).px(px(10.)).py(px(9.)).rounded(px(10.))
            .child(div().size_full().flex().items_center().gap(px(10.)).child(icon(image,16.).text_color(gpui_kit::rgb(Palette::new(self.ui.dark).muted)))
                .child(div().when(section!="other",|body|body.flex_1()).text_size(px(12.)).line_height(px(17.)).child(label.to_owned()))
                .when(!detail.is_empty(),|body|body.child(div().text_size(px(10.)).line_height(px(14.)).text_color(gpui_kit::rgb(Palette::new(self.ui.dark).muted)).child(detail.to_owned())))
                .when(matches!(section,"appearance"|"other"),|body|body.child(icon("chevron",14.).ml(px(-5.)))))
            .on_click(cx.listener(move |state, _, window, cx| {
                match section {"config"=>{state.open_native_form("config",serde_json::json!({}),window,cx);state.settings_visible=false;},"mirror"=>{state.open_native_form("mirror",serde_json::json!({"updateMirror":state.snapshot.appearance["updateMirror"]}),window,cx);state.settings_visible=false;},"help"=>state.open_help(window,cx),"shell-fonts"|"ui-fonts"|"ui-style"|"charts"|"ansi"|"prompt-colors"=>state.open_appearance(section,window,cx),"security"=>state.open_security_settings(window,cx),"keys"=>{state.ui.settings.keys_visible=true;state.settings_visible=false;},_=>state.ui.settings_view=section};
                cx.notify();
            }))
    }

    fn about_dialog(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        overlay(
            p,
            "关于 DevShell",
            tool("about-close", "×").on_click(cx.listener(|state, _, _, cx| {
                state.ui.about_visible = false;
                cx.notify();
            })),
        )
        .child(
            div()
                .p_6()
                .flex()
                .flex_col()
                .gap_3()
                .child(heading("DevShell · SSH 工作台"))
                .child(self.snapshot.version.clone())
                .child(format!("构建 {}", self.snapshot.build)),
        )
    }
}

pub(super) fn number(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.)
}
pub(super) fn overlay(p: Palette, title: &str, close: KitButton) -> Div {
    card(p)
        .occlude()
        .absolute()
        .top(px(72.))
        .left(gpui_kit::relative(0.5))
        .ml(px(-310.))
        .w(px(620.))
        .max_h_full()
        .flex()
        .flex_col()
        .shadow_lg()
        .child(
            div()
                .h(px(48.))
                .flex_shrink_0()
                .px_4()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(gpui_kit::rgb(p.border))
                .child(title.to_owned())
                .child(close),
        )
}

fn editor_language(path:&str)->&'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(){"rs"=>"Rust","go"=>"Go","js"|"mjs"|"cjs"=>"JavaScript","ts"|"tsx"=>"TypeScript","py"=>"Python","json"=>"JSON","html"|"htm"=>"HTML","css"=>"CSS","sh"|"bash"|"zsh"=>"Shell","yaml"|"yml"=>"YAML","toml"=>"TOML","md"=>"Markdown",_=>"Plain Text"}
}
