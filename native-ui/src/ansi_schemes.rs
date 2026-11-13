//! Port of the pinned web/js/ansi-schemes.js and css/ansi-schemes.css.
use super::*;
use crate::{appearance::{ANSI_KEYS, color}, forms::string, workspace::{Palette, icon, icon_tool}};
use gpui_kit::Div;

fn dictionary<'a,'input>(node: roxmltree::Node<'a,'input>) -> std::collections::BTreeMap<String,roxmltree::Node<'a,'input>> {
    let mut result=std::collections::BTreeMap::new();
    let mut key=None;
    for child in node.children().filter(|n|n.is_element()) {
        if child.has_tag_name("key") {key=Some(child.text().unwrap_or("").to_owned());}
        else if let Some(key)=key.take() {result.insert(key,child);}
    }
    result
}
fn rgb(node: roxmltree::Node<'_, '_>) -> Option<String> {
    if !node.has_tag_name("dict") {return None;}
    let entries=dictionary(node);
    let mut channels=Vec::new();
    for key in ["Red Component","Green Component","Blue Component"] {
        let value=entries.get(key)?.text()?.trim().parse::<f64>().ok()?;
        if !value.is_finite(){return None;}
        channels.push((value.clamp(0.,1.)*255.).round() as u8);
    }
    Some(format!("#{:02x}{:02x}{:02x}",channels[0],channels[1],channels[2]))
}
pub(super) fn parse_iterm(text: &str) -> Result<Value,String> {
    if text.len()>256*1024 {return Err("配色文件太大，请选择更小的 .itermcolors 文件".into());}
    let document=roxmltree::Document::parse_with_options(text.trim_start_matches('\u{feff}'),roxmltree::ParsingOptions{allow_dtd:true,nodes_limit:20000,entity_resolver:None}).map_err(|_|"无法解析 iTerm 配色文件".to_owned())?;
    let root=document.root_element();
    let top=root.children().find(|n|n.is_element()).filter(|n|root.has_tag_name("plist")&&n.has_tag_name("dict")).ok_or_else(||"不是有效的 iTerm 配色文件".to_owned())?;
    let entries=dictionary(top);let mut colors=serde_json::Map::new();
    for (index,key) in ANSI_KEYS.iter().enumerate() {
        let label=format!("Ansi {index} Color");
        let value=entries.get(&label).and_then(|n|rgb(*n)).ok_or_else(||format!("iTerm 配色缺少「{label}」或有效的 RGB 通道"))?;
        colors.insert((*key).into(),value.into());
    }
    for (key,label) in [("foreground","Foreground Color"),("cursor","Cursor Color"),("cursorAccent","Cursor Text Color"),("selectionBackground","Selection Color"),("selectionForeground","Selected Text Color")] {
        if let Some(value)=entries.get(label).and_then(|n|rgb(*n)) {colors.insert(key.into(),value.into());}
    }
    // Match the source's allowlist: background and unrelated plist keys never enter the palette.
    Ok(Value::Object(colors))
}
pub(super) fn import_schemes(mut schemes:Vec<Value>,files:Vec<(String,String)>)->Result<Value,String>{
    let mut active=None;
    for (filename,text) in files {
        let colors=parse_iterm(&text)?;
        let base=if filename.to_ascii_lowercase().ends_with(".itermcolors"){&filename[..filename.len()-12]}else{&filename};
        let name=base.trim().chars().take(48).collect::<String>();let name=if name.is_empty(){"未命名方案".to_owned()}else{name};
        schemes.retain(|s|s["name"]!=name);
        if schemes.len()>=64{return Err("配色方案最多 64 套，请先删除部分方案".into());}
        schemes.push(serde_json::json!({"name":name,"colors":colors}));active=Some(name);
    }
    let active=active.ok_or_else(||"请选择 .itermcolors 文件".to_owned())?;
    Ok(serde_json::json!({"ansiSchemes":schemes,"ansiPalette":"custom","ansiScheme":active}))
}
fn text(value:impl Into<gpui_kit::SharedString>,size:f32)->Div {div().text_size(px(size)).line_height(px(size*1.5)).child(value.into())}
fn action(id:impl Into<gpui_kit::ElementId>,label:&str,p:Palette)->KitButton {KitButton::new(id).ghost().h(px(27.)).px(px(9.)).rounded(px(7.)).border_1().border_color(gpui_kit::rgb(p.border)).text_color(gpui_kit::rgb(p.text)).child(text(label.to_owned(),12.))}

impl DevShellWindow {
    pub(super) fn original_ansi_dialog(&self,p:Palette,window:&Window,cx:&mut Context<Self>)->Div {
        let f=self.form.as_ref().unwrap();
        let schemes=self.snapshot.appearance["ansiSchemes"].as_array().cloned().unwrap_or_default();
        let active=if self.snapshot.appearance["ansiPalette"]=="custom"{string(&self.snapshot.appearance,"ansiScheme")}else{String::new()};
        div().absolute().inset_0().flex().items_center().justify_center().occlude()
        .on_key_down(cx.listener(|state,event:&gpui_kit::KeyDownEvent,_,cx|{
            let Some(name)=state.form.as_ref().and_then(|f|f.original["_rename"].as_str()).map(str::to_owned)else{return;};
            match event.keystroke.key.as_str(){"enter"=>state.rename_ansi_scheme(name,cx),"escape"=>{state.form.as_mut().unwrap().original["_rename"]=Value::Null;cx.notify();},_=>return,}cx.stop_propagation();
        })).child(
            div().id("ansi-dialog").w(px(560.)).max_w(window.viewport_size().width-px(32.)).max_h(window.viewport_size().height-px(32.)).px(px(18.)).py(px(16.)).rounded(px(19.)).bg(gpui_kit::rgb(p.surface)).text_color(gpui_kit::rgb(p.text)).shadow_lg().occlude().flex().flex_col().gap(px(9.))
                .child(div().flex().items_start().justify_between().mb(px(10.)).child(text("配色方案",16.).font_weight(gpui_kit::FontWeight(550.))).child(icon_tool("ansi-close","close","关闭配色方案").size(px(30.)).disabled(f.busy).on_click(cx.listener(|state,_,_,cx|state.dismiss_native_form(cx)))))
                .child(text("支持导入多套 .itermcolors 配色方案，名称沿用导入文件名，同名导入即覆盖；应用其 16 色 ANSI，文件包含相应键时并应用前景、光标与选区颜色；背景等其他键会被忽略，终端保持透明背景。",11.5).text_color(gpui_kit::rgb(p.muted)))
                .child(div().flex().items_center().justify_between().gap(px(8.)).child(KitButton::new("ansi-import").primary().h(px(29.)).px(px(11.)).rounded(px(8.)).child(icon("plus",14.).text_color(gpui_kit::rgb(0xffffff))).child(text("导入 iTerm 配色…",12.)).disabled(f.busy).on_click(cx.listener(|state,_,_,cx|state.choose_iterm_schemes(cx)))).when(!schemes.is_empty(),|d|d.child(text(format!("已导入 {} 套，最多 64 套",schemes.len()),11.).text_color(gpui_kit::rgb(p.muted)))))
                .child(div().id("ansi-scheme-list").min_h_0().overflow_y_scroll().flex().flex_col().gap(px(6.)).when(schemes.is_empty(),|d|d.child(text("还没有配色方案：点「导入 iTerm 配色…」选择 .itermcolors 文件，方案名称将沿用文件名。",11.5).text_color(gpui_kit::rgb(p.muted)).mb(px(20.)))).children(schemes.iter().map(|scheme|{
                    let name=string(scheme,"name");let selected=name==active;let renaming=f.original["_rename"].as_str()==Some(&name);
                    let rename=name.clone();let apply=name.clone();let delete=name.clone();let save=name.clone();
                    div().px(px(10.)).py(px(7.)).border_1().border_color(gpui_kit::rgb(p.border)).rounded(px(9.)).flex().items_center().justify_between().gap(px(10.)).child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.))
                        .when(!renaming,|d|d.child(text(name.clone(),13.).font_weight(gpui_kit::FontWeight::SEMIBOLD)))
                        .when(renaming,|d|d.child(div().flex().items_center().gap(px(6.)).child(div().flex_1().min_w_0().child(Input::new(&f.inputs["name"]).h(px(29.)).text_size(px(12.5)))).child(action(format!("ansi-rename-save-{name}"),"保存",p).on_click(cx.listener(move|state,_,_,cx|state.rename_ansi_scheme(save.clone(),cx)))).child(action(format!("ansi-rename-cancel-{name}"),"取消",p).on_click(cx.listener(|state,_,_,cx|{state.form.as_mut().unwrap().original["_rename"]=Value::Null;cx.notify();})))))
                        .child(div().flex().gap(px(2.)).children(ANSI_KEYS.iter().map(|key|div().size(px(9.)).rounded(px(2.)).border_1().border_color(gpui_kit::rgba(0x0000002e)).bg(gpui_kit::rgb(color(&scheme["colors"][key]).unwrap_or(p.border))))))
                        .child(text(if selected{"当前应用 · 16 色 ANSI + 前景/光标/选区"}else{"16 色 ANSI + 前景/光标/选区"},11.).text_color(gpui_kit::rgb(p.muted))))
                        .child(div().flex().items_center().gap(px(6.))
                            .child(action(format!("ansi-apply-{name}"),if selected{"已应用"}else{"应用"},p).primary().text_color(gpui_kit::rgb(0xffffff)).disabled(selected||f.busy).on_click(cx.listener(move|state,_,_,cx|state.save_ansi_patch(serde_json::json!({"ansiPalette":"custom","ansiScheme":apply}),cx))))
                            .child(action(format!("ansi-rename-{name}"),"重命名",p).disabled(f.busy).on_click(cx.listener(move|state,_,window,cx|{use gpui_kit::Focusable;let f=state.form.as_mut().unwrap();f.original["_rename"]=Value::from(rename.clone());f.inputs["name"].update(cx,|input,cx|input.set_value(rename.clone(),window,cx));window.focus(&f.inputs["name"].read(cx).focus_handle(cx),cx);cx.notify();})))
                            .child(action(format!("ansi-delete-{name}"),if f.original["_confirmDelete"].as_str()==Some(&name){"确认删除？"}else{"删除"},p).text_color(gpui_kit::rgb(if self.ui.dark{0xff9a8f}else{0xb3261e})).disabled(f.busy).on_click(cx.listener(move|state,_,_,cx|state.remove_ansi_scheme(delete.clone(),cx)))))
                })))
                .when(f.error.is_some(),|d|d.child(f.error_panel()))
        )
    }
    fn save_ansi_patch(&mut self,patch:Value,cx:&mut Context<Self>){
        let Some(f)=self.form.as_mut().filter(|f|f.kind=="ansi"&&!f.busy)else{return;};
        let serial=f.original["_nativeSerial"].clone();f.busy=true;f.error=None;
        let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{post_json(&base,&token,"/api/appearance/patch",patch)}).await;let _=this.update(cx,|state,cx|{match result{Ok(value)=>{state.snapshot.appearance=value.get("appearance").cloned().unwrap_or(value);if let Some(f)=state.form.as_mut().filter(|f|f.original["_nativeSerial"]==serial){f.busy=false;f.original["_rename"]=Value::Null;f.original["_confirmDelete"]=Value::Null;}},Err(error)=>{if let Some(f)=state.form.as_mut().filter(|f|f.original["_nativeSerial"]==serial){f.busy=false;f.error=Some(error);}}}cx.notify();});}).detach();cx.notify();
    }
    fn choose_iterm_schemes(&mut self,cx:&mut Context<Self>){
        if self.form.as_ref().is_none_or(|f|f.busy){return;}
        let serial=self.form.as_ref().unwrap().original["_nativeSerial"].clone();
        let picker=cx.prompt_for_paths(gpui_kit::PathPromptOptions{files:true,directories:false,multiple:true,prompt:Some("选择 .itermcolors 配色方案".into())});
        let schemes=self.snapshot.appearance["ansiSchemes"].as_array().cloned().unwrap_or_default();
        cx.spawn(async move|this,cx|{let Ok(Ok(Some(paths)))=picker.await else{return;};
            let result=cx.background_executor().spawn(async move{let mut files=Vec::new();for path in paths{let metadata=fs::metadata(&path).map_err(|e|e.to_string())?;if metadata.len()>256*1024{return Err("配色文件太大，请选择更小的 .itermcolors 文件".into());}files.push((path.file_name().unwrap_or_default().to_string_lossy().into_owned(),fs::read_to_string(path).map_err(|e|e.to_string())?));}import_schemes(schemes,files)}).await;
            let _=this.update(cx,|state,cx|{if !state.form.as_ref().is_some_and(|f|f.kind=="ansi"&&!f.busy&&f.original["_nativeSerial"]==serial){return;}match result{Ok(patch)=>state.save_ansi_patch(patch,cx),Err(error)=>{state.form.as_mut().unwrap().error=Some(error);cx.notify();}}});
        }).detach();
    }
    fn rename_ansi_scheme(&mut self,previous:String,cx:&mut Context<Self>){
        let Some(f)=self.form.as_ref().filter(|f|!f.busy)else{return;};let name=f.text("name",cx).trim().to_owned();
        let mut schemes=self.snapshot.appearance["ansiSchemes"].as_array().cloned().unwrap_or_default();
        let error=if name.is_empty(){Some("请先输入配色方案名称。".to_owned())}else if name.chars().count()>48{Some("配色方案名称最长 48 个字符。".into())}else if schemes.iter().any(|s|s["name"]==name&&s["name"]!=previous){Some(format!("已有同名方案「{name}」。"))}else{None};
        if let Some(error)=error{self.form.as_mut().unwrap().error=Some(error);cx.notify();return;}
        for scheme in &mut schemes{if scheme["name"]==previous{scheme["name"]=Value::from(name.clone());}}
        let mut patch=serde_json::json!({"ansiSchemes":schemes});if self.snapshot.appearance["ansiScheme"]==previous&&self.snapshot.appearance["ansiPalette"]=="custom"{patch["ansiScheme"]=Value::from(name);}
        self.save_ansi_patch(patch,cx);
    }
    fn remove_ansi_scheme(&mut self,name:String,cx:&mut Context<Self>){
        let Some(f)=self.form.as_mut().filter(|f|!f.busy)else{return;};
        if f.original["_confirmDelete"]!=name{f.original["_confirmDelete"]=Value::from(name.clone());let serial=f.original["_nativeSerial"].clone();cx.spawn(async move|this,cx|{cx.background_executor().timer(Duration::from_secs(4)).await;let _=this.update(cx,|state,cx|{if let Some(f)=state.form.as_mut().filter(|f|f.original["_nativeSerial"]==serial&&f.original["_confirmDelete"]==name){f.original["_confirmDelete"]=Value::Null;cx.notify();}});}).detach();cx.notify();return;}
        let schemes=self.snapshot.appearance["ansiSchemes"].as_array().into_iter().flatten().filter(|s|s["name"]!=name).cloned().collect::<Vec<_>>();
        let mut patch=serde_json::json!({"ansiSchemes":schemes});if self.snapshot.appearance["ansiScheme"]==name&&self.snapshot.appearance["ansiPalette"]=="custom"{patch["ansiPalette"]=Value::from("soft");}
        self.save_ansi_patch(patch,cx);
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    fn fixture()->String{format!("<?xml version=\"1.0\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict>{}</dict></plist>",(0..16).map(|i|format!("<key>Ansi {i} Color</key><dict><key>Red Component</key><real>1.5</real><key>Green Component</key><real>0.5</real><key>Blue Component</key><real>-0.1</real></dict>")).collect::<String>())}
    #[test]fn iterm_parser_clamps_channels_and_keeps_the_original_key_allowlist(){let text=fixture().replace("</dict></plist>","<key>Foreground Color</key><dict><key>Red Component</key><real>0</real><key>Green Component</key><real>0</real><key>Blue Component</key><real>1</real></dict><key>Background Color</key><dict/></dict></plist>");let colors=parse_iterm(&text).unwrap();assert_eq!(colors["black"],"#ff8000");assert_eq!(colors["foreground"],"#0000ff");assert!(colors.get("background").is_none());assert_eq!(colors.as_object().unwrap().len(),17);}
    #[test]fn incomplete_and_non_finite_palettes_are_rejected(){assert!(parse_iterm("<plist><dict/></plist>").is_err());assert!(parse_iterm(&fixture().replace("1.5","NaN")).is_err());assert!(parse_iterm(&"x".repeat(256*1024+1)).is_err());}
    #[test]fn a_batch_replaces_same_names_and_applies_the_last_file(){let existing=vec![serde_json::json!({"name":"first","colors":{}})];let patch=import_schemes(existing,vec![("first.itermcolors".into(),fixture()),("中文.ITermColors".into(),fixture())]).unwrap();assert_eq!(patch["ansiSchemes"].as_array().unwrap().len(),2);assert_eq!(patch["ansiScheme"],"中文");assert_eq!(patch["ansiSchemes"][0]["colors"]["red"],"#ff8000");let full=(0..64).map(|i|serde_json::json!({"name":i.to_string()})).collect();assert!(import_schemes(full,vec![("new.itermcolors".into(),fixture())]).is_err());}
}
