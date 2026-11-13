use super::*;
use crate::forms::string;
use crate::workspace::{Palette,icon};
use gpui_kit::{Div,component::Sizable};

pub(super) struct TabDrag(pub String,pub String);
impl Clone for TabDrag {fn clone(&self)->Self{Self(self.0.clone(),self.1.clone())}}
impl Render for TabDrag {fn render(&mut self,_:&mut Window,_:&mut Context<Self>)->impl IntoElement{div().px_3().py_2().rounded_lg().bg(gpui_kit::rgb(0x213948)).text_color(gpui_kit::rgb(0xd3e0e8)).child(self.1.clone())}}

impl DevShellWindow {
    pub(super) fn tabs_list(&self,p:Palette,cx:&mut Context<Self>)->Div {
        use gpui_kit::component::Sizable;
        let mut tabs=self.parked.iter().filter_map(|session|session.terminal.as_ref()).collect::<Vec<_>>();if let Some(terminal)=self.workspace.terminal.as_ref(){tabs.push(terminal);}
        tabs.sort_by_key(|terminal|self.ui.session_order.iter().position(|id|id==&terminal.session_id));
        div().absolute().top(px(48.)).right(px(132.)).w(px(300.)).p(px(6.)).rounded(px(12.)).bg(gpui_kit::rgb(p.surface)).shadow_lg().occlude().flex().flex_col().gap_1()
            .children(self.ui.start_tabs.iter().map(|&index|KitButton::new(format!("tab-list-start-{index}")).ghost().small().justify_start().label(format!("新选项卡 {index}")).on_click(cx.listener(move|state,_,_,cx|{state.activate_start_tab(index,cx);state.ui.tab_list_visible=false;cx.notify();}))))
            .children(tabs.iter().map(|terminal|{let id=terminal.session_id.clone();KitButton::new(format!("tab-list-session-{id}")).ghost().small().justify_start().label(terminal.name.clone()).on_click(cx.listener(move|state,_,_,cx|{state.switch_session(&id,cx);state.ui.tab_list_visible=false;cx.notify();}))}))
    }
    pub(super) fn quick_connect_command(&mut self,command:String,auth:String,key:String,secret:String,cx:&mut Context<Self>){
        let base=self.snapshot.base.clone();let token=self.token.clone();let serial=self.form.as_ref().map(|f|f.original["_nativeSerial"].clone());
        if let Some(f)=self.form.as_mut(){f.busy=true;}
        cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{post_json(&base,&token,"/api/quick-connect",serde_json::json!({"command":command,"auth":auth,"keyId":key}))}).await;
            let _=this.update(cx,|state,cx|{match result{Ok(mut profile)=>{profile["temporary"]=true.into();if let Some(server)=ServerSummary::from_value(&profile){state.temporary_profiles.push(server.clone());state.form=None;if secret.is_empty(){state.connect_profile(server,cx);}else{state.connect_profile_with_secret(server,secret,cx);}}},Err(error)=>{if let Some(f)=state.form.as_mut().filter(|f|Some(f.original["_nativeSerial"].clone())==serial){f.busy=false;f.error=Some(error);}else{state.feedback=Some(error);}}}cx.notify();});
        }).detach();
    }
    pub(super) fn start_auxiliary(&mut self,protocol:&'static str,body:Value,cx:&mut Context<Self>){
        let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{post_json(&base,&token,if protocol=="local"{"/api/local-shell/sessions"}else{"/api/tcp-sessions"},body)}).await;
            let _=this.update(cx,|state,cx|{match result{Ok(mut info)=>{info["protocol"]=protocol.into();if let Some(server)=ServerSummary::from_value(&info){let id=string(&info,"id");state.temporary_profiles.push(server.clone());state.attach_terminal_session(server,id,protocol.into(),vec![],cx);}},Err(error)=>state.feedback=Some(error)}cx.notify();});
        }).detach();
    }
    fn clone_channel(&mut self,id:String,cx:&mut Context<Self>){
        let Some(terminal)=self.session_mut(&id).and_then(|s|s.terminal.clone()).filter(|t|t.protocol=="ssh"&&t.input_tx.is_some())else{return;};
        let Some(server)=self.find_profile(&terminal.profile_id)else{return;};let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{post_json(&base,&token,&format!("/api/sessions/{id}/channel"),serde_json::json!({}))}).await;
            let _=this.update(cx,|state,cx|{match result{Ok(info)=>state.attach_terminal_session(server,string(&info,"id"),"ssh".into(),vec![],cx),Err(error)=>state.feedback=Some(error)}cx.notify();});
        }).detach();
    }
    pub(super) fn toggle_pin(&mut self,profile:&str,cx:&mut Context<Self>){
        let mut pins=self.snapshot.appearance["layout"]["dengshell.pinned.v1"].as_array().cloned().unwrap_or_default();
        if pins.iter().any(|p|p.as_str()==Some(profile)){pins.retain(|p|p.as_str()!=Some(profile));}else{pins.push(profile.into());}
        self.snapshot.appearance["layout"]["dengshell.pinned.v1"]=pins.clone().into();self.save_appearance(serde_json::json!({"layout":{"dengshell.pinned.v1":pins}}),cx);cx.notify();
    }
    pub(super) fn pinned_menu(&self,p:Palette,cx:&mut Context<Self>)->Div {
        let pins=self.snapshot.appearance["layout"]["dengshell.pinned.v1"].as_array().cloned().unwrap_or_default();
        div().absolute().top(px(48.)).left(px(if self.ui.monitor_visible{360.}else{232.})).w(px(300.)).rounded(px(14.)).p_2().bg(gpui_kit::rgb(p.surface)).border_1().border_color(gpui_kit::rgb(p.border)).shadow_lg().occlude().flex().flex_col().gap_1()
            .child(div().p_2().text_size(px(11.)).text_color(gpui_kit::rgb(p.muted)).child("收藏夹"))
            .when(pins.is_empty(),|body|body.child(div().p_3().text_size(px(12.)).child("右键会话标签，选择“加入收藏夹”。")))
            .children(pins.iter().filter_map(|id|self.find_profile(id.as_str()?)).map(|server|{let open=server.clone();let id=server.id.clone();div().flex().items_center().gap_1()
                .child(KitButton::new(format!("pinned-open-{id}")).ghost().flex_1().justify_start().child(icon("server",16.)).label(server.name).on_click(cx.listener(move|state,_,_,cx|{state.ui.pinned_visible=false;state.connect_profile(open.clone(),cx);})))
                .child(KitButton::new(format!("pinned-remove-{id}")).ghost().small().label("×").accessibility_label("取消收藏").on_click(cx.listener(move|state,_,_,cx|state.toggle_pin(&id,cx))))}))
    }
    pub(super) fn tab_context_menu(&self,p:Palette,window:&Window,cx:&mut Context<Self>)->Div{
        let (id,point)=self.ui.tab_menu.as_ref().unwrap();let mut sessions=self.parked.iter().collect::<Vec<_>>();sessions.push(&self.workspace);
        let terminal=sessions.iter().filter_map(|s|s.terminal.as_ref()).find(|t|t.session_id==*id);
        let title=terminal.map(|t|t.name.clone()).unwrap_or_default();
        let actions=[("rename-session","重命名标签"),("reconnect","重新连接"),("clone","复制同服务器 SSH 会话"),("pin","加入收藏夹 / 取消收藏"),("disconnect","断开连接"),("close","关闭标签")];
        div().absolute().inset_0().child(div().absolute().inset_0().occlude().on_mouse_down(gpui_kit::MouseButton::Left,cx.listener(|s,_,_,cx|{s.ui.tab_menu=None;cx.notify();})))
            .child(div().absolute().left(point.x.min(window.viewport_size().width-px(248.))).top(point.y.min(window.viewport_size().height-px(275.))).w(px(240.)).p_2().rounded(px(12.)).bg(gpui_kit::rgb(p.surface)).border_1().border_color(gpui_kit::rgb(p.border)).shadow_lg().occlude().flex().flex_col().gap_1()
                .children(actions.map(|(action,label)|{let id=id.clone();let title=title.clone();KitButton::new(format!("tab-action-{action}")).ghost().small().justify_start().label(label).on_click(cx.listener(move|state,_,window,cx|{
                    state.ui.tab_menu=None;
                    let Some(t)=state.session_mut(&id).and_then(|s|s.terminal.clone())else{return;};
                    match action {
                        "rename-session"=>state.open_native_form("rename-session",serde_json::json!({"id":id,"name":title}),window,cx),
                        "clone"=>state.clone_channel(id.clone(),cx),
                        "pin"=>state.toggle_pin(&t.profile_id,cx),
                        "reconnect"=>{if t.protocol=="tcp"{if let Some(p)=state.find_profile(&t.profile_id){state.start_auxiliary("tcp",serde_json::json!({"host":p.host,"port":p.port}),cx);}}else if t.protocol=="local"{state.start_auxiliary("local",serde_json::json!({}),cx);}else if let Some(server)=state.find_profile(&t.profile_id){state.connect_profile(server,cx);}},
                        "close"=>{state.ui.closing_sessions.push(id.clone());disconnect_session(cx.entity(),id.clone(),cx);},
                        _=>disconnect_session(cx.entity(),id.clone(),cx),
                    }cx.notify();
                }))})))
    }
}

pub(super) fn tcp_target(command:&str)->Option<(String,u16)>{
    let parts=command.split_whitespace().collect::<Vec<_>>();if parts.len()<2||parts.len()>3||!parts[0].eq_ignore_ascii_case("telnet"){return None;}
    let port=if parts.len()==3{parts[2].parse::<u16>().ok()?}else{23};if port==0{return None;}Some((parts[1].into(),port))
}

impl DevShellWindow {
    pub(super) fn accept_terminal_event(&mut self,id:&str,event:TerminalEvent,cx:&mut Context<Self>)->bool {
        let mut directory=None;let mut commands=Vec::new();let mut closed=false;
        let follow=self.snapshot.appearance["followTerminal"].as_bool().unwrap_or(true);
        if let Some(session)=self.session_mut(id){
            match event {
                TerminalEvent::Ready(metadata)=>{session.integration.accept(&metadata);session.terminal_ready=true;session.auto_next=Some(std::time::Instant::now()+Duration::from_millis(2500));if let Some(t)=session.terminal.as_mut(){t.status="远端 Shell 已就绪".into();}},
                TerminalEvent::Output(bytes)=>{
                    for event in session.integration.feed(&bytes){match event{
                        integration::ShellEvent::Directory(path)=>{if follow&&session.files.as_ref().is_none_or(|f|f.path!=path){directory=Some(path);}},
                        integration::ShellEvent::Command(command)=>commands.push(command),
                        integration::ShellEvent::Prompt=>session.auto_next=Some(std::time::Instant::now()),
                    }}
                    if let Some(t)=session.terminal.as_mut(){if let Ok(mut screen)=t.screen.lock(){screen.feed(&bytes);}if let Some(view)=&t.view{view.update(cx,|terminal,cx|terminal.feed(&bytes,cx));}if let Some(sender)=&t.input_tx{let _=sender.send(TerminalInput::Ack);}}
                },
                TerminalEvent::Info(message)=>{if let Some(t)=session.terminal.as_mut(){t.status=message;}},
                TerminalEvent::Closed(message)=>{session.auto_commands.clear();session.auto_next=None;session.terminal_ready=false;if let Some(t)=session.terminal.as_mut(){t.input_tx=None;t.status=message;}closed=true;},
            }
        }
        if let Some(path)=directory{navigate_files_for(cx.entity(),id.into(),path,cx);}
        for command in commands{self.record_shell_command(id,command,cx);}
        self.advance_auto_commands(id,cx);closed
    }
    pub(super) fn advance_auto_commands(&mut self,id:&str,_cx:&mut Context<Self>){
        if self.snapshot.locked||!self.snapshot.appearance["autoCommandsEnabled"].as_bool().unwrap_or(true){return;}
        let Some(session)=self.session_mut(id)else{return;};
        if !session.terminal_ready||session.auto_commands.is_empty()||session.auto_next.is_none_or(|next|std::time::Instant::now()<next){return;}
        if session.integration.ready&&!session.integration.at_prompt{return;}
        let Some(sender)=session.terminal.as_ref().and_then(|t|t.input_tx.as_ref())else{return;};
        let command=session.auto_commands.remove(0);let _=sender.send(TerminalInput::Text(format!("{}\r",command.trim_end_matches(['\r','\n']))));
        session.integration.at_prompt=false;session.auto_next=Some(std::time::Instant::now()+Duration::from_millis(if session.integration.ready{15000}else{1500}));
    }
    fn record_shell_command(&mut self,id:&str,command:String,cx:&mut Context<Self>){
        if command.trim().is_empty()||command.len()>16384||command.contains('\0'){return;}
        let Some(session)=self.session_mut(id)else{return;};let Some(terminal)=&session.terminal else{return;};if terminal.protocol!="ssh"{return;}
        let profile=terminal.profile_id.clone();
        if session.command_history.last()!=Some(&command){session.command_history.push(command.clone());if session.command_history.len()>200{session.command_history.remove(0);}}
        let base=self.snapshot.base.clone();let token=self.token.clone();
        let operation=format!("native_{:032x}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{post_json(&base,&token,&format!("/api/profiles/{profile}/history"),serde_json::json!({"operation":operation,"command":command}))}).await;if let Err(error)=result{let _=this.update(cx,|state,cx|{state.feedback=Some(format!("保存命令历史失败：{error}"));cx.notify();});}}).detach();
    }
}
