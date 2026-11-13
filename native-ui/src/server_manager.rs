use super::*;
use crate::forms::{NativeForm,merge_profile,string};
use crate::workspace::{Palette,icon,icon_tool};
use gpui_kit::{Div,component::Sizable};
use std::collections::BTreeSet;

pub(super) struct ManagerUi {
    pub trash:bool,
    pub collapsed:BTreeSet<String>,
    pub selected:BTreeSet<String>,
    pub menu:Option<(String,String,gpui_kit::Point<gpui_kit::Pixels>)>,
    pub serial:u64,
    pub group:Option<String>,
    pub descendants:bool,
    pub tree_width:f32,
    pub resizing:bool,
}
impl Default for ManagerUi {fn default()->Self {Self{trash:false,collapsed:Default::default(),selected:Default::default(),menu:None,serial:0,group:None,descendants:true,tree_width:320.,resizing:false}}}

impl DevShellWindow {
    pub(super) fn dismiss_native_form(&mut self,cx:&mut Context<Self>){if self.form.as_ref().is_some_and(|f|f.busy&&f.kind!="font-license"&&!(f.kind=="file-upload"&&f.original["_sending"]!=true)){return;}self.form=self.form.take().and_then(|f|f.return_to).map(|f|*f);cx.notify();}
    pub(super) fn refresh_connection_groups(&mut self,window:&mut Window,cx:&mut Context<Self>){
        if !self.form.as_ref().is_some_and(|f|f.original["_refreshGroups"]==true){return;}
        let mut f=self.form.take().unwrap();f.original.as_object_mut().unwrap().remove("_refreshGroups");
        let groups=group_order(&self.snapshot.config);let options=std::iter::once((String::new(),"未分组".into())).chain(groups.iter().map(|(g,_)|(string(g,"id"),group_choice_label(&groups,g)))).collect();
        f.add_choices("groupId",options,&string(&f.original,"groupId"),window,cx);self.form=Some(f);
    }

    pub(super) fn server_context_menu(&self,p:Palette,window:&mut Window,cx:&mut Context<Self>)->Div {
        let (kind,id,point)=self.ui.manager.menu.as_ref().unwrap();
        let target=if kind=="group" {self.snapshot.config.get("groupNodes").and_then(Value::as_array).and_then(|groups|groups.iter().find(|g|string(g,"id")==*id)).cloned().unwrap_or_default()}else{profile_by_id(&self.snapshot.config,id)};
        let actions:Vec<(&'static str,&'static str)>=if kind=="group" {vec![("child","新建子分组"),("group","编辑分组"),("group-keep","删除分组（保留连接）"),("group-delete","删除整个分组")]}else if self.ui.manager.trash {vec![("restore","恢复连接"),("purge-profile","永久删除")]}else{vec![("connect","连接"),("connection","编辑连接"),("move","移动连接"),("duplicate","复制连接"),("trash-profile","删除连接")]};
        div().absolute().inset_0()
            .child(div().absolute().inset_0().occlude().on_mouse_down(gpui_kit::MouseButton::Left,cx.listener(|state,_,_,cx|{state.ui.manager.menu=None;cx.notify();})))
            .child(div().absolute().left(point.x.min(window.viewport_size().width-px(210.))).top(point.y.min(window.viewport_size().height-px(220.))).w(px(200.)).p_2().rounded(px(13.)).bg(gpui_kit::rgb(p.surface)).border_1().border_color(gpui_kit::rgb(p.border)).shadow_lg().occlude().flex().flex_col().gap_1().children(actions.into_iter().map(|(action,label)|{
                let target=target.clone();let id=id.clone();
                KitButton::new(format!("context-{action}")).ghost().small().w_full().justify_start().label(label).on_click(cx.listener(move|state,_,window,cx|{
                    state.ui.manager.menu=None;
                    match action {
                        "connect"=>{if let Some(server)=state.find_profile(&id){state.connect_profile(server,cx);}},
                        "restore"=>state.config_action("POST",format!("/api/trash/{id}/restore"),serde_json::json!({}),cx),
                        "duplicate"=>state.config_action("POST",format!("/api/profiles/{id}/duplicate"),serde_json::json!({"name":format!("{} 副本",string(&target,"name"))}),cx),
                        "child"=>state.open_native_form("group",serde_json::json!({"parentId":id}),window,cx),
                        "group-delete"=>{
                            state.open_native_form("group-delete",target.clone(),window,cx);
                            let serial=state.ui.manager.serial;let base=state.snapshot.base.clone();let token=state.token.clone();let id=id.clone();
                            cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{get_json(&base,&token,&format!("/api/group-nodes/{id}/deletion"))}).await;
                                let _=this.update(cx,|state,cx|{if state.ui.manager.serial==serial {if let Some(form)=state.form.as_mut(){match result {Ok(plan)=>form.original["_deletionPlan"]=plan,Err(error)=>form.error=Some(error)}}}cx.notify();});
                            }).detach();
                        },
                        _=>state.open_native_form(action,target.clone(),window,cx),
                    }cx.notify();
                }))
            })))
    }
    pub(super) fn server_manager(&self,p:Palette,window:&mut Window,cx:&mut Context<Self>)->Div {
        use gpui_kit::component::checkbox::Checkbox;
        let trash=self.ui.manager.trash;
        let profiles=self.snapshot.config.get(if trash {"trash"}else{"servers"}).and_then(Value::as_array).cloned().unwrap_or_default();
        let query=self.ui.search.read(cx).value().to_lowercase();let groups=group_order(&self.snapshot.config);
        let active=self.ui.manager.group.clone().unwrap_or_else(||groups.first().map(|(g,_)|string(g,"id")).unwrap_or_default());
        let title=groups.iter().find(|(g,_)|string(g,"id")==active).map(|(g,_)|string(g,"name")).unwrap_or("未分组".into());
        let scope=group_scope(&groups,&active,self.ui.manager.descendants);
        let mut members=profiles.iter().filter(|profile|(query.is_empty()&&scope.contains(&string(profile,"groupId")))||(!query.is_empty()&&(profile_matches(profile,&query)||group_path(&groups,&string(profile,"groupId")).to_lowercase().contains(&query)))).collect::<Vec<_>>();
        members.sort_by_key(|p|string(p,"name").to_lowercase());
        let width=((f32::from(window.viewport_size().width)/crate::ui_scale::current())-32.).min(700.);let tree=self.ui.manager.tree_width.clamp(150.,(width-230.).max(150.));
        div().absolute().top(px(60.)).bottom(px(32.)).left(px(((f32::from(window.viewport_size().width)/crate::ui_scale::current())-width)/2.)).w(px(width)).occlude().rounded(px(18.)).bg(gpui_kit::rgb(p.surface)).shadow_lg().px_3().py(px(10.)).flex().flex_col()
            .on_mouse_move(cx.listener(move|state,event:&gpui_kit::MouseMoveEvent,window,cx|{if state.ui.manager.resizing {let left=((f32::from(window.viewport_size().width)/crate::ui_scale::current())-width)/2.+12.;state.ui.manager.tree_width=((f32::from(event.position.x)/crate::ui_scale::current())-left).clamp(150.,(width-230.).max(150.));cx.notify();}}))
            .on_mouse_up(gpui_kit::MouseButton::Left,cx.listener(|state,_,_,cx|{state.ui.manager.resizing=false;cx.notify();}))
            .child(div().flex_shrink_0().flex().items_center().gap_3().mb(px(7.)).child(div().text_size(px(15.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child("服务器管理"))
                .child(div().flex_1().p(px(3.)).rounded(px(8.)).bg(gpui_kit::rgb(if self.ui.dark{0x131a21}else{0xeef3f6})).flex().gap(px(2.)).children([(false,"服务器","servers"),(true,"已删除","trash")].map(|(value,label,key)|{
                    let count=self.snapshot.config.get(key).and_then(Value::as_array).map_or(0,Vec::len);
                    KitButton::new(format!("manager-tab-{key}")).ghost().flex_1().h(px(28.)).rounded(px(6.)).text_size(px(11.)).label(format!("{label}  {count}")).when(value==trash,|b|b.bg(gpui_kit::rgb(p.surface))).on_click(cx.listener(move|state,_,_,cx|{state.ui.manager.trash=value;state.ui.manager.selected.clear();cx.notify();}))
                })))
                .child(icon_tool("servers-close","close","关闭服务器管理").on_click(cx.listener(|state,_,_,cx|{state.ui.connections_visible=false;state.ui.manager.menu=None;cx.notify();}))))
            .child(div().flex_shrink_0().flex().items_center().gap_2().mb_2()
                .child(div().flex_1().min_w_0().h(px(32.)).flex().items_center().gap_2().rounded(px(7.)).border_1().border_color(gpui_kit::rgb(if self.ui.dark{0x2d3944}else{0xe0e8ef})).bg(gpui_kit::rgb(if self.ui.dark{0x131a21}else{0xf1f5f8})).px(px(9.)).child(icon("search",14.)).child(Input::new(&self.ui.search).appearance(false).bordered(false).h(px(28.)).text_size(px(12.))))
                .child(KitButton::new("open-selected").ghost().small().text_size(px(11.)).disabled(self.ui.manager.selected.is_empty()).label(format!("{} ({})",if trash{"恢复所选"}else{"打开所选"},self.ui.manager.selected.len())).on_click(cx.listener(move|state,_,_,cx|{let ids=state.ui.manager.selected.iter().cloned().collect::<Vec<_>>();for id in ids {if trash {state.config_action("POST",format!("/api/trash/{id}/restore"),serde_json::json!({}),cx);}else if let Some(server)=state.find_profile(&id){state.connect_profile(server,cx);}}}))))
            .child(div().flex_1().min_h_0().rounded(px(10.)).border_1().border_color(gpui_kit::rgb(p.border)).overflow_hidden().flex()
                .child(div().w(px(tree)).flex_shrink_0().min_h_0().bg(gpui_kit::rgb(if self.ui.dark{0x131a21}else{0xf7f9fb})).flex().flex_col()
                    .child(div().h(px(40.)).px_2().flex_shrink_0().flex().items_center().gap_2().border_b_1().border_color(gpui_kit::rgb(p.border)).child(div().flex_1().text_size(px(12.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child("分组目录"))
                        .child(KitButton::new("groups-expand").ghost().xsmall().label("展开").on_click(cx.listener(|state,_,_,cx|{state.ui.manager.collapsed.clear();cx.notify();})))
                        .child(KitButton::new("groups-collapse").ghost().xsmall().label("折叠").on_click(cx.listener(|state,_,_,cx|{state.ui.manager.collapsed=group_order(&state.snapshot.config).iter().map(|(g,_)|string(g,"id")).collect();cx.notify();}))))
                    .child(div().id("server-tree-scroll").flex_1().min_h_0().overflow_y_scroll().p(px(5.)).flex().flex_col()
                        .child(self.manager_group(p,&serde_json::json!({"id":"","name":"未分组"}),0,&active,&groups,&profiles,cx))
                        .children(groups.iter().filter(|(g,_)|!group_ancestors(&groups,&string(g,"id")).iter().any(|id|self.ui.manager.collapsed.contains(id))).map(|(g,depth)|self.manager_group(p,g,*depth,&active,&groups,&profiles,cx)))))
                .child(div().id("manager-divider").w(px(5.)).h_full().flex_shrink_0().cursor_col_resize().border_x_1().border_color(gpui_kit::rgb(p.border)).on_mouse_down(gpui_kit::MouseButton::Left,cx.listener(|state,_,_,cx|{state.ui.manager.resizing=true;cx.notify();})))
                .child(div().flex_1().min_w_0().min_h_0().flex().flex_col()
                    .child(div().px_2().py(px(7.)).border_b_1().border_color(gpui_kit::rgb(p.border)).flex().flex_col().gap_1()
                        .child(div().flex().items_center().justify_between().child(div().text_size(px(16.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child(if query.is_empty(){title}else{"搜索结果".into()})).child(div().text_size(px(11.)).text_color(gpui_kit::rgb(p.muted)).child(format!("{} 个连接",members.len()))))
                        .child(div().text_size(px(11.)).text_color(gpui_kit::rgb(p.muted)).child(group_path(&groups,&active)))
                        .child(div().flex().justify_end().child(Checkbox::new("server-descendants").label("包含子分组").checked(self.ui.manager.descendants).on_click(cx.listener(|state,value:&bool,_,cx|{state.ui.manager.descendants=*value;cx.notify();})))))
                    .child(div().id("server-profile-scroll").flex_1().min_h_0().overflow_y_scroll().p(px(5.)).children(members.iter().map(|profile|self.manager_profile(p,profile,trash,cx)))
                        .when(members.is_empty(),|body|body.child(div().p_6().text_size(px(12.)).text_color(gpui_kit::rgb(p.muted)).child("此分组暂无连接"))))
                    .child(div().px_3().py_2().border_t_1().border_color(gpui_kit::rgb(p.border)).bg(gpui_kit::rgb(p.inset)).flex_shrink_0().flex().flex_col().gap_1().child(div().text_size(px(12.)).child("单击选择，双击打开连接"))
                        .child(div().text_size(px(11.)).text_color(gpui_kit::rgb(p.muted)).child("右键管理连接；可用 Ctrl / ⌘ 多选，再打开所选连接。")))))
            .child(div().flex_shrink_0().pt(px(7.)).flex().items_center().gap_2()
                .when(!trash,|body|body.child(KitButton::new("new-connection").ghost().flex_1().h(px(32.)).bg(gpui_kit::rgb(p.inset)).child(icon("plus",14.)).label("新建连接").on_click(cx.listener(|state,_,window,cx|{let group=state.ui.manager.group.clone().unwrap_or_else(||group_order(&state.snapshot.config).first().map(|(g,_)|string(g,"id")).unwrap_or_default());state.open_native_form("connection",serde_json::json!({"groupId":group}),window,cx);})))
                    .child(KitButton::new("new-group").ghost().small().label("＋ 新建分组").on_click(cx.listener(|state,_,window,cx|state.open_native_form("group",serde_json::json!({}),window,cx)))))
                .when(trash,|body|body.child(KitButton::new("purge-trash").danger().small().label("一键清理").on_click(cx.listener(|state,_,window,cx|state.open_native_form("purge-trash",serde_json::json!({}),window,cx)))))
                .child(KitButton::new("export-backup").ghost().small().label("备份").on_click(cx.listener(|state,_,window,cx|state.open_native_form("backup",serde_json::json!({}),window,cx))))
                .child(KitButton::new("import-backup").ghost().small().label("恢复").on_click(cx.listener(|state,_,_,cx|state.import_backup(cx)))))
    }

    fn manager_group(&self,p:Palette,group:&Value,depth:usize,active:&str,groups:&[(Value,usize)],profiles:&[Value],cx:&mut Context<Self>)->impl IntoElement + use<> {
        let id=string(group,"id");let chosen=id==active;let click=id.clone();let caret=id.clone();let menu=id.clone();let emoji=string(group,"emoji");
        let count=profiles.iter().filter(|profile|group_scope(groups,&id,true).contains(&string(profile,"groupId"))).count();
        let has_children=groups.iter().any(|(g,_)|!id.is_empty()&&string(g,"parentId")==id);let collapsed=self.ui.manager.collapsed.contains(&id);
        div().ml(px(depth as f32*24.)).my(px(3.)).h(px(if depth==0{42.}else{38.})).flex_shrink_0().id(format!("group-row-{id}")).rounded(px(6.)).px(px(3.)).flex().items_center().gap(px(2.)).border_1().border_color(gpui_kit::rgba(0x00000000))
            .bg(gpui_kit::rgb(if chosen {if self.ui.dark {0x213948}else{0xe2edf5}}else if self.ui.dark{if depth==0{0x202b35}else{0x1c242d}}else if depth==0{0xeef3f7}else{0xf2f6f9})).when(chosen,|body|body.border_1().border_color(gpui_kit::rgb(if self.ui.dark{0x86afc9}else{0x547e9b})))
            .on_mouse_down(gpui_kit::MouseButton::Right,cx.listener(move|state,event:&gpui_kit::MouseDownEvent,_,cx|{state.ui.manager.menu=Some(("group".into(),menu.clone(),event.position));cx.notify();}))
            .child(KitButton::new(format!("group-caret-{id}")).ghost().w(px(18.)).h(px(25.)).p_0().label(if !has_children{""}else if collapsed{"›"}else{"⌄"}).on_click(cx.listener(move|state,_,_,cx|{if !state.ui.manager.collapsed.remove(&caret){state.ui.manager.collapsed.insert(caret.clone());}cx.notify();})))
            .child(div().id(format!("group-select-{id}")).px(px(1.)).flex_1().min_w_0().h_full().flex().items_center().gap(px(6.)).cursor_pointer().on_click(cx.listener(move|state,_,_,cx|{state.ui.manager.group=Some(click.clone());state.ui.manager.selected.clear();cx.notify();}))
                .child(if emoji.is_empty(){div().child(icon("folder",18.))}else{div().text_size(px(17.)).child(emoji)})
                .child(div().flex_1().min_w_0().overflow_hidden().text_size(px(if depth==0{15.}else{14.})).font_weight(gpui_kit::FontWeight::BOLD).child(string(group,"name")))
                .child(div().text_size(px(11.)).text_color(gpui_kit::rgb(p.muted)).child(count.to_string())))
            .child(KitButton::new(format!("group-menu-{id}")).ghost().p_0().w(px(20.)).h(px(25.)).label("···").on_click(cx.listener(move|state,event:&gpui_kit::ClickEvent,_,cx|{state.ui.manager.menu=Some(("group".into(),id.clone(),event.position()));cx.notify();})))
    }

    fn manager_profile(&self,p:Palette,profile:&Value,trash:bool,cx:&mut Context<Self>)->impl IntoElement + use<> {
        let id=string(profile,"id");let copy=profile.clone();let menu=id.clone();let selected=self.ui.manager.selected.contains(&id);
        let endpoint=if string(profile,"protocol")=="serial" {format!("串口 · {} · {}bps",string(profile,"serialPort"),string(profile,"baudRate"))}else{format!("{}{}@{}:{}",if string(profile,"protocol")=="rdp"{"RDP · "}else{""},string(profile,"user"),string(profile,"host"),profile.get("port").and_then(Value::as_u64).unwrap_or(22))};
        div().id(format!("profile-row-{id}")).mb_1().px(px(7.)).py(px(6.)).rounded(px(8.)).border_1().border_color(gpui_kit::rgb(p.border)).bg(gpui_kit::rgb(if self.ui.dark{0x131a21}else{0xf7f9fb})).when(selected,|body|body.border_color(gpui_kit::rgb(0x547e9b))).flex().items_center().gap(px(7.)).cursor_pointer()
            .on_mouse_down(gpui_kit::MouseButton::Right,cx.listener(move|state,event:&gpui_kit::MouseDownEvent,_,cx|{state.ui.manager.menu=Some(("profile".into(),menu.clone(),event.position));cx.notify();}))
            .on_click(cx.listener(move|state,event:&gpui_kit::ClickEvent,_,cx|{
                let id=string(&copy,"id");
                if event.modifiers().control||event.modifiers().platform {if !state.ui.manager.selected.remove(&id){state.ui.manager.selected.insert(id);}}
                else {state.ui.manager.selected.clear();state.ui.manager.selected.insert(id.clone());if event.click_count()==2&&!trash {if let Some(server)=state.find_profile(&id){state.connect_profile(server,cx);}}}
                cx.notify();
            }))
            .child(icon("server",17.))
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(3.)).child(div().text_size(px(14.)).line_height(px(20.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child(string(profile,"name"))).child(div().text_size(px(12.)).line_height(px(17.)).text_color(gpui_kit::rgb(p.muted)).child(endpoint))
                .when(!string(profile,"notes").is_empty(),|body|body.child(div().p(px(7.)).rounded(px(5.)).border_1().border_color(gpui_kit::rgb(p.border)).text_size(px(11.)).line_height(px(15.)).text_color(gpui_kit::rgb(p.muted)).child(string(profile,"notes")))))
            .child(div().size(px(5.)).rounded_full().bg(gpui_kit::rgb(0x93aec4))).child(div().w(px(18.)))
    }

    pub(super) fn open_native_form(&mut self,kind:&'static str,mut value:Value,window:&mut Window,cx:&mut Context<Self>) {
        if kind=="connection" {
            if let Some(proxy)=value.get("proxy").cloned(){for(from,to)in[("type","proxyType"),("host","proxyHost"),("port","proxyPort"),("user","proxyUser")] {if let Some(v)=proxy.get(from){value[to]=v.clone();}}}
        }
        let mut form=NativeForm::new(kind,value,window,cx);
        let groups=group_order(&self.snapshot.config);
        let options=std::iter::once((String::new(),"未分组".into())).chain(groups.iter().map(|(group,_)|{let id=string(group,"id");let label=group_choice_label(&groups,group);(id,label)})).collect::<Vec<_>>();
        for key in ["groupId","parentId","targetGroupId"] {form.add_choices(key,options.clone(),&string(&form.original,key),window,cx);}
        let keys=std::iter::once((String::new(),"使用私钥路径".into())).chain(self.snapshot.keys.iter().map(|k|(string(k,"id"),string(k,"name")))).collect();
        form.add_choices("keyId",keys,&string(&form.original,"keyId"),window,cx);
        let proxies=std::iter::once((String::new(),"自定义 / 直连".into())).chain(self.snapshot.proxies.iter().map(|p|(p.id.clone(),p.name.clone()))).collect();
        form.add_choices("proxyId",proxies,&string(&form.original,"proxyId"),window,cx);
        self.ui.manager.serial+=1;form.original["_nativeSerial"]=Value::from(self.ui.manager.serial);
        self.form=Some(form);self.ui.manager.menu=None;if kind=="file-upload"{self.check_local_selection(cx);}cx.notify();
    }

    pub(super) fn native_form_dialog(&self,p:Palette,window:&mut Window,cx:&mut Context<Self>)->Div {
        let form=self.form.as_ref().unwrap();
        if form.kind=="file-upload"{return self.local_upload_dialog(p,window,cx);}
        if form.kind=="file-action"{return self.original_file_action_dialog(p,window,cx);}
        if form.kind=="ansi"{return self.original_ansi_dialog(p,window,cx);}
        if matches!(form.kind,"ui-style"|"charts"|"shell-fonts"|"ui-fonts"){return self.original_appearance_dialog(p,window,cx);}
        let title=match form.kind {"file-action"=>form.original["_title"].as_str().unwrap_or("文件操作"),"file-paths"=>if form.original["_pathsKind"]=="directory-favorites"{"常用目录"}else{"历史路径"},"connection"=>if string(&form.original,"id").is_empty(){"新建连接"}else{"编辑连接"},"group"=>if string(&form.original,"id").is_empty(){"新建分组"}else{"编辑分组"},"move"=>"移动连接","key"=>"密钥管理器","security"=>"安全锁定","credentials"=>"连接凭据","config"=>"连接配置文档","mirror"=>"更新加速域名","help"=>"使用手册与功能引导","font-library"=>"在线字体库","font-license"=>"字体下载与许可","font-info"=>"字体许可","asset-rename"=>"修改字体名称","asset-delete"=>"删除字体","font-color"=>"字体颜色","quick-connect"=>"快速连接","rename-session"=>"重命名标签","shell-fonts"=>"Shell 字体与颜色","ui-fonts"=>"界面字体与文字颜色","ui-style"=>"全局主题","charts"=>"曲线样式","ansi"=>"配色方案","prompt-colors"=>"提示符颜色","backup"=>"备份",_=>"确认操作"};
        let width:f32=if form.kind=="help" {1000.}else if form.kind=="font-library"{760.}else if form.kind=="connection" {800.}else if form.kind=="file-action"{if form.original["_action"]=="permissions"{440.}else{430.}}else{520.};let width=width.min((f32::from(window.viewport_size().width)/crate::ui_scale::current())-32.);
        let panel=div().w(px(width)).max_h(window.viewport_size().height-px(56.)).rounded(px(14.)).bg(gpui_kit::rgb(p.surface)).occlude().shadow_lg().flex().flex_col()
            .child(div().px_5().pt(px(14.)).pb(px(12.)).flex().items_center().justify_between().border_b_1().border_color(gpui_kit::rgb(p.border)).child(div().text_size(px(17.)).line_height(px(23.8)).child(title.to_owned())).child(icon_tool("native-form-close","close","取消").size(px(30.)).disabled(form.busy&&form.kind!="font-license").on_click(cx.listener(|state,_,_,cx|state.dismiss_native_form(cx)))))
            .child(div().id("native-form-body").min_h_0().overflow_y_scroll().px_5().py(px(18.)).flex().flex_col().gap_3()
                .when(form.kind=="file-action",|body|body.child(self.file_action_fields(cx)))
                .when(form.kind=="file-paths",|body|body.child(self.file_paths_fields(p,cx)))
                .when(form.kind=="connection",|body|body.child(self.connection_fields(p,width,cx)))
                .when(form.kind=="help",|body|body.child(self.help_fields(p,cx)))
                .when(form.kind=="config",|body|body.child(paragraph("新建、编辑和成功连接后自动保存，升级后从原位置继续读取。")).child(paragraph(format!("当前配置目录：{}",self.snapshot.config_dir.as_deref().unwrap_or_default()))).child(paragraph("升级应用时在原目录替换程序文件，保留完整 data 配置目录。备份或换电脑时，退出程序后复制整个配置目录，其中包含 devshell.config.enc、devshell.config.key、assets 和 keys。加密密钥不能丢失；外部私钥路径需要另行保留。")))
                .when(form.kind=="mirror",|body|body.child(form.field("updateMirror","更新加速域名")).child(paragraph("留空时直连 GitHub；只影响更新检查与下载。")))
                .when(form.kind=="font-library",|body|body.child(self.font_library_fields(cx)))
                .when(form.kind=="font-license",|body|body.child(heading(string(&form.original,"name"))).child(paragraph(string(&form.original,"description"))).child(paragraph(format!("下载来源：{}",string(&form.original,"project")))).child(paragraph(format!("文件大小：{} bytes",form.original["file"]["size"]))).child(div().text_size(px(11.)).child(string(&form.original,"licenseText"))).when(form.original.get("_fontJob").is_some(),|body|body.child(paragraph(format!("下载进度：{} / {} bytes",form.original["_fontJob"]["received"],form.original["_fontJob"]["total"])))))
                .when(form.kind=="security",|body|body.child(self.security_fields(cx)))
                .when(matches!(form.kind,"shell-fonts"|"ui-fonts"|"ui-style"|"charts"|"ansi"|"prompt-colors"|"font-color"),|body|body.child(self.appearance_fields(cx)))
                .when(form.kind=="credentials",|body|body.child(paragraph(format!("连接 {}",string(&form.original,"name")))).child(form.field("secret",if string(&form.original,"auth")=="key"{"私钥口令"}else{"密码"})))
                .when(form.kind=="quick-connect",|body|body.child(form.field("quickCommand","SSH 命令")).child(form.select("auth","认证方式")).when(form.selected("auth",cx)=="key",|b|b.child(form.select("keyId","选择私钥"))))
                .when(form.kind=="font-info",|body|body.child(paragraph(string(&form.original,"name"))).child(div().text_size(px(13.)).line_height(px(22.)).child(string(&form.original,"licenseText"))))
                .when(form.kind=="asset-rename",|body|body.child(form.field("name","字体名称")))
                .when(form.kind=="asset-delete",|body|body.child(paragraph(format!("删除「{}」？",string(&form.original,"name")))).child(paragraph("删除导入的本机副本，原始文件不受影响。")))
                .when(form.kind=="rename-session",|body|body.child(form.field("name","标签名称")))
                .when(form.kind=="group",|body|body.child(form.field("name","分组名称")).child(form.select("parentId","上级分组")).child(form.field("backgroundColor","分组背景色（留空跟随主题）")).child(form.field("emoji","分组图标")))
                .when(matches!(form.kind,"close-preview"|"open-preview"),|body|body.child(paragraph("文件有未保存的修改，确认放弃修改吗？")))
                .when(form.kind=="move",|body|body.child(form.select("targetGroupId","移动到分组")))
                .when(form.kind=="group-keep",|body|body.child(paragraph("删除此分组，连接与子分组移交给所选分组。" )).child(form.select("targetGroupId","连接与子分组移交给")))
                .when(form.kind=="group-delete",|body|body.child(paragraph("整个分组及其连接将删除。输入分组名称，并完成三次确认。" )).child(form.field("confirmName","分组名称")))
                .when(form.kind=="key",|body|body.child(form.field("name","名称")).child(form.toggle("generate","生成 Ed25519 密钥",cx)).when(!form.flag("generate"),|b|b.child(form.field("sourcePath","私钥文件路径")).child(form.area("privateKey","或粘贴私钥"))).child(form.field("passphrase","私钥口令（可选）")))
                .when(form.kind=="backup",|body|body.child(form.toggle("remember","包括密码、私钥与口令",cx)).child(paragraph("备份文件为明文 JSON，请妥善保管。")))
                .when(matches!(form.kind,"trash-profile"|"purge-profile"|"purge-trash"|"delete-key"),|body|body.child(paragraph(match form.kind {"trash-profile"=>"连接将移入“已删除”，可以恢复。","purge-profile"=>"此连接及其凭据和历史将永久删除，无法恢复。",_=>"全部已删除的连接、凭据和历史将永久清除，无法恢复。"})).child(div().text_size(px(14.)).child(string(&form.original,"name"))))
                .when(form.error.is_some(),|body|body.child(form.error_panel())))
            .child(div().flex_shrink_0().px_5().py_3().border_t_1().border_color(gpui_kit::rgb(p.border)).flex().items_center().gap_2()
                .when(form.kind=="connection"&&!string(&form.original,"id").is_empty(),|bar|bar.child(KitButton::new("reset-host-key").ghost().small().label("重置此服务器的主机指纹").on_click(cx.listener(|state,_,_,cx|{if let Some(form)=&state.form {let id=string(&form.original,"id");state.config_action("POST",format!("/api/profiles/{id}/reset-key"),serde_json::json!({}),cx);}}))))
                .child(div().flex_1())
                .when(form.kind!="connection",|bar|bar.child(KitButton::new("native-form-cancel").ghost().small().label(if form.kind=="font-license"&&form.busy{"取消下载"}else{"取消"}).disabled(form.busy&&form.kind!="font-license").on_click(cx.listener(|state,_,_,cx|state.dismiss_native_form(cx)))))
                .when(!matches!(form.kind,"help"|"font-library"|"font-info"|"file-paths"|"config"),|bar|bar.child(KitButton::new("native-form-submit").primary().h(px(36.)).min_w(px(144.)).rounded(px(10.)).child(div().text_size(px(12.)).line_height(px(16.8)).child(if form.busy {"处理中…"}else if form.kind=="file-action"{match form.original["_action"].as_str(){Some("delete"|"quick-delete")=>"确认删除",Some("permissions")=>"保存权限",Some("new-file"|"mkdir")=>"创建",_=>"确定"}}else if form.kind=="connection" {"保存并连接"}else if matches!(form.kind,"trash-profile"|"purge-profile"|"purge-trash") {"确认删除"}else{"保存"})).disabled(form.busy).on_click(cx.listener(|state,_,_,cx|state.submit_native_form(cx))))));
        div().absolute().inset_0().occlude().flex().items_center().justify_center().when_some(form.return_to.as_ref().filter(|f|matches!(f.kind,"shell-fonts"|"ui-fonts")),|root,f|root.child(self.original_font_panel(f,p,window,cx))).child(div().absolute().inset_0().when(form.kind!="font-color",|d|d.bg(gpui_kit::rgba(0x1f354655)))).child(panel)
    }

    fn connection_fields(&self,p:Palette,width:f32,cx:&mut Context<Self>)->Div {
        let form=self.form.as_ref().unwrap();let protocol=form.selected("protocol",cx);let auth=form.selected("auth",cx);let horizontal=width>690.;
        let section=||div().flex().flex_col().gap(px(10.)).min_w_0();
        div().flex().items_start().gap(px(22.)).when(!horizontal,|row|row.flex_col())
            .child(section().flex_1()
                .child(div().text_size(px(12.)).line_height(px(16.8)).font_weight(gpui_kit::FontWeight::SEMIBOLD).mb(px(2.)).child("连接信息"))
                .child(div().flex().gap_3().child(div().flex_1().min_w_0().child(form.field("name","名称"))).child(div().w(px(165.)).child(form.select("protocol","连接类型"))))
                .when(protocol!="serial",|body|body.child(div().flex().gap_3().child(div().flex_1().min_w_0().child(form.field("host","服务器地址"))).child(div().w(px(80.)).child(form.field("port","端口")))))
                .when(protocol=="serial",|body|body.child(form.field("serialPort","串口")).child(form.field("baudRate","波特率")).child(div().flex().gap_2().children([("dataBits","数据位"),("parity","校验位"),("stopBits","停止位"),("flowControl","流控")].map(|(key,label)|div().flex_1().child(form.select(key,label))))))
                .when(protocol!="serial",|body|body.child(section().mt(px(6.)).pt(px(14.)).border_t_1().border_color(gpui_kit::rgb(p.border))
                    .child(div().text_size(px(12.)).line_height(px(16.8)).font_weight(gpui_kit::FontWeight::SEMIBOLD).mb(px(2.)).child("登录凭据"))
                    .child(div().flex().gap_3().child(div().flex_1().child(form.field("user","用户名"))).child(div().flex_1().child(if protocol=="rdp"{form.field("domain","域（可选）")}else{form.select("auth","认证方式")})))
                    .when(auth=="key"&&protocol!="rdp",|b|b.child(form.select("keyId","选择私钥")).when(form.selected("keyId",cx).is_empty(),|b|b.child(form.field("keyPath","私钥路径"))))
                    .when(auth!="agent"||protocol=="rdp",|b|b.child(form.field("secret",if auth=="key"{"私钥口令"}else{"密码"})).child(form.toggle("remember","在本机保存密码／口令",cx).mt(px(-4.))))
                    .when(form.original.get("hasSecret").and_then(Value::as_bool).unwrap_or(false),|b|b.child(paragraph("已保存凭据。留空保留，取消勾选则清除保存的凭据。")))
                    .when(protocol=="rdp",|b|b.child(form.toggle("rdpClipboard","允许本机与远程桌面共享剪贴板",cx))))))
            .child(section().when(horizontal,|s|s.w(px((width-62.)/2.3)).pl_5().border_l_1().border_color(gpui_kit::rgb(p.border)))
                .child(div().text_size(px(12.)).line_height(px(16.8)).font_weight(gpui_kit::FontWeight::SEMIBOLD).mb(px(2.)).child("分组与选项")).child(div().flex().items_end().gap(px(7.)).child(div().flex_1().min_w_0().child(form.select("groupId","分组"))).child(KitButton::new("connection-new-group").ghost().h(px(36.)).px(px(10.)).rounded(px(8.)).bg(gpui_kit::rgb(p.inset)).child(div().text_size(px(11.)).line_height(px(20.)).child("新建分组")).on_click(cx.listener(|state,_,window,cx|{let previous=state.form.take();state.open_native_form("group",serde_json::json!({}),window,cx);state.form.as_mut().unwrap().return_to=previous.map(Box::new);}))))
                .child(form.disclosure("proxy","连接代理",if form.selected("proxyId",cx).is_empty()&&form.selected("proxyType",cx)=="direct"{"直连"}else{"已配置"},cx).mt(px(6.)))
                .when(form.expanded.contains("proxy"),|body|body.child(form.select("proxyId","使用代理"))
                    .when(form.selected("proxyId",cx).is_empty(),|b|b.child(form.select("proxyType","自定义代理类型")).when(form.selected("proxyType",cx)!="direct",|b|b.child(form.field("proxyHost","代理地址")).child(form.field("proxyPort","端口")).child(form.field("proxyUser","用户名（可选）")).child(form.field("proxyPassword","密码（可选）")).child(form.toggle("clearProxyPassword","清除已保存的代理密码",cx)))))
                .child(form.area("notes","备注（可选）").mt(px(4.))).child(div().mt(px(-4.)).text_size(px(10.)).line_height(px(15.)).text_color(gpui_kit::rgb(p.muted)).child("最多 3 行，每行 20 中文 / 40 英文"))
                .when(protocol.is_empty()||protocol=="ssh",|b|b.child(form.disclosure("autoCommands","连接后自动执行",if form.flag("autoRun"){"已配置"}else{"未配置"},cx).mt(px(6.)))
                    .when(form.expanded.contains("autoCommands"),|b|b.child(form.toggle("autoRun","连接成功后自动执行下列命令",cx)).child(form.area("autoCommands","每行一条命令")))))
    }

    pub(super) fn submit_native_form(&mut self,cx:&mut Context<Self>) {
        let Some(form)=self.form.as_ref()else{return;};if form.busy{return;}
        if form.kind=="file-action"{self.submit_file_action(cx);return;}
        if matches!(form.kind,"shell-fonts"|"ui-fonts"|"ui-style"|"charts"|"ansi"|"prompt-colors"|"font-color"){self.save_appearance_form(cx);return;}
        if matches!(form.kind,"close-preview"|"open-preview"){let path=string(&form.original,"path");let open=form.kind=="open-preview";self.workspace.preview=None;self.workspace.editor=None;self.workspace.preview_sync=None;self.form=None;if open {self.preview_file(path,cx);}cx.notify();return;}
        if form.kind=="quick-connect" {self.quick_connect_command(form.text("quickCommand",cx),form.selected("auth",cx),form.selected("keyId",cx),form.text("secret",cx),cx);return;}
        if form.kind=="rename-session" {let id=string(&form.original,"id");let name=form.text("name",cx);if let Some(t)=self.session_mut(&id).and_then(|s|s.terminal.as_mut()){t.name=name;}self.form=None;cx.notify();return;}
        if form.kind=="mirror" {let mirror=form.text("updateMirror",cx);self.form=None;self.save_appearance(serde_json::json!({"updateMirror":mirror}),cx);cx.notify();return;}
        if form.kind=="font-license" {self.download_library_font(cx);return;}
        if form.kind=="security" {self.save_security_settings(cx);return;}
        if form.kind=="credentials" {let id=string(&form.original,"id");let secret=form.text("secret",cx);self.form=None;if let Some(server)=self.find_profile(&id){self.connect_profile_with_secret(server,secret,cx);}return;}
        let kind=form.kind;let id=string(&form.original,"id");let new_connection=kind=="connection"&&id.is_empty();let serial=form.original["_nativeSerial"].clone();
        let mut method="POST";let path:String;let mut body=serde_json::json!({});
        match kind {
            "asset-rename"=>{path=format!("/api/assets/{id}");body=serde_json::json!({"name":form.text("name",cx)});},
            "asset-delete"=>{method="DELETE";path=format!("/api/assets/{id}");},
            "connection"=>{
                let protocol=form.selected("protocol",cx);
                let port=form.text("port",cx).parse::<u64>().unwrap_or(0);
                if protocol!="serial"&&(port==0||port>65535){self.form.as_mut().unwrap().error=Some("端口应为 1–65535".into());cx.notify();return;}
                let mut edits=serde_json::Map::new();
                for key in ["name","host","user","domain","keyPath","serialPort","notes"] {edits.insert(key.into(),Value::String(form.text(key,cx)));}
                for key in ["protocol","auth","groupId","keyId","proxyId","parity","flowControl"] {edits.insert(key.into(),Value::String(form.selected(key,cx)));}
                edits.insert("port".into(),Value::from(port));
                for key in ["baudRate","dataBits","stopBits"] {let text=if key=="baudRate"{form.text(key,cx)}else{form.selected(key,cx)};edits.insert(key.into(),Value::from(text.parse::<u64>().unwrap_or(0)));}
                for key in ["autoRun","rdpClipboard"] {edits.insert(key.into(),Value::Bool(form.flag(key)));}
                edits.insert("autoCommands".into(),Value::Array(form.text("autoCommands",cx).lines().map(|s|Value::String(s.into())).collect()));
                edits.insert("secret".into(),Value::String(if form.flag("remember"){form.text("secret",cx)}else{String::new()}));
                edits.insert("clearSecret".into(),Value::Bool(!form.flag("remember")));
                if form.selected("proxyId",cx).is_empty(){edits.insert("proxy".into(),serde_json::json!({"type":form.selected("proxyType",cx),"host":form.text("proxyHost",cx),"port":form.text("proxyPort",cx).parse::<u64>().unwrap_or(0),"user":form.text("proxyUser",cx),"password":form.text("proxyPassword",cx),"clearPassword":form.flag("clearProxyPassword")}));}
                body=merge_profile(&form.original,Value::Object(edits));body.as_object_mut().unwrap().remove("_nativeSerial");
                path="/api/profiles".into();
            },
            "group"=>{path="/api/group-nodes".into();body=serde_json::json!({"id":id,"name":form.text("name",cx),"parentId":form.selected("parentId",cx),"emoji":form.text("emoji",cx),"backgroundColor":form.text("backgroundColor",cx),"order":form.original.get("order").cloned().unwrap_or(Value::from(0))});},
            "group-keep"=>{path=format!("/api/group-nodes/{id}/delete-keep-connections");body=serde_json::json!({"targetGroupId":form.selected("targetGroupId",cx)});},
            "group-delete"=>{
                if form.text("confirmName",cx)!=string(&form.original,"name"){self.form.as_mut().unwrap().error=Some("分组名称不匹配".into());cx.notify();return;}
                let confirmations=form.original.get("_confirmations").and_then(Value::as_u64).unwrap_or(0)+1;
                if confirmations<3 {let form=self.form.as_mut().unwrap();form.original["_confirmations"]=Value::from(confirmations);form.error=Some(format!("已完成第 {confirmations} 次确认，请再次确认删除"));cx.notify();return;}
                path=format!("/api/group-nodes/{id}/deletion");body=serde_json::json!({"revision":form.original["_deletionPlan"]["revision"],"name":form.text("confirmName",cx),"confirmations":3});
            },
            "move"=>{path=format!("/api/profiles/{id}/group");body=serde_json::json!({"groupId":form.selected("targetGroupId",cx)});},
            "key"=>{path="/api/keys".into();body=serde_json::json!({"id":id,"name":form.text("name",cx),"generate":form.flag("generate"),"sourcePath":form.text("sourcePath",cx),"privateKey":form.text("privateKey",cx),"passphrase":form.text("passphrase",cx)});},
            "delete-key"=>{method="DELETE";path=format!("/api/keys/{id}");},
            "trash-profile"=>{method="DELETE";path=format!("/api/profiles/{id}");},
            "purge-profile"=>{method="DELETE";path=format!("/api/trash/{id}");},
            "purge-trash"=>{path="/api/trash/purge-all".into();},
            "backup"=>{self.export_backup(form.flag("remember"),cx);return;},
            _=>return,
        }
        let base=self.snapshot.base.clone();let token=self.token.clone();let connection_secret=form.text("secret",cx);
        self.form.as_mut().unwrap().busy=true;
        cx.spawn(async move|this,cx|{
            let result=cx.background_executor().spawn(async move{
                let saved=if method=="DELETE"{delete_json(&base,&token,&path)}else{post_json(&base,&token,&path,body)}?;
                Ok::<_,String>((saved,BackendSnapshot::load(&base,&token)))
            }).await;
            let _=this.update(cx,|state,cx|{
                let current=state.form.as_ref().is_some_and(|f|f.original["_nativeSerial"]==serial);
                match result {Ok((saved,snapshot))=>{state.snapshot=snapshot;if current {let previous=state.form.take().and_then(|f|f.return_to).map(|f|*f);state.form=previous;if let Some(f)=state.form.as_mut().filter(|_|kind=="group"){f.original["_refreshGroups"]=Value::Bool(true);f.original["groupId"]=saved["id"].clone();}}
                    if new_connection {let id=string(&saved,"id");if let Some(profile)=state.find_profile(&id){state.connect_profile_with_secret(profile,connection_secret,cx);}}
                    state.feedback=Some("已保存".into());},Err(error)=>{if current {let form=state.form.as_mut().unwrap();form.busy=false;form.error=Some(error);}else{state.feedback=Some(error);}}
                }cx.notify();
            });
        }).detach();cx.notify();
    }

    pub(super) fn config_action(&mut self,method:&'static str,path:String,body:Value,cx:&mut Context<Self>) {
        let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{if method=="DELETE"{delete_json(&base,&token,&path)?;}else{post_json(&base,&token,&path,body)?;}Ok::<_,String>(BackendSnapshot::load(&base,&token))}).await;
            let _=this.update(cx,|state,cx|{match result{Ok(snapshot)=>state.snapshot=snapshot,Err(error)=>state.feedback=Some(error)};state.ui.manager.menu=None;cx.notify();});
        }).detach();
    }

    fn export_backup(&mut self,secrets:bool,cx:&mut Context<Self>) {
        let selection=cx.prompt_for_new_path(&std::env::current_dir().unwrap_or_default(),Some("devshell-backup.json"));
        let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let Ok(Ok(Some(path)))=selection.await else{return;};let result=cx.background_executor().spawn(async move{let backup=get_json(&base,&token,if secrets{"/api/backup/export?secrets=1"}else{"/api/backup/export"})?;fs::write(path,serde_json::to_vec_pretty(&backup).map_err(|e|e.to_string())?).map_err(|e|e.to_string())}).await;
            let _=this.update(cx,|state,cx|{state.form=None;state.feedback=Some(match result{Ok(())=>"备份已导出".into(),Err(e)=>e});cx.notify();});
        }).detach();
    }
    fn import_backup(&mut self,cx:&mut Context<Self>) {
        let selection=cx.prompt_for_paths(gpui_kit::PathPromptOptions{files:true,directories:false,multiple:false,prompt:Some("选择 DevShell 备份文件".into())});let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let Ok(Ok(Some(paths)))=selection.await else{return;};let Some(path)=paths.first().cloned()else{return;};let result=cx.background_executor().spawn(async move{let bytes=fs::read(path).map_err(|e|e.to_string())?;let backup=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;post_json(&base,&token,"/api/backup/import",backup)?;Ok::<_,String>(BackendSnapshot::load(&base,&token))}).await;
            let _=this.update(cx,|state,cx|{match result{Ok(snapshot)=>{state.snapshot=snapshot;state.feedback=Some("备份已恢复".into());},Err(e)=>state.feedback=Some(e)};cx.notify();});
        }).detach();
    }
}

fn profile_matches(profile:&Value,query:&str)->bool { ["name","host","user","group","notes","serialPort"].iter().any(|key|string(profile,key).to_lowercase().contains(query)) }
fn profile_by_id(config:&Value,id:&str)->Value { ["servers","trash"].iter().filter_map(|k|config.get(k).and_then(Value::as_array)).flatten().find(|p|string(p,"id")==id).cloned().unwrap_or_default() }
pub(super) fn group_order(config:&Value)->Vec<(Value,usize)> {
    fn visit(parent:&str,groups:&[Value],depth:usize,seen:&mut BTreeSet<String>,result:&mut Vec<(Value,usize)>) {
        let mut children=groups.iter().filter(|g|string(g,"parentId")==parent).collect::<Vec<_>>();children.sort_by_key(|g|(g.get("order").and_then(Value::as_i64).filter(|v|*v!=0).unwrap_or(i64::MAX),string(g,"name")));
        for group in children {let id=string(group,"id");if seen.insert(id.clone()){result.push((group.clone(),depth));visit(&id,groups,depth+1,seen,result);}}
    }
    let groups=config.get("groupNodes").and_then(Value::as_array).cloned().unwrap_or_default();let mut result=Vec::new();let mut seen=BTreeSet::new();visit("",&groups,0,&mut seen,&mut result);
    // Imported legacy configurations may contain an orphan or a cycle. Keep
    // those groups reachable so their connections can be repaired in the UI.
    for group in &groups {let id=string(group,"id");if seen.insert(id.clone()){result.push((group.clone(),0));visit(&id,&groups,1,&mut seen,&mut result);}}
    result
}
fn group_path(groups:&[(Value,usize)],id:&str)->String {let mut parts=Vec::new();let mut id=id.to_owned();let mut seen=BTreeSet::new();while !id.is_empty()&&seen.insert(id.clone()){let Some((group,_))=groups.iter().find(|(g,_)|string(g,"id")==id)else{break;};parts.push(string(group,"name"));id=string(group,"parentId");}parts.reverse();parts.join(" / ")}

/// Cycle-safe ancestry is shared by tree visibility and the descendant filter.
pub(super) fn group_ancestors(groups:&[(Value,usize)],id:&str)->Vec<String> {
    let mut result=Vec::new();let mut cursor=id.to_owned();let mut seen=BTreeSet::new();seen.insert(cursor.clone());
    while let Some((group,_))=groups.iter().find(|(g,_)|string(g,"id")==cursor){let parent=string(group,"parentId");if parent.is_empty()||!seen.insert(parent.clone()){break;}result.push(parent.clone());cursor=parent;}
    result
}
pub(super) fn group_scope(groups:&[(Value,usize)],id:&str,descendants:bool)->BTreeSet<String> {
    std::iter::once(id.to_owned()).chain(groups.iter().filter(|(g,_)|descendants&&!id.is_empty()&&group_ancestors(groups,&string(g,"id")).contains(&id.to_owned())).map(|(g,_)|string(g,"id"))).collect()
}

fn group_choice_label(groups:&[(Value,usize)],group:&Value)->String{let path=group_path(groups,&string(group,"id"));let emoji=string(group,"emoji");if emoji.is_empty(){path}else{format!("{emoji} {path}")}}
