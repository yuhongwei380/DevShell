use super::*;
use crate::{forms::string, workspace::Palette};
use gpui_kit::{Div, component::Sizable};

pub(super) fn valid_delete_target(path: &str) -> bool {
    path.starts_with('/')
        && !path.contains('\0')
        && !path.split('/').any(|p| p == "..")
        && path.split('/').any(|p| !p.is_empty() && p != ".")
}

fn open_local_copy(local: &str, editor: &str) -> Result<(), String> {
    let path = std::path::Path::new(local);
    if !path.is_absolute() || !path.exists() {return Err("本地副本不存在或路径无效".into());}
    if !editor.is_empty() {
        let executable = std::path::Path::new(editor);
        if !executable.is_absolute() || !executable.is_file() { return Err("自定义编辑器不存在，请重新设置打开方式".into()); }
        let mut child = std::process::Command::new(executable).arg(path).spawn().map_err(|e| format!("启动编辑器：{e}"))?;
        std::thread::spawn(move || {let _=child.wait();});
        return Ok(());
    }
    open_associated_copy(path)
}

#[cfg(target_os="windows")]
fn open_associated_copy(path: &std::path::Path) -> Result<(),String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name="shell32")]
    unsafe extern "system" {fn ShellExecuteW(window:*mut std::ffi::c_void,operation:*const u16,file:*const u16,parameters:*const u16,directory:*const u16,show:i32)->isize;}
    let verb = "open\0".encode_utf16().collect::<Vec<_>>();
    let file = path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let result = unsafe {ShellExecuteW(std::ptr::null_mut(),verb.as_ptr(),file.as_ptr(),std::ptr::null(),std::ptr::null(),1)};
    if result<=32 {Err(format!("系统关联打开失败（{result}）"))} else {Ok(())}
}
#[cfg(target_os="macos")]
fn open_associated_copy(path: &std::path::Path) -> Result<(),String> {
    let status = std::process::Command::new("/usr/bin/open").arg("--").arg(path).status().map_err(|e|e.to_string())?;
    if status.success() {Ok(())} else {Err("系统关联打开失败".into())}
}

impl DevShellWindow {
    fn open_external_file(&mut self, owner: String, remote: String, cx: &mut Context<Self>) {
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        let editor = self.snapshot.appearance["externalEditor"].as_str().unwrap_or("").to_owned();
        self.feedback = Some("正在下载本地副本…".into());
        cx.spawn(async move |this, cx| {
            let session_id = owner.clone();
            let result = cx.background_executor().spawn(async move {
                let value = post_json(&base, &token, "/api/windows/native", serde_json::json!({"method":"external-file","sessionId":session_id,"remote":remote}))?;
                value.as_str().filter(|s| !s.is_empty()).map(str::to_owned).ok_or_else(|| "系统关联未返回本地副本路径".to_owned())
            }).await;
            let local = match result {
                Ok(local) => local,
                Err(error) => { let _=this.update(cx, |state,cx| {state.feedback=Some(error);cx.notify();});return; }
            };
            let allowed = this.update(cx, |state,_| !state.snapshot.locked && state.session_mut(&owner).is_some_and(|s| s.terminal.as_ref().is_some_and(|t| t.input_tx.is_some()))).unwrap_or(false);
            if !allowed {return;}
            let display = local.clone();
            let result = cx.background_executor().spawn(async move { open_local_copy(&local,&editor) }).await;
            let _=this.update(cx, |state,cx| {state.feedback=Some(match result {Ok(())=>format!("已打开本地副本：{display}。修改后需上传回服务器。"),Err(error)=>error});cx.notify();});
        }).detach();
        cx.notify();
    }

    pub(super) fn file_context_action(
        &mut self,
        action: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.form.is_some() {
            return;
        }
        let targets = self
            .workspace
            .file_ui
            .menu
            .take()
            .map(|(_, entries)| entries)
            .unwrap_or_else(|| self.selected_file_entries());
        if let Some(focus) = &self.workspace.file_ui.focus { window.focus(focus, cx); }
        cx.notify();
        let Some(session) = self
            .workspace
            .terminal
            .as_ref()
            .filter(|t| t.protocol == "ssh" && t.input_tx.is_some())
        else {
            return;
        };
        if self.snapshot.locked {
            return;
        }
        let id = session.session_id.clone();
        let base = self
            .workspace
            .files
            .as_ref()
            .map(|f| f.path.clone())
            .unwrap_or("/".into());
        let paths = targets
            .iter()
            .map(|e| join_remote_path(&base, &e.name))
            .collect::<Vec<_>>();
        if action == "external-settings" {
            self.open_native_form("file-action", serde_json::json!({"_action":action,"_title":"系统关联打开方式","_sessionId":id,"_base":base,"_targets":[],"name":self.snapshot.appearance["externalEditor"].as_str().unwrap_or("")}), window, cx);
            return;
        }
        if action == "external" && targets.first().is_some_and(|e| e.kind != "folder") {
            self.open_external_file(id, paths[0].clone(), cx);
            return;
        }
        if action == "refresh" {
            self.browse_files(base,cx);
            return;
        }
        if action == "copy" {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(paths.join("\n")));
            cx.notify();
            return;
        }
        if action == "open" {
            if let Some(entry) = targets.first() {
                if entry.kind == "folder" {
                    self.browse_files(paths[0].clone(),cx);
                } else {
                    self.preview_file(paths[0].clone(),cx);
                }
            }
            return;
        }
        if matches!(action, "download" | "archive") {
            if !paths.is_empty() {
                self.download_file_selection(
                    id,
                    paths,
                    action == "archive" || targets.len() > 1 || targets[0].kind == "folder",
                    cx,
                );
            }
            return;
        }
        if targets.is_empty() && !matches!(action, "new-file" | "mkdir") {
            return;
        }
        let title = match action {
            "rename" => "重命名",
            "new-file" => "新建文件",
            "mkdir" => "新建文件夹",
            "permissions" => "文件权限",
            "quick-delete" => "快速删除",
            "external" => "在系统文件管理器中打开目录？",
            _ => "删除",
        };
        let title = if matches!(action, "delete" | "quick-delete") { if paths.len()==1 {format!("{title} {}？",targets[0].name)} else {format!("{title}选中的 {} 个项目？",paths.len())} } else {title.to_owned()};
        self.open_native_form("file-action",serde_json::json!({"_action":action,"_title":title,"_server":session.name,"_sessionId":id,"_base":base,"_targets":paths,"name":targets.first().map(|e|e.name.as_str()).unwrap_or(""),"_directories":targets.iter().any(|e|e.kind=="folder")}),window,cx);
        {
            use gpui_kit::Focusable;
            let f = self.form.as_ref().unwrap();
            let input = f.inputs[if action == "permissions" {
                "mode"
            } else {
                "name"
            }]
            .clone();
            if matches!(action, "rename" | "new-file" | "mkdir" | "permissions") {
                window.focus(&input.read(cx).focus_handle(cx), cx);
            }
            cx.subscribe_in(
                &input,
                window,
                |state, _, event: &gpui_kit::component::input::InputEvent, _, cx| {
                    if matches!(
                        event,
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                    ) {
                        state.submit_file_action(cx);
                    } else {
                        cx.notify();
                    }
                },
            )
            .detach();
        }
        if action == "permissions" {
            let serial = self.form.as_ref().unwrap().original["_nativeSerial"].clone();
            let remote = paths[0].clone();
            let base = self.snapshot.base.clone();
            let token = self.token.clone();
            let owner = id.clone();
            self.form.as_mut().unwrap().busy = true;
            cx.spawn_in(window, async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        get_json(
                            &base,
                            &token,
                            &format!(
                                "/api/sessions/{owner}/file-permissions?path={}",
                                query_escape(&remote)
                            ),
                        )
                    })
                    .await;
                let _ = this.update_in(cx, |state, window, cx| {
                    if let Some(f) = state
                        .form
                        .as_mut()
                        .filter(|f| f.original["_nativeSerial"] == serial)
                    {
                        f.busy = false;
                        match result {
                            Ok(value) => {
                                f.original["_permissions"] = value.clone();
                                f.inputs["mode"].update(cx, |input, cx| {
                                    input.set_value(string(&value, "mode"), window, cx)
                                });
                            }
                            Err(error) => f.error = Some(error),
                        }
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    pub(super) fn original_file_action_dialog(&self, p: Palette, window: &Window, cx: &mut Context<Self>) -> Div {
        let f = self.form.as_ref().unwrap();
        let permissions = f.original["_action"] == "permissions";
        let action = f.original["_action"].as_str().unwrap_or("");
        let label = match action {"new-file"|"mkdir"=>"创建","delete"=>"删除","quick-delete"=>"快速删除","permissions"=>"保存权限","external"=>"下载并打开","external-settings"=>"保存",_=>"确定"};
        let label = if matches!(action,"delete"|"quick-delete") && f.original["_targets"].as_array().is_some_and(|paths|paths.len()>1) {format!("{label} {} 项",f.original["_targets"].as_array().unwrap().len())} else {label.to_owned()};
        div().absolute().inset_0().flex().items_center().justify_center().occlude().child(
            div().w(px(if permissions{440.}else{430.})).max_w(window.viewport_size().width-px(30.)).max_h(window.viewport_size().height-px(32.)).px(px(28.)).py(px(25.)).rounded(px(19.)).bg(gpui_kit::rgb(p.surface)).text_color(gpui_kit::rgb(p.text)).shadow_lg().occlude().flex().flex_col()
                .child(div().flex().items_center().justify_between().mb(px(if permissions{16.}else{14.94})).when(!permissions,|d|d.mt(px(14.94))).child(div().text_size(px(if permissions{17.}else{18.})).line_height(px(if permissions{27.2}else{25.})).font_weight(gpui_kit::FontWeight(500.)).child(string(&f.original,"_title"))).when(permissions,|d|d.child(crate::workspace::icon_tool("file-permissions-close","close","关闭").disabled(f.busy).on_click(cx.listener(|state,_,_,cx|state.dismiss_native_form(cx))))))
                .child(div().id("file-action-fields").min_h_0().overflow_y_scroll().child(self.file_action_fields(cx)).when(f.error.is_some(),|d|d.child(f.error_panel())))
                .child(div().flex().justify_end().gap(px(12.)).when(!permissions,|d|d.mt(px(22.))).when(!permissions,|d|d.child(KitButton::new("file-action-cancel").ghost().px(px(18.)).h(px(36.)).rounded(px(9.)).text_color(gpui_kit::rgb(p.text)).child(div().text_size(px(13.)).child("取消")).disabled(f.busy).on_click(cx.listener(|state,_,_,cx|state.dismiss_native_form(cx)))))
                    .child(KitButton::new("file-action-submit").primary().px(px(18.)).h(px(if permissions{37.}else{36.})).rounded(px(9.)).when(permissions,|b|b.w_full()).child(div().text_size(px(12.)).child(if f.busy{"处理中…".to_owned()}else{label})).disabled(f.busy).on_click(cx.listener(|state,_,_,cx|state.submit_file_action(cx)))))
        )
    }

    pub(super) fn file_action_fields(&self, cx: &mut Context<Self>) -> Div {
        let f = self.form.as_ref().unwrap();
        let action = string(&f.original, "_action");
        let paths = f.original["_targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap_0()
            .text_size(px(12.))
            .line_height(px(19.2))
            .child(div().mb(px(20.)).text_size(px(11.)).line_height(px(19.8)).text_color(gpui_kit::rgb(if self.ui.dark {0xa4b6c5} else {0x486174})).child(
                match action.as_str() {
                    "rename" => String::new(),
                    "external-settings" => "留空使用系统默认程序，或填写编辑器可执行文件的绝对路径（不附加命令参数）。外部编辑只修改本地副本。".into(),
                    "external" => "先下载此目录的本地副本（最多 256 MiB / 10000 项）。修改不会自动上传；含符号链接或特殊文件时请使用打包下载。".into(),
                    "delete" | "quick-delete" => format!("服务器：{}\n共 {} 个项目：\n{}{}\n\n{}此操作无法撤销。",string(&f.original,"_server"),paths.len(),paths.iter().take(12).copied().collect::<Vec<_>>().join("\n"),if paths.len()>12 {format!("\n……另有 {} 项",paths.len()-12)} else {String::new()},if f.original["_directories"]==true {"选中目录及其全部内容也会被删除。"} else {""}),
                    "permissions" => paths.join("\n"),
                    _ => string(&f.original,"_base"),
                }
            ))
            .when(
                matches!(action.as_str(), "rename" | "mkdir" | "new-file" | "external-settings"),
                |d| {
                    d.child(div().h(px(37.)).border_1().border_color(gpui_kit::rgb(if self.ui.dark{0x2d3944}else{0xdce6ee})).rounded(px(9.)).bg(gpui_kit::rgb(if self.ui.dark{0x131a21}else{0xf1f5f8})).child(gpui_kit::Styled::h(Input::new(&f.inputs["name"]).appearance(false).bordered(false),px(35.)).text_size(px(12.)).px(px(10.))))
                },
            )
            .when(action == "permissions", |d| {
                d.child(div().mb(px(20.)).text_size(px(11.)).line_height(px(19.8)).child(format!(
                    "所有者 / 用户组：{}",
                    string(&f.original["_permissions"], "owner")
                )))
                .child(div().flex().flex_col().gap(px(7.)).child(div().text_size(px(11.)).line_height(px(15.)).child("八进制权限")).child(gpui_kit::Styled::h(Input::new(&f.inputs["mode"]),px(37.)).text_size(px(12.)).px(px(10.)).rounded(px(9.))))
                .child(
                    div().my(px(20.)).flex().flex_col().gap(px(8.)).line_height(px(17.)).children(
                        ["所有者", "用户组", "其他用户"]
                            .into_iter()
                            .enumerate()
                            .map(|(row, role)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(div().w(px(65.)).flex_shrink_0().text_size(px(12.)).child(role))
                                    .children(["读取", "写入", "执行"].into_iter().enumerate().map(
                                        |(column, label)| {
                                            let bit = 1u32 << (8 - row * 3 - column);
                                            let mode =
                                                u32::from_str_radix(f.text("mode", cx).trim(), 8)
                                                    .unwrap_or(0);
                                            gpui_kit::component::checkbox::Checkbox::new(format!(
                                                "permission-{row}-{column}"
                                            ))
                                            .small().flex_1()
                                            .checked(mode & bit != 0)
                                            .child(div().text_size(px(12.)).child(label))
                                            .on_click(
                                                cx.listener(
                                                    move |state, enabled: &bool, window, cx| {
                                                        if let Some(f) = state.form.as_mut() {
                                                            let old = u32::from_str_radix(
                                                                f.text("mode", cx).trim(),
                                                                8,
                                                            )
                                                            .unwrap_or(0);
                                                            let next = if *enabled {
                                                                old | bit
                                                            } else {
                                                                old & !bit
                                                            };
                                                            f.inputs["mode"].update(
                                                                cx,
                                                                |input, cx| {
                                                                    input.set_value(
                                                                        format!("{next:04o}"),
                                                                        window,
                                                                        cx,
                                                                    )
                                                                },
                                                            );
                                                        }
                                                        cx.notify();
                                                    },
                                                ),
                                            )
                                        },
                                    ))
                            }),
                    ),
                )
            })
    }

    pub(super) fn submit_file_action(&mut self, cx: &mut Context<Self>) {
        let Some(f) = self.form.as_ref().filter(|f| !f.busy) else {
            return;
        };
        let owner = string(&f.original, "_sessionId");
        if self.snapshot.locked
            || !self.workspace.terminal.as_ref().is_some_and(|t| {
                t.session_id == owner && t.protocol == "ssh" && t.input_tx.is_some()
            })
        {
            self.form.as_mut().unwrap().error =
                Some("原 SSH 连接已断开、切换或软件已锁定，请重新选择文件。".into());
            cx.notify();
            return;
        }
        let action = string(&f.original, "_action");
        let paths = f.original["_targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let name = f.text("name", cx);
        let mode = f.text("mode", cx);
        if action == "external-settings" {
            let editor = name.trim().to_owned();
            if !editor.is_empty() && (!std::path::Path::new(&editor).is_absolute() || editor.contains(['\0','\r','\n']) || !std::path::Path::new(&editor).is_file()) {
                self.form.as_mut().unwrap().error = Some("请填写编辑器可执行文件的绝对路径，不附加命令参数；也可留空使用系统默认程序。".into());cx.notify();return;
            }
            self.form = None;
            self.save_appearance(serde_json::json!({"externalEditor":editor}), cx);
            return;
        }
        if action == "external" {
            let target = paths[0].clone();
            self.form = None;
            self.open_external_file(owner, target, cx);
            return;
        }
        if matches!(action.as_str(), "delete" | "quick-delete")
            && paths.iter().any(|p| !valid_delete_target(p))
        {
            self.form.as_mut().unwrap().error = Some("不能删除此路径".into());
            cx.notify();
            return;
        }
        if matches!(action.as_str(), "rename" | "new-file" | "mkdir")
            && (name.trim().is_empty()
                || name.contains('\0')
                || name.contains('/')
                || matches!(name.as_str(), "." | ".."))
        {
            self.form.as_mut().unwrap().error = Some("请输入有效的文件或文件夹名称".into());
            cx.notify();
            return;
        }
        if action == "permissions"
            && (mode.len() < 3 || mode.len() > 4 || !mode.bytes().all(|c| matches!(c, b'0'..=b'7')))
        {
            self.form.as_mut().unwrap().error = Some("权限应为 3–4 位八进制数字".into());
            cx.notify();
            return;
        }
        let serial = f.original["_nativeSerial"].clone();
        let directory = string(&f.original, "_base");
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        self.form.as_mut().unwrap().busy = true;
        cx.spawn(async move |this, cx| {
            let result_owner = owner.clone();
            let result_directory = directory.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut errors = Vec::new();
                    let mut failed = Vec::new();
                    let mut completed = 0;
                    if matches!(action.as_str(), "delete" | "quick-delete") {
                        for path in paths {
                            match post_file_action(&base, &token, &owner, "delete", &path, "") {
                                Ok(_) => completed += 1,
                                Err(error) => {
                                    errors.push(format!("{path}：{error}"));
                                    failed.push(path);
                                }
                            }
                        }
                    } else if action == "permissions" {
                        post_json(
                            &base,
                            &token,
                            &format!("/api/sessions/{owner}/file-permissions"),
                            serde_json::json!({"path":paths[0],"mode":mode}),
                        )?;
                        completed = 1;
                    } else {
                        post_file_action(
                            &base,
                            &token,
                            &owner,
                            if action == "rename" {
                                "rename"
                            } else if action == "mkdir" {
                                "mkdir"
                            } else {
                                "new-file"
                            },
                            if action == "rename" {
                                &paths[0]
                            } else {
                                &directory
                            },
                            &name,
                        )?;
                        completed = 1;
                    }
                    let files = load_files(&base, &token, &owner, &directory);
                    Ok::<_, String>((files, completed, errors, failed))
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                let current = state
                    .form
                    .as_ref()
                    .is_some_and(|f| f.original["_nativeSerial"] == serial);
                match result {
                    Ok((files, count, errors, failed)) => {
                        if let Some(session) = state.session_mut(&result_owner) {
                            if let Ok(files) = files {
                                if session
                                    .files
                                    .as_ref()
                                    .is_some_and(|f| f.path == result_directory)
                                {
                                    session.files = Some(files);
                                    session.file_ui.selected.clear();
                                    session.selected_file = None;
                                }
                            }
                            session.file_ui.invalidate(&result_directory);
                        }
                        if errors.is_empty() {
                            if current {
                                state.form = None;
                            }
                            state.feedback = Some(format!("已完成 {count} 项"));
                        } else if current {
                            let f = state.form.as_mut().unwrap();
                            f.busy = false;
                            f.original["_targets"] = serde_json::json!(failed);
                            f.error = Some(format!(
                                "已完成 {count} 项；{} 项失败：\n{}",
                                errors.len(),
                                errors.join("\n")
                            ));
                        }
                    }
                    Err(error) => {
                        if current {
                            let f = state.form.as_mut().unwrap();
                            f.busy = false;
                            f.error = Some(error);
                        } else {
                            state.feedback = Some(error);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn open_file_paths(
        &mut self,
        kind: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self
            .workspace
            .terminal
            .as_ref()
            .filter(|t| t.protocol == "ssh" && t.input_tx.is_some())
            .map(|t| t.session_id.clone())
        else {
            return;
        };
        let directory = self
            .workspace
            .files
            .as_ref()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        self.open_native_form(
            "file-paths",
            serde_json::json!({"_pathsKind":kind,"_sessionId":id,"_directory":directory}),
            window,
            cx,
        );
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(if kind == "directory-favorites" {
                "搜索收藏路径"
            } else {
                "搜索历史路径"
            })
        });
        cx.observe(&input, |_, _, cx| cx.notify()).detach();
        self.form
            .as_mut()
            .unwrap()
            .inputs
            .insert("pathQuery".into(), input);
        self.change_file_paths("GET", None, cx);
    }

    pub(super) fn change_file_paths(
        &mut self,
        method: &'static str,
        path: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(f) = self.form.as_ref().filter(|f| !f.busy) else {
            return;
        };
        let id = string(&f.original, "_sessionId");
        if self.snapshot.locked
            || !self
                .workspace
                .terminal
                .as_ref()
                .is_some_and(|t| t.session_id == id && t.input_tx.is_some())
        {
            return;
        }
        let kind = string(&f.original, "_pathsKind");
        let serial = f.original["_nativeSerial"].clone();
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        self.form.as_mut().unwrap().busy = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let endpoint = format!("/api/sessions/{id}/{kind}");
                    match method {
                        "POST" => {
                            post_json(&base, &token, &endpoint, serde_json::json!({"path":path}))
                        }
                        "DELETE" => {
                            if let Some(path) = path {
                                json_body_request(
                                    "DELETE",
                                    &base,
                                    &token,
                                    &endpoint,
                                    serde_json::json!({"path":path}),
                                )
                            } else {
                                delete_json(&base, &token, &endpoint)
                            }
                        }
                        _ => get_json(&base, &token, &endpoint),
                    }
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                if let Some(f) = state
                    .form
                    .as_mut()
                    .filter(|f| f.original["_nativeSerial"] == serial)
                {
                    f.busy = false;
                    match result {
                        Ok(data) => {
                            f.original["_data"] = data;
                            f.error = None;
                        }
                        Err(error) => f.error = Some(error),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn file_paths_fields(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let f = self.form.as_ref().unwrap();
        let favorites = string(&f.original, "_pathsKind") == "directory-favorites";
        let query = f.text("pathQuery", cx).to_lowercase();
        let directory = string(&f.original, "_directory");
        let saved = f.original["_data"]["saved"] != false;
        let entries = if favorites {
            f.original["_data"]["paths"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|path| serde_json::json!({"path":path,"kind":"folder"}))
                .collect::<Vec<_>>()
        } else {
            f.original["_data"]["entries"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().text_size(px(11.)).child(directory.clone()))
            .when(favorites, |d| {
                d.child(
                    KitButton::new("favorite-current-directory")
                        .ghost()
                        .w_full()
                        .h(px(32.))
                        .bg(gpui_kit::rgb(p.inset))
                        .child(div().text_size(px(12.)).child("收藏当前目录"))
                        .disabled(f.busy || !saved)
                        .on_click(cx.listener(move |state, _, _, cx| {
                            state.change_file_paths("POST", Some(directory.clone()), cx)
                        })),
                )
            })
            .child(Input::new(&f.inputs["pathQuery"]).text_size(px(12.)))
            .child(
                div()
                    .id("file-path-list")
                    .max_h(px(340.))
                    .overflow_y_scroll()
                    .children(
                        entries
                            .iter()
                            .filter(|e| string(e, "path").to_lowercase().contains(&query))
                            .map(|entry| {
                                let path = string(entry, "path");
                                let remove = path.clone();
                                let folder = string(entry, "kind") == "folder";
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(
                                        KitButton::new(format!("jump-path-{path}"))
                                            .ghost()
                                            .min_w_0()
                                            .flex_1()
                                            .h_auto()
                                            .py(px(9.))
                                            .child(
                                                div()
                                                    .w_full()
                                                    .text_size(px(12.))
                                                    .child(path.clone()),
                                            )
                                            .disabled(f.busy)
                                            .on_click(cx.listener(move |state, _, _, cx| {
                                                let Some(f) = state.form.as_ref() else {
                                                    return;
                                                };
                                                let owner = string(&f.original, "_sessionId");
                                                if state.snapshot.locked
                                                    || !state
                                                        .workspace
                                                        .terminal
                                                        .as_ref()
                                                        .is_some_and(|t| {
                                                            t.session_id == owner
                                                                && t.input_tx.is_some()
                                                        })
                                                {
                                                    return;
                                                }
                                                state.form = None;
                                                if folder {
                                                    state.browse_files(path.clone(),cx);
                                                } else {
                                                    state.preview_file(path.clone(),cx);
                                                }
                                            })),
                                    )
                                    .when(favorites, |d| {
                                        d.child(
                                            crate::workspace::icon_tool(
                                                format!("remove-favorite-{remove}"),
                                                "close",
                                                "取消收藏",
                                            )
                                            .disabled(f.busy)
                                            .on_click(
                                                cx.listener(move |state, _, _, cx| {
                                                    state.change_file_paths(
                                                        "DELETE",
                                                        Some(remove.clone()),
                                                        cx,
                                                    )
                                                }),
                                            ),
                                        )
                                    })
                            }),
                    ),
            )
            .when(entries.is_empty(), |d| {
                d.child(div().text_size(px(12.)).child(if f.busy {
                    "正在读取…"
                } else if !saved {
                    "请先保存此连接，再记录访问路径。"
                } else if favorites {
                    "还没有收藏目录。"
                } else {
                    "暂无历史，成功打开文件或目录后会自动记录。"
                }))
            })
            .when(!favorites && !entries.is_empty(), |d| {
                d.child(
                    KitButton::new("clear-path-history")
                        .ghost()
                        .child(div().text_size(px(11.)).child("清空此服务器历史"))
                        .disabled(f.busy)
                        .on_click(cx.listener(|state, _, _, cx| {
                            state.change_file_paths("DELETE", None, cx)
                        })),
                )
            })
    }

    fn download_file_selection(
        &mut self,
        id: String,
        paths: Vec<String>,
        archive: bool,
        cx: &mut Context<Self>,
    ) {
        let filename = if archive {
            if paths.len() == 1 {
                format!("{}.tar.gz", paths[0].rsplit('/').next().unwrap_or("files"))
            } else {
                "DevShell-files.tar.gz".into()
            }
        } else {
            paths[0].rsplit('/').next().unwrap_or("download").to_owned()
        };
        let destination = cx.prompt_for_new_path(
            &std::env::current_dir().unwrap_or_default(),
            Some(&filename),
        );
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(destination))) = destination.await else {
                return;
            };
            let allowed = this
                .update(cx, |state, _| {
                    !state.snapshot.locked
                        && state
                            .workspace
                            .terminal
                            .as_ref()
                            .is_some_and(|t| t.session_id == id && t.input_tx.is_some())
                })
                .unwrap_or(false);
            if !allowed {
                return;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    let endpoint = if archive {
                        format!(
                            "/api/sessions/{id}/archive?{}",
                            paths
                                .iter()
                                .map(|p| format!("path={}", query_escape(p)))
                                .collect::<Vec<_>>()
                                .join("&")
                        )
                    } else {
                        format!(
                            "/api/sessions/{id}/download?path={}",
                            query_escape(&paths[0])
                        )
                    };
                    download_to_path(&base, &token, &endpoint, &destination)
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                state.feedback = Some(match result {
                    Ok(()) => "下载已保存".into(),
                    Err(error) => error,
                });
                cx.notify();
            });
        })
        .detach();
    }
}

fn download_to_path(
    base: &str,
    token: &str,
    path: &str,
    destination: &std::path::Path,
) -> Result<(), String> {
    let authority = base.strip_prefix("http://").ok_or("后端地址无效")?;
    let mut stream = TcpStream::connect(authority).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(120)))
        .map_err(|e| e.to_string())?;
    write!(stream,"GET {path} HTTP/1.1\r\nHost: {authority}\r\nX-CloudShell-Token: {token}\r\nConnection: close\r\n\r\n").map_err(|e|e.to_string())?;
    let mut reader = std::io::BufReader::new(stream);
    http::download_to_file(&mut reader, destination).map(|_| ())
}
