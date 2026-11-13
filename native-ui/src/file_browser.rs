use super::*;

impl DevShellWindow {
    pub(super) fn choose_upload(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self
            .workspace
            .terminal
            .as_ref()
            .map(|t| t.session_id.clone())
        else {
            return;
        };
        let Some(path) = self
            .workspace
            .files
            .as_ref()
            .map(|files| files.path.clone())
        else {
            return;
        };
        let selection=cx.prompt_for_paths(gpui_kit::PathPromptOptions{files:true,directories:true,multiple:true,prompt:Some("选择上传到当前远程目录的文件或文件夹".into())});
        cx.spawn(async move|this,cx|{let Ok(Ok(Some(paths)))=selection.await else{return;};let _=this.update(cx,|state,cx|state.queue_local_selection(session_id,path,paths,cx));}).detach();
    }

}

pub(super) fn size(bytes: i64) -> String {
    if bytes >= 1048576 {
        format!("{:.1} MB", bytes as f64 / 1048576.)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} B")
    }
}

// Folder placement is independent of the requested direction, matching the web table.
pub(super) fn compare_entries(
    left: &RemoteEntry,
    right: &RemoteEntry,
    key: &str,
    descending: bool,
) -> std::cmp::Ordering {
    let folder = (left.kind != "folder").cmp(&(right.kind != "folder"));
    if !folder.is_eq() {
        return folder;
    }
    let order = match key {
        "size" => left.bytes.cmp(&right.bytes),
        "time" => left.modified_at.cmp(&right.modified_at),
        _ => crate::collation::compare(&left.name,&right.name,false),
    }
    .then_with(|| crate::collation::compare(&left.name,&right.name,false));
    if descending { order.reverse() } else { order }
}
