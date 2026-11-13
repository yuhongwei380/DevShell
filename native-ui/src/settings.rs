use super::*;
use crate::forms::string;
use crate::workspace::{Palette,icon,icon_tool,overlay};
use gpui_kit::{Div,Focusable,component::Sizable};

pub(super) struct SettingsUi {
    pub unlock:Entity<InputState>,
    pub unlock_method:&'static str,
    pub error:Option<String>,
    pub busy:bool,
    pub last_activity:std::time::Instant,
    pub pending_activity:bool,
    pub clear_unlock:bool,
    pub keys_visible:bool,
    pub preferences:Option<forms::NativeForm>,
}
impl SettingsUi {
    pub fn new(window:&mut Window,cx:&mut Context<DevShellWindow>)->Self {
        Self{unlock:cx.new(|cx|InputState::new(window,cx).placeholder("密码或验证码").masked(true)),unlock_method:"password",error:None,busy:false,last_activity:std::time::Instant::now(),pending_activity:false,clear_unlock:false,keys_visible:false,preferences:None}
    }
}

impl DevShellWindow {
    pub(super) fn ensure_preferences(&mut self,window:&mut Window,cx:&mut Context<Self>) {
        if self.ui.settings.preferences.is_some(){return;}
        use gpui_kit::component::select::SelectEvent;
        let mut form=forms::NativeForm::new("preferences",self.snapshot.appearance.clone(),window,cx);
        for (key,options,selected) in [
            ("concise",vec![("true","开启"),("false","关闭")],self.ui.concise.to_string()),
            ("monitorVisible",vec![("true","显示"),("false","隐藏")],self.ui.monitor_visible.to_string()),
            ("filesPosition",vec![("bottom","终端下方"),("right","终端右侧"),("top","终端上方")],string(&self.snapshot.appearance,"filesPosition")),
            ("followTerminal",vec![("true","开启"),("false","关闭")],self.snapshot.appearance["followTerminal"].as_bool().unwrap_or(true).to_string()),
            ("uiScale",vec![("0.75","75%"),("0.9","90%"),("1","100%"),("1.1","110%"),("1.25","125%"),("1.5","150%"),("1.75","175%"),("2","200%")],self.snapshot.appearance["uiScale"].as_f64().unwrap_or(1.).to_string()),
        ] {
            form.add_choices(key,options.into_iter().map(|(a,b)|(a.into(),b.into())).collect(),&selected,window,cx);
            cx.subscribe_in(&form.choices[key],window,move|state,_,event:&SelectEvent<Vec<forms::Choice>>,_,cx|{if let SelectEvent::Confirm(Some(value))=event{
                let mut patch=serde_json::json!({});
                match key {
                    "concise"=>{state.ui.concise=value=="true";state.ui.command_visible=!state.ui.concise;state.ui.monitor_visible=!state.ui.concise;let mut layout=state.snapshot.appearance["layout"].as_object().cloned().unwrap_or_default();layout.insert("conciseMode".into(),state.ui.concise.into());patch=serde_json::json!({"layout":layout,"monitorVisible":state.ui.monitor_visible});},
                    "monitorVisible"=>{state.ui.monitor_visible=value=="true";patch[key]=Value::Bool(value=="true");},
                    "filesPosition"=>{state.ui.files_position=workspace::FilesPosition::from_value(value);patch[key]=value.clone().into();},
                    "uiScale"=>{let value=value.parse::<f64>().unwrap_or(1.);state.snapshot.appearance[key]=value.into();patch[key]=value.into();},
                    _=>patch[key]=Value::Bool(value=="true"),
                }state.save_appearance(patch,cx);cx.notify();
            }}).detach();
        }
        self.ui.settings.preferences=Some(form);
    }
    pub(super) fn preference_row(&self,key:&str,label:&str)->Div {
        use gpui_kit::component::select::Select;
        div().mx(px(10.)).mt(px(4.)).h(px(27.)).flex().items_center().justify_between().gap(px(15.)).text_size(px(12.)).line_height(px(16.8)).child(label.to_owned()).when_some(self.ui.settings.preferences.as_ref(),|body,form|body.child(div().w(px(if key=="uiScale"{78.}else{112.})).h(px(27.)).flex_shrink_0().child(Select::new(&form.choices[key]).w(px(if key=="uiScale"{78.}else{112.})).h(px(27.)).appearance(false).rounded(px(8.)).bg(gpui_kit::rgb(if self.ui.dark{0x131a21}else{0xf1f5f8})).px(px(9.)).text_size(px(12.)).line_height(px(17.)).accessibility_label(label.to_owned()))))
    }
    pub(super) fn note_activity(&mut self) {if self.ui.settings.last_activity.elapsed()>=Duration::from_millis(500){self.ui.settings.pending_activity=true;self.ui.settings.last_activity=std::time::Instant::now();}}
    pub(super) fn poll_security(&mut self,cx:&mut Context<Self>) {
        let weak=cx.entity().downgrade();
        cx.spawn(async move|_,cx|{loop{
            let Some((base,token,activity))=weak.update(cx,|state,_|{let activity=std::mem::take(&mut state.ui.settings.pending_activity);(state.snapshot.base.clone(),state.token.clone(),activity&&state.snapshot.security.get("enabled").and_then(Value::as_bool).unwrap_or(false)&&!state.snapshot.locked)}).ok()else{break;};
            let result=cx.background_executor().spawn(async move{if activity {post_json(&base,&token,"/api/security-lock/activity",serde_json::json!({}))}else{get_json(&base,&token,"/api/security-lock/status")}}).await;
            if weak.update(cx,|state,cx|{if let Ok(status)=result {
                let locked=status.get("locked").and_then(Value::as_bool).unwrap_or(false);
                if locked&&!state.snapshot.locked {state.form=None;state.settings_visible=false;state.ui.connections_visible=false;state.ui.settings.keys_visible=false;}
                if locked!=state.snapshot.locked||status!=state.snapshot.security {state.snapshot.locked=locked;state.snapshot.security=status;cx.notify();}
            }}).is_err(){break;}
            cx.background_executor().timer(Duration::from_secs(1)).await;
        }}).detach();
    }

    pub(super) fn open_security_settings(&mut self,window:&mut Window,cx:&mut Context<Self>) {
        let mut draft=self.snapshot.security.clone();
        draft["passwordEnabled"]=draft["passwordConfigured"].clone();draft["twoFactorEnabled"]=draft["twoFactorConfigured"].clone();
        draft["lockMode"]=Value::String(if draft["idleSeconds"].as_u64().unwrap_or(0)>0 {"idle"}else{"manual"}.into());
        self.open_native_form("security",draft,window,cx);self.settings_visible=false;
        if !self.snapshot.security["enabled"].as_bool().unwrap_or(false){self.security_call("authorize",serde_json::json!({}),cx);}
    }

    pub(super) fn security_fields(&self,cx:&mut Context<Self>)->Div {
        let form=self.form.as_ref().unwrap();let authorized=form.original.get("_grant").and_then(Value::as_str).is_some_and(|s|!s.is_empty());
        div().flex().flex_col().gap_3()
            .when(!authorized,|body|body.child(paragraph("修改安全设置前，请先验证身份。" )).child(form.select("proofMethod","验证方式")).child(form.field("authValue","密码 / 2FA 验证码"))
                .child(KitButton::new("security-authorize").primary().small().label("验证后修改").disabled(form.busy).on_click(cx.listener(|state,_,_,cx|{if let Some(form)=&state.form{let body=serde_json::json!({"method":form.selected("proofMethod",cx),"value":form.text("authValue",cx)});state.security_call("authorize",body,cx);}}))))
            .when(authorized,|body|body.child(form.toggle("enabled","启用安全锁定",cx))
                .child(form.select("lockMode","锁定方式"))
                .when(form.selected("lockMode",cx)=="idle",|body|body.child(form.field("idleSeconds","闲置多少秒后锁定")))
                .child(form.toggle("passwordEnabled","使用密码解锁",cx))
                .when(form.flag("passwordEnabled"),|body|body.child(form.field("password","设置密码（留空保留现有密码）")).child(form.field("repeatPassword","确认密码")).child(form.field("passwordHint","密码提示")))
                .child(form.toggle("twoFactorEnabled","使用 2FA 验证码解锁",cx))
                .when(form.flag("twoFactorEnabled"),|body|body.child(KitButton::new("security-enroll").secondary().small().label(if form.original["twoFactorConfigured"].as_bool().unwrap_or(false){"重新绑定验证器"}else{"扫码绑定验证器"}).disabled(form.busy).on_click(cx.listener(|state,_,_,cx|{if let Some(form)=&state.form{state.security_call("enroll",serde_json::json!({"grant":form.original["_grant"]}),cx);}}))))
                .when(form.original.get("_enrollment").is_some(),|body|{
                    body.child(paragraph("在验证器中添加下方密钥，并输入当前验证码，保存后生效。"))
                        .when_some(form.image.clone(),|body,image|body.child(gpui_kit::img(image).size(px(220.))))
                        .child(div().text_size(px(13.)).child(string(&form.original["_enrollment"],"secret")))
                        .child(form.field("code","当前验证码"))
                }))
    }

    pub(super) fn save_security_settings(&mut self,cx:&mut Context<Self>) {
        let Some(form)=self.form.as_ref()else{return;};
        if form.text("password",cx)!=form.text("repeatPassword",cx){self.form.as_mut().unwrap().error=Some("两次输入的密码不一致".into());cx.notify();return;}
        let body=serde_json::json!({"grant":form.original["_grant"],"enabled":form.flag("enabled"),"idleSeconds":if form.selected("lockMode",cx)=="manual"{0}else{form.text("idleSeconds",cx).parse::<u64>().unwrap_or(0)},"passwordEnabled":form.flag("passwordEnabled"),"password":form.text("password",cx),"passwordHint":form.text("passwordHint",cx),"twoFactorEnabled":form.flag("twoFactorEnabled"),"code":form.text("code",cx)});
        self.security_call("settings",body,cx);
    }

    pub(super) fn security_call(&mut self,action:&'static str,body:Value,cx:&mut Context<Self>) {
        let serial=self.form.as_ref().map(|f|f.original["_nativeSerial"].clone());let base=self.snapshot.base.clone();let token=self.token.clone();
        if let Some(form)=self.form.as_mut(){form.busy=true;form.error=None;}self.ui.settings.busy=true;self.ui.settings.error=None;
        cx.spawn(async move|this,cx|{let result=cx.background_executor().spawn(async move{
            let response=post_json(&base,&token,&format!("/api/security-lock/{action}"),body)?;
            let snapshot=if matches!(action,"unlock"|"settings"){Some(BackendSnapshot::load(&base,&token))}else{None};
            Ok::<_,String>((response,snapshot))
        }).await;
            let _=this.update(cx,|state,cx|{
                state.ui.settings.busy=false;
                match result {
                    Ok((response,snapshot))=>{
                        if let Some(snapshot)=snapshot{state.snapshot=snapshot;}
                        match action {"authorize"|"enroll"=>{
                            if let Some(form)=state.form.as_mut().filter(|f|Some(f.original["_nativeSerial"].clone())==serial){form.busy=false;if action=="authorize"{form.original["_grant"]=response["grant"].clone();}else{form.image=response["qr"].as_str().and_then(|s|s.strip_prefix("data:image/png;base64,")).and_then(|s|base64::engine::general_purpose::STANDARD.decode(s).ok()).map(|bytes|std::sync::Arc::new(gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png,bytes)));form.original["_enrollment"]=response;}}
                        },"settings"=>{state.form=None;state.feedback=Some("安全锁定设置已保存".into());},"unlock"=>{state.ui.settings.clear_unlock=true;state.snapshot.locked=false;},"lock"=>{state.snapshot.locked=true;state.snapshot.security=response;},_=>{}}
                    },Err(error)=>{state.ui.settings.error=Some(error.clone());if let Some(form)=state.form.as_mut().filter(|f|Some(f.original["_nativeSerial"].clone())==serial){form.busy=false;form.error=Some(error);}}
                }cx.notify();
            });
        }).detach();cx.notify();
    }

    pub(super) fn lock_screen(&mut self,p:Palette,window:&mut Window,cx:&mut Context<Self>)->Div {
        let handle=self.ui.settings.unlock.read(cx).focus_handle(cx);
        if !handle.is_focused(window){window.focus(&handle,cx);}
        let password=self.snapshot.security["passwordConfigured"].as_bool().unwrap_or(false);let totp=self.snapshot.security["twoFactorConfigured"].as_bool().unwrap_or(false);
        if !password&&totp {self.ui.settings.unlock_method="totp";}
        div().absolute().inset_0().bg(gpui_kit::rgb(p.background)).occlude().flex().items_center().justify_center()
            .child(div().w(px(370.)).p_6().rounded(px(18.)).bg(gpui_kit::rgb(p.surface)).border_1().border_color(gpui_kit::rgb(p.border)).flex().flex_col().gap_4()
                .child(div().flex().items_center().gap_3().child(icon("lock",24.)).child(heading("DevShell 已锁定")))
                .child(paragraph("解锁后继续使用工作台。"))
                .when(password&&totp,|body|body.child(div().flex().gap_2().children([("password","密码解锁"),("totp","2FA 验证码解锁")].map(|(method,label)|KitButton::new(format!("unlock-{method}")).ghost().small().label(label).when(self.ui.settings.unlock_method==method,|b|b.secondary()).on_click(cx.listener(move|state,_,_,cx|{state.ui.settings.unlock_method=method;cx.notify();}))))))
                .child(Input::new(&self.ui.settings.unlock).mask_toggle())
                .when(!string(&self.snapshot.security,"passwordHint").is_empty(),|body|body.child(paragraph(format!("密码提示：{}",string(&self.snapshot.security,"passwordHint")))))
                .when_some(self.ui.settings.error.clone(),|body,error|body.child(div().text_color(gpui_kit::rgb(0xc46c68)).child(error)))
                .child(KitButton::new("unlock-confirm").primary().label(if self.ui.settings.busy{"正在验证…"}else{"解锁"}).disabled(self.ui.settings.busy).on_click(cx.listener(|state,_,_,cx|{
                    state.security_call("unlock",serde_json::json!({"method":state.ui.settings.unlock_method,"value":state.ui.settings.unlock.read(cx).value().to_string()}),cx);
                }))))
    }

    pub(super) fn keys_manager(&self,p:Palette,cx:&mut Context<Self>)->Div {
        overlay(p,"密钥管理器",icon_tool("keys-close","close","关闭密钥管理器").on_click(cx.listener(|state,_,_,cx|{state.ui.settings.keys_visible=false;cx.notify();})))
            .child(div().id("keys-scroll").max_h(px(500.)).overflow_y_scroll().p_5().flex().flex_col().gap_3()
                .child(paragraph("导入私钥，或生成 Ed25519 密钥；公钥可复制到服务器。"))
                .child(KitButton::new("key-import").primary().small().label("导入 / 生成密钥").on_click(cx.listener(|state,_,window,cx|state.open_native_form("key",serde_json::json!({}),window,cx))))
                .children(self.snapshot.keys.iter().map(|key|{
                    let id=string(key,"id");let edit=key.clone();let delete=key.clone();let public=string(key,"publicKey");
                    div().p_3().rounded_lg().border_1().border_color(gpui_kit::rgb(p.border)).flex().flex_col().gap_2().child(paragraph(string(key,"name"))).child(paragraph(string(key,"fingerprint")))
                        .child(div().flex().gap_2()
                            .child(KitButton::new(format!("key-public-{id}")).ghost().small().label("复制公钥").on_click(move|_,_,cx|cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(public.clone()))))
                            .child(KitButton::new(format!("key-edit-{id}")).ghost().small().label("编辑").on_click(cx.listener(move|state,_,window,cx|state.open_native_form("key",edit.clone(),window,cx))))
                            .child(KitButton::new(format!("key-delete-{id}")).ghost().small().label("删除").on_click(cx.listener(move|state,_,window,cx|state.open_native_form("delete-key",delete.clone(),window,cx)))))
                })))
    }
}
