use super::*;
use crate::forms::string;
use std::collections::{BTreeMap,BTreeSet};

pub(super) struct NativeFonts {pub families:BTreeMap<String,String>,loading:BTreeSet<String>}
impl Default for NativeFonts {
    fn default()->Self {Self{families:appearance::FONTS.iter().map(|(id,_,family)|(id.to_string(),family.to_string())).chain(std::iter::once(("builtin:ui-alibaba-puhuiti-3-55".into(),"Alibaba PuHuiTi 3.0".into()))).collect(),loading:BTreeSet::new()}}
}
pub(super) fn decode_font(bytes:Vec<u8>)->Result<(Vec<u8>,String),String>{
    if bytes.len()>64*1024*1024{return Err("字体文件不能超过 64 MB".into());}
    let expanded=if bytes.starts_with(b"wOF2")||bytes.starts_with(b"wOFF") {
        let limit=bytes.get(16..20).and_then(|s|s.try_into().ok()).map(u32::from_be_bytes).unwrap_or(u32::MAX);
        if limit>128*1024*1024{return Err("展开后的字体文件过大".into());}
        if bytes.starts_with(b"wOF2"){wuff::decompress_woff2(&bytes)}else{wuff::decompress_woff1(&bytes)}.map_err(|e|format!("字体解码失败：{e:?}"))?
    }else{bytes};
    let face=ttf_parser::Face::parse(&expanded,0).map_err(|e|format!("字体文件无效：{e:?}"))?;
    let family=[ttf_parser::name_id::TYPOGRAPHIC_FAMILY,ttf_parser::name_id::FAMILY].iter().find_map(|id|face.names().into_iter().filter(|n|n.name_id==*id).find_map(|name|name.to_string())).ok_or("字体缺少可读的家族名称")?;
    Ok((expanded,family))
}
impl DevShellWindow {
    pub(super) fn open_font_library(&mut self,kind:&'static str,window:&mut Window,cx:&mut Context<Self>){
        self.open_native_form("font-library",serde_json::json!({"_fontKind":kind}),window,cx);let serial=self.ui.manager.serial;let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{get_json(&base,&token,"/api/font-library")}).await;let _=this.update(cx,|state,cx|{if let Some(f)=state.form.as_mut().filter(|f|f.original["_nativeSerial"].as_u64()==Some(serial)){match result{Ok(value)=>f.original["_catalog"]=value,Err(error)=>f.error=Some(error)}}cx.notify();});}).detach();
    }
    pub(super) fn font_library_fields(&self,cx:&mut Context<Self>)->gpui_kit::Div{
        use gpui_kit::component::Sizable;
        let f=self.form.as_ref().unwrap();let kind=string(&f.original,"_fontKind");let fonts=f.original["_catalog"]["fonts"].as_array().cloned().unwrap_or_default();
        div().flex().flex_col().gap_3().children(fonts.iter().filter(|font|string(font,"kind")==kind).map(|font|{
            let font=font.clone();let builtin=string(&font,"builtinId");let apply=builtin.clone();let asset=self.snapshot.config["assets"].as_array().and_then(|assets|assets.iter().find(|asset|asset["libraryId"]==font["id"])).map(|asset|string(asset,"id"));
            let kind=kind.clone();div().p_3().rounded_lg().border_1().flex().flex_col().gap_2().child(heading(string(&font,"name"))).child(paragraph(string(&font,"description")))
                .child(div().text_size(px(11.)).child(format!("{} · {}",string(&font,"licenseName"),string(&font,"version"))))
                .child(KitButton::new(format!("library-font-{}",string(&font,"id"))).secondary().small().label(if !builtin.is_empty(){"使用内置字体"}else if asset.is_some(){"使用已下载字体"}else{"查看许可并下载"}).on_click(cx.listener(move|state,_,window,cx|{
                    if let Some(id)=asset.clone().or_else(||if apply.is_empty(){None}else{Some(apply.clone())}){state.form=None;state.save_appearance(if kind=="ui-font"{serde_json::json!({"uiFontId":id})}else{serde_json::json!({"fontId":id})},cx);}
                    else{state.open_native_form("font-license",font.clone(),window,cx);}cx.notify();
                })))
        }))
    }
    pub(super) fn download_library_font(&mut self,cx:&mut Context<Self>){
        let Some(f)=self.form.as_mut()else{return;};if f.busy{return;}f.busy=true;let font=string(&f.original,"id");let serial=f.original["_nativeSerial"].clone();let kind=string(&f.original,"kind");let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let start_base=base.clone();let start_token=token.clone();let started=cx.background_executor().spawn(async move{post_json(&start_base,&start_token,"/api/font-library/downloads",serde_json::json!({"fontId":font}))}).await;
            let mut job=match started{Ok(job)=>job,Err(error)=>{let _=this.update(cx,|state,cx|{if let Some(f)=state.form.as_mut().filter(|f|f.original["_nativeSerial"]==serial){f.busy=false;f.error=Some(error);}cx.notify();});return;}};
            loop {
                let mut alive=false;let _=this.update(cx,|state,cx|{if let Some(f)=state.form.as_mut().filter(|f|f.original["_nativeSerial"]==serial){f.original["_fontJob"]=job.clone();alive=true;cx.notify();}});
                if !alive{let id=string(&job,"id");let cancel_base=base.clone();let cancel_token=token.clone();cx.background_executor().spawn(async move{let _=delete_json(&cancel_base,&cancel_token,&format!("/api/font-library/downloads/{id}"));}).await;return;}
                if job["status"].as_str()!=Some("downloading"){break;}
                cx.background_executor().timer(Duration::from_millis(400)).await;let poll_base=base.clone();let poll_token=token.clone();let id=string(&job,"id");
                match cx.background_executor().spawn(async move{get_json(&poll_base,&poll_token,&format!("/api/font-library/downloads/{id}"))}).await{Ok(value)=>job=value,Err(error)=>{job=serde_json::json!({"status":"error","error":error});}}
            }
            let snapshot=if job["status"].as_str()==Some("complete"){Some(cx.background_executor().spawn(async move{BackendSnapshot::load(&base,&token)}).await)}else{None};
            let _=this.update(cx,|state,cx|{if let Some(snapshot)=snapshot{state.snapshot=snapshot;state.form=None;state.save_appearance(if kind=="ui-font"{serde_json::json!({"uiFontId":job["asset"]["id"]})}else{serde_json::json!({"fontId":job["asset"]["id"]})},cx);}else if let Some(f)=state.form.as_mut().filter(|f|f.original["_nativeSerial"]==serial){f.busy=false;f.error=Some(job["error"].as_str().unwrap_or("下载已取消").into());}cx.notify();});
        }).detach();cx.notify();
    }
    pub(super) fn load_native_fonts(&mut self,cx:&mut Context<Self>){
        let assets=self.snapshot.config["assets"].as_array().cloned().unwrap_or_default();
        for asset in assets {
            let id=string(&asset,"id");if self.fonts.families.contains_key(&id)||self.fonts.loading.contains(&id){continue;}
            if string(&asset,"kind")=="ui-font"&&!self.snapshot.ui_font_runtime["availableIds"].as_array().is_some_and(|ids|ids.iter().any(|item|item.as_str()==Some(&id))){continue;}
            self.fonts.loading.insert(id.clone());let base=self.snapshot.base.clone();let token=self.token.clone();
            cx.spawn(async move|this,cx|{let source=id.clone();let result=cx.background_executor().spawn(async move{
                let value=get_json(&base,&token,&format!("/api/assets/{source}/data"))?;
                let data=value["dataUrl"].as_str().and_then(|s|s.split_once(";base64,").map(|(_,s)|s)).ok_or("字体内容格式无效")?;
                decode_font(base64::engine::general_purpose::STANDARD.decode(data).map_err(|e|e.to_string())?)
            }).await;
                let _=this.update(cx,|state,cx|{match result {Ok((bytes,family))=>{match cx.text_system().add_fonts(vec![std::borrow::Cow::Owned(bytes)]){Ok(())=>{state.fonts.families.insert(id,family);},Err(error)=>state.feedback=Some(format!("字体无法注册：{error}"))}},Err(error)=>state.feedback=Some(error)}cx.notify();});
            }).detach();
        }
    }
    pub(super) fn import_font(&mut self,kind:&'static str,cx:&mut Context<Self>){
        let chooser=cx.prompt_for_paths(gpui_kit::PathPromptOptions{files:true,directories:false,multiple:false,prompt:Some("选择 TTF / OTF / WOFF / WOFF2 字体".into())});let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let Ok(Ok(Some(paths)))=chooser.await else{return;};let Some(path)=paths.first().cloned()else{return;};let result=cx.background_executor().spawn(async move{
            let bytes=fs::read(&path).map_err(|e|e.to_string())?;decode_font(bytes.clone())?;
            let asset=post_json(&base,&token,"/api/assets",serde_json::json!({"kind":kind,"filename":path.file_name().map(|s|s.to_string_lossy()).unwrap_or_default(),"name":path.file_stem().map(|s|s.to_string_lossy()).unwrap_or_default(),"data":base64::engine::general_purpose::STANDARD.encode(bytes)}))?;
            Ok::<_,String>((asset,BackendSnapshot::load(&base,&token)))
        }).await;let _=this.update(cx,|state,cx|{match result{Ok((asset,snapshot))=>{state.snapshot=snapshot;state.form=None;state.save_appearance(if kind=="ui-font"{serde_json::json!({"uiFontId":asset["id"]})}else{serde_json::json!({"fontId":asset["id"]})},cx);state.feedback=Some(if kind=="ui-font"{"界面字体已导入，完整退出并重启后生效"}else{"终端字体已导入并应用"}.into());},Err(error)=>state.feedback=Some(error)}cx.notify();});}).detach();
    }
}
