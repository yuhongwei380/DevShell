use super::*;
use crate::workspace::{Palette, icon, icon_tool};
use gpui_kit::{Div, SharedString, component::Sizable};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
pub(super) struct FileUi {
    pub folders: BTreeMap<String, Vec<RemoteEntry>>,
    pub expanded: BTreeSet<String>,
    pub pending: BTreeSet<String>,
    pub errors: BTreeMap<String, String>,
    pub invalidated:BTreeSet<String>,
    pub selected: BTreeSet<String>,
    pub anchor: Option<String>,
    pub menu: Option<(gpui_kit::Point<gpui_kit::Pixels>, Vec<RemoteEntry>)>,
    pub focus: Option<FocusHandle>,
    pub menu_focus: Option<FocusHandle>,
    pub menu_index: usize,
    pub create_menu: bool,
    pub submenu: Option<&'static str>,
    pub submenu_index: usize,
    pub menu_keyboard: bool,
    pub navigation: u64,
}

pub(super) fn select_names(
    selected: &mut BTreeSet<String>,
    anchor: &mut Option<String>,
    ordered: &[String],
    name: &str,
    toggle: bool,
    range: bool,
) {
    if range {
        if let Some((from, to)) = anchor.as_ref().and_then(|anchor| {
            Some((
                ordered.iter().position(|v| v == anchor)?,
                ordered.iter().position(|v| v == name)?,
            ))
        }) {
            if !toggle {
                selected.clear();
            }
            selected.extend(ordered[from.min(to)..=from.max(to)].iter().cloned());
            return;
        }
    }
    if toggle {
        if !selected.remove(name) {
            selected.insert(name.into());
        }
    } else {
        selected.clear();
        selected.insert(name.into());
    }
    *anchor = Some(name.into());
}

fn text(value: impl Into<SharedString>, size: f32) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size * 1.6))
        .child(value.into())
}
fn button(id: impl Into<gpui_kit::ElementId>, label: &str) -> KitButton {
    KitButton::new(id)
        .ghost()
        .h(px(28.))
        .px(px(8.))
        .rounded(px(7.))
        .child(text(label.to_owned(), 11.).w_full().text_left())
}

impl DevShellWindow {
    pub(super) fn ensure_file_tree(&mut self, cx: &mut Context<Self>) {
        if self.workspace.file_ui.focus.is_none() {
            self.workspace.file_ui.focus = Some(cx.focus_handle());
            self.workspace.file_ui.menu_focus = Some(cx.focus_handle());
        }
        if self.workspace.files.is_none() {
            return;
        }
        let query = self.ui.file_filter.read(cx).value().to_lowercase();
        let visible = self
            .workspace
            .files
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .filter(|e| e.name.to_lowercase().contains(&query))
            .map(|e| e.name.clone())
            .collect::<BTreeSet<_>>();
        self.workspace
            .file_ui
            .selected
            .retain(|name| visible.contains(name));
        if self
            .workspace
            .file_ui
            .anchor
            .as_ref()
            .is_some_and(|a| !visible.contains(a))
        {
            self.workspace.file_ui.anchor = None;
        }
        let missing = self
            .workspace
            .file_ui
            .expanded
            .iter()
            .filter(|path| {
                !self.workspace.file_ui.folders.contains_key(*path)
                    && !self.workspace.file_ui.pending.contains(*path)
                    && !self.workspace.file_ui.errors.contains_key(*path)
            })
            .cloned()
            .collect::<Vec<_>>();
        for path in missing {
            self.read_file_tree(path, cx);
        }

        if self.workspace.file_ui.expanded.is_empty() && self.workspace.file_ui.folders.is_empty() {
            self.workspace.file_ui.expanded.insert("/".into());
        }
        if !self.workspace.file_ui.folders.contains_key("/")
            && !self.workspace.file_ui.pending.contains("/")
            && !self.workspace.file_ui.errors.contains_key("/")
        {
            self.read_file_tree("/".into(), cx);
        }
    }

    pub(super) fn read_file_tree(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(id) = self
            .workspace
            .terminal
            .as_ref()
            .filter(|t| t.protocol == "ssh")
            .map(|t| t.session_id.clone())
        else {
            return;
        };
        if !self.workspace.file_ui.pending.insert(path.clone()) {
            return;
        }
        self.workspace.file_ui.errors.remove(&path);
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        let owner = id.clone();
        cx.spawn(async move |this, cx| {
            let request_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let data = get_json(
                        &base,
                        &token,
                        &format!(
                            "/api/sessions/{id}/files?path={}&directories=1",
                            query_escape(&request_path)
                        ),
                    )?;
                    Ok::<_, String>(parse_file_list(&data, &request_path).entries)
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                if let Some(session) = state.session_mut(&owner) {
                    session.file_ui.pending.remove(&path);
                    if session.file_ui.invalidated.remove(&path){session.file_ui.folders.remove(&path);session.file_ui.errors.remove(&path);cx.notify();return;}
                    match result {
                        Ok(mut entries) => {
                            entries.retain(|e| e.kind == "folder");
                            entries
                                .sort_by(|a, b| crate::collation::compare(&a.name, &b.name, true));
                            session.file_ui.folders.insert(path, entries);
                        }
                        Err(error) => {
                            session.file_ui.errors.insert(path, error);
                        }
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn original_file_browser(
        &self,
        p: Palette,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let files = self.workspace.files.as_ref();
        let path = files.map(|f| f.path.clone()).unwrap_or_default();
        let query = self.ui.file_filter.read(cx).value().to_lowercase();
        let mut entries = files
            .into_iter()
            .flat_map(|f| f.entries.iter())
            .filter(|e| e.name.to_lowercase().contains(&query))
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| {
            crate::file_browser::compare_entries(a, b, self.ui.file_sort, self.ui.file_sort_desc)
        });
        let names = entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        let connected = files.is_some()
            && self
                .workspace
                .terminal
                .as_ref()
                .is_some_and(|t| t.input_tx.is_some());
        let parent = parent_remote_path(&path);
        let right = self.ui.files_position == workspace::FilesPosition::Right;
        let hidden_tree = right && width <= 480.;
        let tree_width = if hidden_tree {
            0.
        } else {
            (width * 0.18).clamp(126., 208.)
        };
        let table_width = (width - tree_width - 23.).max(if hidden_tree { 360. } else { 80. });
        let compact = self.ui.viewport.0 <= 1100.;
        let short = self.ui.viewport.1 <= 850.;
        let row_height = if short { 26. } else { 28. };
        let columns = if compact {
            vec![
                ("name", "文件名", 0.48),
                ("size", "大小", 0.18),
                ("time", "修改时间", 0.34),
            ]
        } else if self.ui.viewport.0 <= 1350. {
            vec![
                ("name", "文件名", 0.40),
                ("size", "大小", 0.14),
                ("time", "修改时间", 0.28),
                ("mode", "权限", 0.18),
            ]
        } else {
            vec![
                ("name", "文件名", 0.35),
                ("size", "大小", 0.12),
                ("time", "修改时间", 0.23),
                ("mode", "权限", 0.14),
                ("owner", "用户 / 用户组", 0.16),
            ]
        };
        let table = div()
            .id("file-table-region")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .overflow_x_scroll()
            .pl(px(5.))
            .pb(px(7.))
            .on_mouse_down(
                gpui_kit::MouseButton::Right,
                cx.listener(|state, event: &gpui_kit::MouseDownEvent, window, cx| {
                    state.open_file_menu(event.position, Vec::new(), false, window, cx);
                }),
            )
            .child(
                div()
                    .h(px(if short { 25. } else { 28. }))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .bg(gpui_kit::rgb(p.surface))
                    .children(columns.iter().map(|&(key, label, ratio)| {
                        let active = self.ui.file_sort == key;
                        div()
                            .w(px(table_width * ratio))
                            .min_w_0()
                            .px(px(10.))
                            .child(
                                button(
                                    format!("file-sort-{key}"),
                                    &if !matches!(key, "name" | "size" | "time") { label.to_owned() } else { format!(
                                        "{label} {}", if active {
                                            if self.ui.file_sort_desc { "↓" } else { "↑" }
                                        } else {
                                            "↕"
                                        }
                                    ) },
                                )
                                .w_full()
                                .p_0()
                                .justify_start()
                                .text_color(gpui_kit::rgb(p.muted))
                                .on_click(cx.listener(
                                    move |state, _, _, cx| {
                                        if matches!(key, "name" | "size" | "time") {
                                            if state.ui.file_sort == key {
                                                state.ui.file_sort_desc = !state.ui.file_sort_desc;
                                            } else {
                                                state.ui.file_sort = key;
                                                state.ui.file_sort_desc = false;
                                            }
                                            cx.notify();
                                        }
                                    },
                                )),
                            )
                    })),
            )
            .children(entries.iter().enumerate().map(|(index, entry)| {
                let name = entry.name.clone();
                let right = entry.clone();
                let selected = self.workspace.file_ui.selected.contains(&name);
                let ordered = names.clone();
                let target = join_remote_path(&path, &name);
                let entry = entry.clone();
                let click_entry = entry.clone();
                div()
                    .id(format!("file-entry-{name}"))
                    .h(px(row_height))
                    .flex_shrink_0()
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .bg(gpui_kit::rgb(if selected {
                        if self.ui.dark { 0x213948 } else { 0xeaf3f8 }
                    } else if index % 2 == 1 {
                        if self.ui.dark { 0x1c252e } else { 0xf8fafc }
                    } else {
                        p.surface
                    }))
                    .hover(|d| d.bg(gpui_kit::rgb(p.inset)))
                    .on_mouse_down(
                        gpui_kit::MouseButton::Right,
                        cx.listener(move |state, event: &gpui_kit::MouseDownEvent, window, cx| {
                            if !state.workspace.file_ui.selected.contains(&right.name) {
                                state.workspace.file_ui.selected.clear();
                                state.workspace.file_ui.selected.insert(right.name.clone());
                                state.workspace.selected_file = Some(right.clone());
                            }
                            let targets = state.selected_file_entries();
                            state.open_file_menu(event.position, targets, false, window, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .on_click(cx.listener(
                        move |state, event: &gpui_kit::ClickEvent, window, cx| {
                            if let Some(focus) = &state.workspace.file_ui.focus {
                                window.focus(focus, cx);
                            }
                            let ui = &mut state.workspace.file_ui;
                            select_names(
                                &mut ui.selected,
                                &mut ui.anchor,
                                &ordered,
                                &name,
                                event.modifiers().control || event.modifiers().platform,
                                event.modifiers().shift,
                            );
                            state.workspace.selected_file = Some(click_entry.clone());
                            if event.click_count() == 2&&!event.modifiers().control&&!event.modifiers().platform&&!event.modifiers().shift {
                                if click_entry.kind == "folder" {
                                    state.browse_files(target.clone(),cx);
                                } else if click_entry.link {
                                    if let Some(id) = state.workspace.terminal.as_ref().map(|t| t.session_id.clone()) {
                                        navigate_files_for_mode(cx.entity(), id, target.clone(), true, cx);
                                    }
                                } else {
                                    state.preview_file(target.clone(),cx);
                                }
                            }
                            cx.notify();
                        },
                    ))
                    .children(columns.iter().map(|&(key, _, ratio)| {
                        let cell = div()
                            .w(px(table_width * ratio))
                            .min_w_0()
                            .h_full()
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .text_color(gpui_kit::rgb(if self.ui.dark {
                                0xc3d1dd
                            } else if key == "name" {
                                p.text
                            } else {
                                0x344f62
                            }));
                        match key {
                            "name" => cell
                                .gap(px(8.))
                                .child(
                                    icon(
                                        if entry.kind == "folder" {
                                            "folder"
                                        } else {
                                            "file"
                                        },
                                        14.,
                                    )
                                    .text_color(gpui_kit::rgb(
                                        if entry.kind == "folder" {
                                            0xc8b695
                                        } else {
                                            0xacc1cf
                                        },
                                    )),
                                )
                                .child(text(format!("{}{}", entry.name, if entry.link {" ↗"} else {""}), 12.)),
                            "size" => cell.child(text(
                                if entry.kind == "folder" {
                                    "—".into()
                                } else {
                                    crate::file_browser::size(entry.bytes)
                                },
                                12.,
                            )),
                            "time" => cell.child(text(entry.modified.clone(), 11.)),
                            "mode" => cell
                                .font_family("Deng JetBrains Mono")
                                .child(text(entry.mode.clone(), 10.)),
                            _ => cell.child(text(entry.owner.clone(), 11.)),
                        }
                    }))
            }))
            .when(entries.is_empty(), |d| {
                d.child(
                    text(
                        if files.is_some() {
                            "当前目录没有匹配的文件"
                        } else {
                            "连接后显示远程文件"
                        },
                        12.,
                    )
                    .py(px(35.))
                    .text_center()
                    .text_color(gpui_kit::rgb(p.muted)),
                )
            });

        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .child(
                div()
                    .flex_shrink_0()
                    .min_h(px(36.))
                    .px(px(9.))
                    .py(px(4.))
                    .flex()
                    .when(right, |d| d.flex_wrap())
                    .items_center()
                    .gap(px(3.))
                    .child(
                        icon_tool("file-parent", "up", "上一级目录")
                            .size(px(28.))
                            .disabled(!connected || parent.is_none())
                            .on_click(cx.listener(move |state, _, _, cx| {
                                if let Some(path) = &parent {
                                    state.browse_files(path.clone(),cx);
                                }
                            })),
                    )
                    .child(
                        icon_tool("file-favorites", "bookmark", "常用目录")
                            .size(px(28.))
                            .disabled(!connected)
                            .on_click(cx.listener(|state, _, window, cx| {
                                state.open_file_paths("directory-favorites", window, cx)
                            })),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .when(!right, |d| d.flex_1())
                            .when(right, |d| d.w(px((width - 118.).max(80.))).flex_shrink_0())
                            .h(px(28.))
                            .rounded(px(7.))
                            .px(px(8.))
                            .bg(gpui_kit::rgb(p.inset))
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .child(icon("folder", 13.))
                            .child(
                                Input::new(&self.ui.file_path)
                                    .small()
                                    .appearance(false)
                                    .bordered(false)
                                    .text_size(px(11.)),
                            ),
                    )
                    .child(
                        icon_tool("file-path-history", "history", "历史路径")
                            .size(px(28.))
                            .disabled(!connected)
                            .on_click(cx.listener(|state, _, window, cx| {
                                state.open_file_paths("path-history", window, cx)
                            })),
                    )
                    .child(
                        icon_tool("file-refresh", "refresh", "刷新目录")
                            .size(px(28.))
                            .disabled(!connected)
                            .on_click(cx.listener(|state, _, _, cx| {
                                if let Some(f) = &state.workspace.files {
                                    state.browse_files(f.path.clone(),cx);
                                }
                            })),
                    )
                    .child(
                        div()
                            .w(px(1.))
                            .h(px(14.))
                            .mx(px(3.))
                            .bg(gpui_kit::rgb(p.border)),
                    )
                    .child(
                        icon_tool("file-create", "plus", "新建文件或文件夹")
                            .size(px(28.))
                            .disabled(!connected)
                            .on_click(cx.listener(|state, event: &gpui_kit::ClickEvent, window, cx| {
                                state.open_file_menu(event.position(), Vec::new(), true, window, cx);
                            })),
                    )
                    .child(
                        KitButton::new("file-upload").ghost().h(px(28.)).px(px(8.)).rounded(px(7.))
                            .child(icon("upload", 14.))
                            .child(text("上传", 11.))
                            .disabled(!connected)
                            .on_click(cx.listener(|state, _, _, cx| state.choose_upload(cx))),
                    )
                    .when(width > 480., |d| {
                        d.child(
                            div()
                                .w(px(120.))
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .child(icon("search", 12.))
                                .child(
                                    Input::new(&self.ui.file_filter)
                                        .small()
                                        .appearance(false)
                                        .bordered(false)
                                        .text_size(px(10.)),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .min_h_0()
                    .flex_1()
                    .flex()
                    .px(px(9.))
                    .when_some(self.workspace.file_ui.focus.as_ref(), |d, f| {
                        d.track_focus(f)
                    })
                    .on_key_down(cx.listener(
                        move |state, event: &gpui_kit::KeyDownEvent, window, cx| {
                            let key = event.keystroke.key.as_str();
                            let modifier = event.keystroke.modifiers.control
                                || event.keystroke.modifiers.platform;
                            if key == "a" && modifier {
                                state.workspace.file_ui.selected = names.iter().cloned().collect();
                                cx.stop_propagation();
                                cx.notify();
                            } else if matches!(key, "delete" | "backspace" | "f2" | "enter") {
                                let action = if key == "f2" {
                                    "rename"
                                } else if key == "enter" {
                                    "open"
                                } else {
                                    "delete"
                                };
                                state.file_context_action(action, window, cx);
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .when(!hidden_tree, |d| {
                        d.child(
                            div()
                                .id("file-directory-tree")
                                .w(px(tree_width))
                                .flex_shrink_0()
                                .h_full()
                                .overflow_y_scroll()
                                .pr(px(7.))
                                .pb(px(8.))
                                .child(if files.is_some() {
                                    div().children(self.file_tree_rows(p, "/", "/", false, 0, &path, cx))
                                } else {
                                    text("连接后显示服务器目录", 11.)
                                        .px(px(8.))
                                        .py(px(12.))
                                        .text_color(gpui_kit::rgb(p.muted))
                                }),
                        )
                    })
                    .child(table),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex_shrink_0()
                    .px(px(17.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_color(gpui_kit::rgb(p.muted))
                    .child(text(
                        if let Some(_files) = files {
                            format!(
                                "{} 个项目{}{}",
                                entries.len(),
                                if query.is_empty() {
                                    ""
                                } else {
                                    " · 已筛选"
                                },
                                if self.workspace.file_ui.selected.is_empty() {
                                    String::new()
                                } else {
                                    format!(" · 已选 {} 项", self.workspace.file_ui.selected.len())
                                }
                            )
                        } else {
                            "尚未连接".into()
                        },
                        9.,
                    ))
                    .child(text(
                        files
                            .map(|f| {
                                if f.status == "已加载" {
                                    "SFTP"
                                } else {
                                    &f.status
                                }
                            })
                            .unwrap_or("SFTP")
                            .to_owned(),
                        9.,
                    )),
            )
    }

    fn file_tree_rows(
        &self,
        p: Palette,
        path: &str,
        name: &str,
        link: bool,
        depth: usize,
        current: &str,
        cx: &mut Context<Self>,
    ) -> Vec<gpui_kit::AnyElement> {
        let ui = &self.workspace.file_ui;
        let expanded = ui.expanded.contains(path);
        let selected = current == path;
        let toggle = path.to_owned();
        let target = path.to_owned();
        let row = div()
            .h(px(28.))
            .flex_shrink_0()
            .ml(px(depth as f32 * 12.))
            .rounded(px(7.))
            .flex()
            .items_center()
            .when(selected, |d| d.bg(gpui_kit::rgb(p.inset)))
            .child(
                KitButton::new(format!("tree-toggle-{path}"))
                    .ghost()
                    .p_0()
                    .w(px(18.))
                    .h(px(24.))
                    .disabled(depth >= 24)
                    .child(icon("chevron", 10.).when(expanded, |s| {
                        s.with_transformation(gpui_kit::Transformation::rotate(gpui_kit::radians(
                            std::f32::consts::FRAC_PI_2,
                        )))
                    }))
                    .on_click(cx.listener(move |state, _, _, cx| {
                        if !state.workspace.file_ui.expanded.remove(&toggle) {
                            state.workspace.file_ui.expanded.insert(toggle.clone());
                            state.read_file_tree(toggle.clone(), cx);
                        }
                        cx.notify();
                    })),
            )
            .child(
                KitButton::new(format!("tree-link-{path}"))
                    .ghost()
                    .min_w_0()
                    .flex_1()
                    .h(px(28.))
                    .p_0()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(icon("folder", 14.).text_color(gpui_kit::rgb(0xc8b695)))
                            .child(text(name.to_owned(), 11.))
                            .when(link, |d| d.child(text("↗", 10.))),
                    )
                    .on_click(cx.listener(move |state, _, _, cx| {
                        state.browse_files(target.clone(),cx)
                    })),
            );
        let mut rows = vec![row.into_any_element()];
        if expanded && depth < 24 {
            if ui.pending.contains(path) {
                rows.push(
                    text("正在读取…", 11.)
                        .ml(px((depth + 1) as f32 * 12. + 10.))
                        .text_color(gpui_kit::rgb(p.muted))
                        .into_any_element(),
                );
            } else if ui.errors.contains_key(path) {
                let path = path.to_owned();
                rows.push(
                    button(format!("tree-retry-{path}"), "读取失败 · 重试")
                        .on_click(cx.listener(move |state, _, _, cx| {
                            state.read_file_tree(path.clone(), cx)
                        }))
                        .into_any_element(),
                );
            } else if let Some(children) = ui.folders.get(path) {
                if children.is_empty() {
                    rows.push(
                        text("无子目录", 11.)
                            .ml(px((depth + 1) as f32 * 12. + 10.))
                            .text_color(gpui_kit::rgb(p.muted))
                            .into_any_element(),
                    );
                }
                for child in children {
                    rows.extend(self.file_tree_rows(
                        p,
                        &join_remote_path(path, &child.name),
                        &child.name,
                        child.link,
                        depth + 1,
                        current,
                        cx,
                    ));
                }
            }
        }
        rows
    }

    pub(super) fn selected_file_entries(&self) -> Vec<RemoteEntry> {
        self.workspace
            .files
            .as_ref()
            .into_iter()
            .flat_map(|f| f.entries.iter())
            .filter(|e| self.workspace.file_ui.selected.contains(&e.name))
            .cloned()
            .collect()
    }

    pub(super) fn open_file_menu(&mut self, point: gpui_kit::Point<gpui_kit::Pixels>, targets: Vec<RemoteEntry>, create: bool, window: &mut Window, cx: &mut Context<Self>) {
        let ui = &mut self.workspace.file_ui;
        ui.menu = Some((point, targets));
        ui.create_menu = create;
        ui.submenu = None;
        ui.submenu_index = 0;
        ui.menu_keyboard = false;
        ui.menu_index = 0;
        let focus = ui.menu_focus.get_or_insert_with(|| cx.focus_handle());
        window.focus(focus, cx);
        cx.notify();
    }

    pub(super) fn close_file_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.file_ui.menu = None;
        if let Some(focus) = &self.workspace.file_ui.focus { window.focus(focus, cx); }
        cx.notify();
    }

    fn file_menu_actions(&self) -> Vec<(&'static str, &'static str)> {
        let targets = &self.workspace.file_ui.menu.as_ref().unwrap().1;
        if self.workspace.file_ui.create_menu {
            return vec![("new-file", "新建文件"), ("mkdir", "新建文件夹")];
        }
        if targets.is_empty() {
            vec![("refresh", "刷新"), ("separator", ""), ("create-submenu", "新建")]
        } else if targets.len() > 1 {
            vec![("delete", ""), ("separator", ""), ("archive", ""), ("separator", ""), ("copy", "复制所选路径")]
        } else {
            let mut actions = vec![("open", "打开"), ("open-submenu", "打开方式"), ("separator", ""), ("rename", "重命名"), ("separator", ""),
                ("delete", "删除"), ("quick-delete", "快速删除"), ("separator", ""), ("download", "下载"),
                ("archive", "打包并下载"), ("separator", ""), ("copy", "复制路径"), ("separator", ""), ("permissions", "文件权限")];
            if targets[0].kind == "folder" { actions.retain(|(id, _)| *id != "download"); }
            actions
        }
    }

    fn file_submenu_actions(&self) -> Vec<(&'static str, &'static str)> {
        match self.workspace.file_ui.submenu {
            Some("create-submenu") => vec![("new-file", "文件"), ("mkdir", "文件夹")],
            Some("open-submenu") => vec![("open", "文本编辑器（默认）"), ("external", "系统关联"), ("external-settings", "自定义打开方式…")],
            _ => Vec::new(),
        }
    }

    pub(super) fn file_context_menu(
        &self,
        p: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let (point, _) = self.workspace.file_ui.menu.as_ref().unwrap();
        let creating = self.workspace.file_ui.create_menu;
        let actions = self.file_menu_actions();
        let width = if creating {166.} else {190.};
        let row_height = if creating {37.} else {31.};
        let menu_height = actions
            .iter()
            .map(|(id, _)| if *id == "separator" { 11. } else { row_height })
            .sum::<f32>()
            + 12.;
        let left = point.x.min(window.viewport_size().width - px(width+8.)).max(px(8.));
        let top = point.y.min((window.viewport_size().height - px(menu_height+8.)).max(px(8.)));
        let submenu_y = actions.iter().take_while(|(id, _)| Some(*id) != self.workspace.file_ui.submenu).map(|(id, _)| if *id == "separator" {11.} else {row_height}).sum::<f32>();
        div()
            .absolute()
            .inset_0()
            .when_some(self.workspace.file_ui.menu_focus.as_ref(), |d, focus| d.track_focus(focus))
            .on_key_down(cx.listener(|state, event: &gpui_kit::KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                state.workspace.file_ui.menu_keyboard = true;
                let in_submenu = state.workspace.file_ui.submenu.is_some();
                let actions = if in_submenu { state.file_submenu_actions() } else { state.file_menu_actions() };
                if matches!(key, "escape" | "tab") { state.close_file_menu(window, cx); }
                else if matches!(key, "up" | "down" | "home" | "end") {
                    let selectable = actions.iter().enumerate().filter(|(_, (action, _))| *action != "separator").map(|(index, _)| index).collect::<Vec<_>>();
                    let current = selectable.iter().position(|i| *i == if in_submenu {state.workspace.file_ui.submenu_index} else {state.workspace.file_ui.menu_index}).unwrap_or(0);
                    let index = match key { "home" => 0, "end" => selectable.len()-1, "up" => (current + selectable.len()-1)%selectable.len(), _ => (current+1)%selectable.len() };
                    if in_submenu { state.workspace.file_ui.submenu_index = selectable[index]; } else { state.workspace.file_ui.menu_index = selectable[index]; }
                    cx.notify();
                } else if key == "left" {
                    state.workspace.file_ui.submenu = None;
                    cx.notify();
                } else if key == "right" && !in_submenu {
                    let action = actions[state.workspace.file_ui.menu_index].0;
                    if matches!(action, "open-submenu" | "create-submenu") { state.workspace.file_ui.submenu=Some(action); state.workspace.file_ui.submenu_index=0; cx.notify(); }
                } else if key == "enter" || key == "space" {
                    let action = actions[if in_submenu {state.workspace.file_ui.submenu_index} else {state.workspace.file_ui.menu_index}].0;
                    if matches!(action, "open-submenu" | "create-submenu") { state.workspace.file_ui.submenu=Some(action);state.workspace.file_ui.submenu_index=0;cx.notify(); } else {state.file_context_action(action, window, cx);}
                } else { return; }
                cx.stop_propagation();
            }))
            .child(div().absolute().inset_0().occlude().on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|state, _, window, cx| {
                    state.close_file_menu(window, cx);
                }),
            ))
            .child(
                div()
                    .absolute()
                    .left(left)
                    .top(top)
                    .w(px(width))
                    .p(px(6.))
                    .rounded(px(13.))
                    .border_1()
                    .border_color(gpui_kit::rgb(p.border))
                    .bg(gpui_kit::rgb(p.surface))
                    .text_color(gpui_kit::rgb(p.text))
                    .shadow_lg()
                    .occlude()
                    .flex()
                    .flex_col()
                    .children(actions.into_iter().enumerate().map(|(index, (action, label))| {
                        if action == "separator" {
                            div()
                                .h(px(1.))
                                .my(px(5.))
                                .mx(px(7.))
                                .bg(gpui_kit::rgb(p.border))
                                .into_any_element()
                        } else {
                            KitButton::new(format!("file-context-{action}")).ghost()
                                .when(creating, |b| b.child(icon(if action == "mkdir" {"folder"} else {"file"}, 16.)))
                                .text_color(gpui_kit::rgb(p.text))
                                .child(text(if label.is_empty() { if action == "delete" {format!("删除选中的 {} 项", self.workspace.file_ui.menu.as_ref().unwrap().1.len())} else {format!("打包并下载 {} 项", self.workspace.file_ui.menu.as_ref().unwrap().1.len())} } else {label.to_owned()}, 12.).w_full().text_left())
                                .when(matches!(action, "open-submenu" | "create-submenu"), |b| b.child(div().flex_1()).child(icon("chevron",12.)))
                                .h(px(row_height))
                                .w_full()
                                .justify_start()
                                .px(px(11.))
                                .gap(px(if creating {11.} else {12.}))
                                .rounded(px(8.))
                                .when(self.workspace.file_ui.menu_keyboard && index == self.workspace.file_ui.menu_index && self.workspace.file_ui.menu_focus.as_ref().is_some_and(|f| f.is_focused(window)), |b| b.bg(gpui_kit::rgb(p.inset)))
                                .when(matches!(action, "delete" | "quick-delete"), |b| {
                                    b.text_color(gpui_kit::rgb(if self.ui.dark {
                                        0xd9a0aa
                                    } else {
                                        0xa33d48
                                    }))
                                })
                                .on_hover(cx.listener(move |state, over: &bool, _, cx| {
                                    if *over {state.workspace.file_ui.menu_keyboard=false;state.workspace.file_ui.submenu=if matches!(action,"open-submenu"|"create-submenu"){Some(action)}else{None};state.workspace.file_ui.menu_index=index;state.workspace.file_ui.submenu_index=0;cx.notify();}
                                }))
                                .on_click(cx.listener(move |state, _, window, cx| {
                                    if matches!(action,"open-submenu"|"create-submenu"){state.workspace.file_ui.submenu=Some(action);state.workspace.file_ui.submenu_index=0;cx.notify();}else{state.file_context_action(action, window, cx)}
                                }))
                                .into_any_element()
                        }
                    })),
            )
            .when(self.workspace.file_ui.submenu.is_some(), |root| root.child(
                div().absolute().left(if left+px(width+210.)>window.viewport_size().width-px(8.){(left-px(203.)).max(px(4.))}else{left+px(width-7.)}).top(top+px(submenu_y+1.)).w(px(210.)).p(px(6.)).rounded(px(13.)).border_1().border_color(gpui_kit::rgb(p.border)).bg(gpui_kit::rgb(p.surface)).text_color(gpui_kit::rgb(p.text)).shadow_lg().occlude().flex().flex_col()
                    .children(self.file_submenu_actions().into_iter().enumerate().map(|(index,(action,label))| KitButton::new(format!("file-submenu-{action}")).ghost().w_full().h(px(31.)).px(px(11.)).rounded(px(8.)).justify_start().text_color(gpui_kit::rgb(p.text)).child(text(label,12.).w_full().text_left()).when(self.workspace.file_ui.menu_keyboard && index==self.workspace.file_ui.submenu_index,|b|b.bg(gpui_kit::rgb(p.inset))).on_click(cx.listener(move|state,_,window,cx|state.file_context_action(action,window,cx)))))
            ))
    }
}

impl DevShellWindow {
    pub(super) fn persist_files_layout(&mut self, cx: &mut Context<Self>) {
        let mut layout = self.snapshot.appearance["layout"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        layout.insert("filesHidden".into(), (!self.ui.files_visible).into());
        if let Some(extent) = self.ui.files_extent {
            layout.insert("files".into(), Value::from(extent));
        }
        self.save_appearance(serde_json::json!({"layout":layout}), cx);
    }
}

impl FileUi {
    pub fn invalidate(&mut self,path:&str){self.folders.remove(path);self.errors.remove(path);if self.pending.contains(path){self.invalidated.insert(path.into());}}
}
impl SessionWorkspace {
    pub(super) fn accept_listing(&mut self,generation:u64,path:String,result:Result<FileListState,String>)->bool{
        if self.file_ui.navigation!=generation{return false;}
        match result {
            Ok(files)=>{let mut folders=files.entries.iter().filter(|e|e.kind=="folder").cloned().collect::<Vec<_>>();folders.sort_by(|a,b|crate::collation::compare(&a.name,&b.name,true));self.file_ui.folders.insert(files.path.clone(),folders);if self.file_ui.pending.contains(&files.path){self.file_ui.invalidated.insert(files.path.clone());}self.file_ui.selected.clear();self.file_ui.anchor=None;self.file_ui.menu=None;self.selected_file=None;self.files=Some(files);},
            Err(error)=>{if let Some(files)=self.files.as_mut(){files.status=format!("目录读取失败：{error}");}else{self.files=Some(FileListState{path,entries:Vec::new(),status:error});}}
        }true
    }
}
impl DevShellWindow {
    pub(super) fn browse_files(&mut self,path:String,cx:&mut Context<Self>){if let Some(id)=self.workspace.terminal.as_ref().filter(|t|t.protocol=="ssh"&&t.input_tx.is_some()).map(|t|t.session_id.clone()){navigate_files_for(cx.entity(),id,path,cx);}}
    pub(super) fn preview_file(&mut self,path:String,cx:&mut Context<Self>){if let Some(id)=self.workspace.terminal.as_ref().filter(|t|t.protocol=="ssh"&&t.input_tx.is_some()).map(|t|t.session_id.clone()){preview_remote_for(cx.entity(),id,path,cx);}}
}
