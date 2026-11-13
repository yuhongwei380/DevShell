//! One immutable destination and one conflict decision per native selection/drop.
use super::*;
use crate::{
    forms::string,
    workspace::{Palette, icon, icon_tool},
};
use gpui_kit::Div;

impl DevShellWindow {
    pub(super) fn queue_local_selection(
        &mut self,
        owner: String,
        directory: String,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.snapshot.locked
            || self.form.is_some()
            || paths.is_empty()
            || !self.workspace.terminal.as_ref().is_some_and(|t| {
                t.session_id == owner && t.protocol == "ssh" && t.input_tx.is_some()
            })
        {
            return;
        }
        self.pending_dialog = Some((
            "file-upload",
            serde_json::json!({"_sessionId":owner,"directory":directory,"paths":paths.iter().map(|path|path.to_string_lossy().to_string()).collect::<Vec<_>>()}),
        ));
        cx.notify();
    }

    pub(super) fn check_local_selection(&mut self, cx: &mut Context<Self>) {
        let f = self.form.as_mut().unwrap();
        f.busy = true;
        let owner = string(&f.original, "_sessionId");
        let serial = f.original["_nativeSerial"].clone();
        let body =
            serde_json::json!({"paths":f.original["paths"],"directory":f.original["directory"]});
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    post_json(
                        &base,
                        &token,
                        &format!("/api/sessions/{owner}/upload-check"),
                        body,
                    )
                })
                .await;
            let _ = this.update(cx, |state, cx| {
                let Some(f) = state
                    .form
                    .as_mut()
                    .filter(|f| f.original["_nativeSerial"] == serial)
                else {
                    return;
                };
                f.busy = false;
                match result {
                    Ok(data) => {
                        if !data.is_object()
                            || !data
                                .get("conflicts")
                                .is_some_and(|v| v.is_null() || v.is_array())
                        {
                            f.error = Some("上传预检响应无效，本次上传未开始。".into());
                            cx.notify();
                            return;
                        }
                        let empty = data["conflicts"].as_array().is_none_or(Vec::is_empty);
                        f.original["_conflicts"] = data["conflicts"].clone();
                        if empty {
                            state.submit_local_selection(false, cx);
                        }
                    }
                    Err(error) => f.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn submit_local_selection(&mut self, overwrite: bool, cx: &mut Context<Self>) {
        let Some(f) = self.form.as_ref().filter(|f| !f.busy) else {
            return;
        };
        let owner = string(&f.original, "_sessionId");
        if self.snapshot.locked
            || !self
                .workspace
                .terminal
                .as_ref()
                .is_some_and(|t| t.session_id == owner && t.input_tx.is_some())
        {
            self.form.as_mut().unwrap().error =
                Some("原 SSH 连接已切换、断开或软件已锁定，本次上传未开始。".into());
            cx.notify();
            return;
        }
        let conflicts = f.original["_conflicts"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if overwrite && conflicts.iter().any(|c| c["canOverwrite"] != true) {
            return;
        }
        let targets = conflicts
            .iter()
            .filter_map(|c| c["path"].as_str())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let paths = f.original["paths"].clone();
        let directory = f.original["directory"].clone();
        let serial = f.original["_nativeSerial"].clone();
        let base = self.snapshot.base.clone();
        let token = self.token.clone();
        self.form.as_mut().unwrap().busy = true;
        self.form.as_mut().unwrap().original["_sending"] = Value::Bool(true);
        cx.spawn(async move|this,cx|{let result_owner=owner.clone();let result=cx.background_executor().spawn(async move{post_json(&base,&token,&format!("/api/sessions/{owner}/upload-local"),serde_json::json!({"paths":paths,"directory":directory,"overwrite":false,"overwriteTargets":if overwrite{targets.clone()}else{Vec::new()},"skipTargets":if overwrite{Vec::new()}else{targets}}))?;load_transfers(&base,&token,&owner)}).await;
            let _=this.update(cx,|state,cx|{let current=state.form.as_ref().is_some_and(|f|f.original["_nativeSerial"]==serial);match result{Ok(transfers)=>{if let Some(session)=state.session_mut(&result_owner){session.transfers=transfers;}if current{state.form=None;}state.feedback=Some("文件已加入上传队列。".into());},Err(error)=>{if current{let f=state.form.as_mut().unwrap();f.busy=false;f.original["_sending"]=Value::Bool(false);f.error=Some(error);}else{state.feedback=Some(error);}}}cx.notify();});
        }).detach();
        cx.notify();
    }

    pub(super) fn local_upload_dialog(
        &self,
        p: Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let f = self.form.as_ref().unwrap();
        let conflicts = f.original["_conflicts"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let blocked = conflicts.iter().any(|c| c["canOverwrite"] != true);
        div().absolute().inset_0().occlude().flex().items_center().justify_center().child(div().w(px(580.)).max_w(window.viewport_size().width-px(32.)).max_h(window.viewport_size().height-px(32.)).p(px(22.)).rounded(px(19.)).bg(gpui_kit::rgb(p.surface)).text_color(gpui_kit::rgb(p.text)).shadow_lg().occlude().flex().flex_col()
            .child(div().flex().items_center().justify_between().gap(px(16.)).mb(px(12.)).child(div().text_size(px(17.)).line_height(px(24.)).font_weight(gpui_kit::FontWeight(650.)).child(if conflicts.is_empty(){"正在检查同名文件…".into()}else{format!("发现 {} 个同名项目",conflicts.len())})).child(icon_tool("upload-close","close","取消上传").size(px(30.)).disabled(f.original["_sending"]==true).on_click(cx.listener(|state,_,_,cx|{if state.form.as_ref().is_some_and(|f|f.original["_sending"]!=true){state.form=None;cx.notify();}}))))
            .child(div().text_size(px(12.)).line_height(px(19.2)).mb(px(14.)).text_color(gpui_kit::rgb(p.muted)).child(format!("{} · {}",self.workspace.terminal.as_ref().map(|t|t.name.as_str()).unwrap_or("服务器"),string(&f.original,"directory"))))
            .child(div().text_size(px(12.)).line_height(px(19.2)).mb(px(14.)).text_color(gpui_kit::rgb(if self.ui.dark{0xb6c5cf}else{0x344f62})).child(if conflicts.is_empty(){"正在检查目标位置，确认后开始上传。"}else if blocked{"目标中包含目录、符号链接或类型不同的项目，不能直接覆盖。可跳过这些同名项目，上传其余文件。"}else{"以下文件已存在。覆盖会替换远程文件内容，本次选择统一处理。"}))
            .when(!conflicts.is_empty(),|d|d.child(div().id("upload-conflict-list").min_h_0().max_h(px(240.)).overflow_y_scroll().px(px(12.)).py(px(4.)).border_1().border_color(gpui_kit::rgb(p.border)).rounded(px(10.)).bg(gpui_kit::rgb(p.surface)).children(conflicts.iter().take(100).enumerate().map(|(i,item)|div().flex().items_start().gap(px(9.)).py(px(10.)).when(i>0,|d|d.border_t_1().border_color(gpui_kit::rgb(p.border))).child(icon("file",16.)).child(div().flex_1().min_w_0().text_size(px(12.)).line_height(px(18.)).child(string(item,"path"))).when(item["canOverwrite"]!=true,|d|d.child(div().text_size(px(10.)).child("不可覆盖")))))))
            .when(conflicts.len()>100,|d|d.child(div().mt(px(10.)).text_size(px(12.)).child(format!("仅列出前 100 项，本次选择对全部 {} 个同名项目生效。",conflicts.len()))))
            .when(f.error.is_some(),|d|d.child(f.error_panel()))
            .child(div().flex().justify_end().gap(px(10.)).mt(px(18.)).flex_wrap().child(KitButton::new("upload-cancel").ghost().h(px(38.)).px(px(18.)).rounded(px(9.)).text_color(gpui_kit::rgb(p.text)).child(div().text_size(px(12.)).child("取消上传")).disabled(f.original["_sending"]==true).on_click(cx.listener(|state,_,_,cx|{if state.form.as_ref().is_some_and(|f|f.original["_sending"]!=true){state.form=None;cx.notify();}})))
                .when(!conflicts.is_empty(),|d|d.child(KitButton::new("upload-skip").ghost().h(px(38.)).px(px(18.)).rounded(px(9.)).text_color(gpui_kit::rgb(p.text)).child(div().text_size(px(12.)).child("跳过同名文件")).disabled(f.busy).on_click(cx.listener(|state,_,_,cx|state.submit_local_selection(false,cx))))
                    .when(!blocked,|d|d.child(KitButton::new("upload-overwrite").primary().h(px(38.)).px(px(18.)).rounded(px(9.)).child(div().text_size(px(12.)).child("覆盖并上传")).disabled(f.busy).on_click(cx.listener(|state,_,_,cx|state.submit_local_selection(true,cx)))))))
        )
    }
}
