use super::*;
use crate::forms::string;
use crate::workspace::{Palette,icon_tool};
use gpui_kit::{Div,component::Sizable};

#[derive(Default)]
pub(super) struct RdpUi { pub parent:u64, desired:Value, sent:Value, busy:bool, ever:bool }

impl DevShellWindow {
    pub(super) fn poll_rdp(&mut self,cx:&mut Context<Self>) {
        let weak=cx.entity().downgrade();cx.spawn(async move|_,cx|{loop{
            let Some(target)=weak.update(cx,|state,_|if state.ui.rdp.ever&&!state.snapshot.locked {Some((state.snapshot.base.clone(),state.token.clone()))}else{None}).ok()else{break;};
            if let Some((base,token))=target {let result=cx.background_executor().spawn(async move{get_json(&base,&token,"/api/rdp")}).await;
                let _=weak.update(cx,|state,cx|{if let Ok(value)=result {if let Some(sessions)=value.get("sessions").and_then(Value::as_array){for info in sessions {if let Some(session)=state.session_mut(&string(info,"id")){session.rdp=Some(info.clone());if let Some(terminal)=&mut session.terminal{terminal.status=string(info,"status");}}}cx.notify();}}});
            }
            cx.background_executor().timer(Duration::from_secs(1)).await;
        }}).detach();
    }
    pub(super) fn native_parent(window:&Window)->u64 {
        #[cfg(target_os="windows")]
        {use raw_window_handle::HasWindowHandle;if let Ok(handle)=HasWindowHandle::window_handle(window){if let raw_window_handle::RawWindowHandle::Win32(win)=handle.as_raw(){return win.hwnd.get() as u64;}}}
        let _=window;0
    }
    pub(super) fn connect_rdp(&mut self,server:ServerSummary,secret:String,cx:&mut Context<Self>) {
        let base=self.snapshot.base.clone();let token=self.token.clone();let parent=self.ui.rdp.parent;let id=server.id.clone();
        self.connecting=Some(id.clone());
        cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{post_json(&base,&token,"/api/rdp/connect",serde_json::json!({"profileId":id,"secret":secret,"parent":parent}))}).await;
            let _=this.update(cx,|state,cx|{state.connecting=None;match result{Ok(info)=>{
                let id=string(&info,"id");state.park_session();if let Some(start)=state.ui.active_start.take(){state.ui.start_tabs.retain(|&s|s!=start);}
                state.workspace=SessionWorkspace{terminal:Some(TerminalState{protocol:"rdp".into(),profile_id:server.id,name:server.name,host:server.host,session_id:id.clone(),screen:Default::default(),status:string(&info,"status"),input_tx:None,view:None}),rdp:Some(info),..Default::default()};
                state.ui.session_order.push(id);state.ui.connections_visible=false;state.ui.rdp.ever=true;
            },Err(error)=>state.feedback=Some(error)}cx.notify();});
        }).detach();cx.notify();
    }
    pub(super) fn sync_rdp_viewport(&mut self,window:&Window,cx:&mut Context<Self>) {
        if !self.ui.rdp.ever{return;}
        let active=self.workspace.rdp.as_ref();
        let visible=active.is_some()&&!self.ui.connections_visible&&!self.settings_visible&&!self.ui.about_visible&&!self.ui.close_visible&&!self.ui.settings.keys_visible&&self.form.is_none()&&!self.snapshot.locked;
        let desired=serde_json::json!({"id":active.map(|r|string(r,"id")).unwrap_or_default(),"x":12.*crate::ui_scale::current(),"y":94.*crate::ui_scale::current(),"width":(f32::from(window.viewport_size().width)-24.*crate::ui_scale::current()).max(1.),"height":(f32::from(window.viewport_size().height)-106.*crate::ui_scale::current()).max(1.),"viewportWidth":f32::from(window.viewport_size().width),"visible":visible});
        self.ui.rdp.desired=desired.clone();if self.ui.rdp.busy||self.ui.rdp.sent==desired{return;}
        self.ui.rdp.busy=true;let base=self.snapshot.base.clone();let token=self.token.clone();
        cx.spawn(async move|this,cx|{let sent=desired.clone();let result=cx.background_executor().spawn(async move{post_json(&base,&token,"/api/rdp/viewport",desired)}).await;
            let _=this.update(cx,|state,cx|{state.ui.rdp.busy=false;state.ui.rdp.sent=sent;if let Err(error)=result{state.feedback=Some(error);}cx.notify();});
        }).detach();
    }
    pub(super) fn rdp_workspace(&self,p:Palette,cx:&mut Context<Self>)->Div {
        let info=self.workspace.rdp.as_ref().unwrap();let id=string(info,"id");let secure_id=id.clone();let disconnect_id=id.clone();
        div().flex_1().min_h_0().w_full().rounded(px(16.)).overflow_hidden().bg(gpui_kit::rgb(p.surface)).flex().flex_col()
            .child(div().h(px(40.)).flex_shrink_0().px_3().flex().items_center().gap_2().border_b_1().border_color(gpui_kit::rgb(p.border))
                .child(paragraph(string(info,"name"))).child(div().flex_1().text_size(px(11.)).child(string(info,"message")))
                .children([(0,0,"跟随窗口"),(1920,1080,"1920 × 1080"),(1280,800,"1280 × 800")].map(|(width,height,label)|{
                    let id=id.clone();KitButton::new(format!("rdp-size-{width}")).ghost().small().label(label).on_click(cx.listener(move|state,_,_,cx|state.rdp_action(format!("/api/rdp/{id}/resolution"),serde_json::json!({"width":width,"height":height}),cx)))
                }))
                .child(KitButton::new("rdp-attention").ghost().small().label("Ctrl + Alt + Del").on_click(cx.listener(move|state,_,_,cx|state.rdp_action(format!("/api/rdp/{secure_id}/secure-attention"),serde_json::json!({}),cx))))
                .child(icon_tool("rdp-disconnect","disconnect","断开远程桌面").on_click(cx.listener(move|_state,_,_,cx|disconnect_session(cx.entity(),disconnect_id.clone(),cx)))))
            .child(div().flex_1().flex().items_center().justify_center().child(paragraph(string(info,"message"))))
    }
    fn rdp_action(&mut self,path:String,body:Value,cx:&mut Context<Self>) {
        let base=self.snapshot.base.clone();let token=self.token.clone();cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{post_json(&base,&token,&path,body)}).await;if let Err(error)=result{let _=this.update(cx,|state,cx|{state.feedback=Some(error);cx.notify();});}}).detach();
    }
}

pub(super) fn session_root(protocol:&str)->&'static str {match protocol {"serial"=>"/api/serial/sessions/","local"=>"/api/local-shell/sessions/","tcp"=>"/api/tcp-sessions/","rdp"=>"/api/rdp/",_=>"/api/sessions/"}}
