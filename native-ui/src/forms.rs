use super::*;
use gpui_kit::{Div, SharedString};
use gpui_kit::component::Sizable;
use gpui_kit::component::{IndexPath, select::{Select, SelectEvent, SelectItem, SelectState}};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) struct Choice { pub label: SharedString, pub value: String }
impl SelectItem for Choice {
    type Value=String;
    fn title(&self)->SharedString {self.label.clone()}
    fn value(&self)->&String {&self.value}
}

pub(super) struct NativeForm {
    pub kind: &'static str,
    pub position: Option<(f32,f32)>,
    pub drag_offset: Option<(f32,f32)>,
    pub return_to: Option<Box<NativeForm>>,
    pub original: Value,
    pub inputs: BTreeMap<String,Entity<InputState>>,
    pub areas: BTreeMap<String,Entity<TextareaState>>,
    pub choices: BTreeMap<String,Entity<SelectState<Vec<Choice>>>>,
    pub flags: BTreeMap<String,bool>,
    pub colors: BTreeMap<String,Entity<gpui_kit::component::color_picker::ColorPickerState>>,
    pub error: Option<String>,
    pub busy: bool,
    pub expanded: std::collections::BTreeSet<String>,
    pub image:Option<std::sync::Arc<gpui_kit::Image>>,
}

impl NativeForm {
    pub fn new(kind:&'static str, original:Value, window:&mut Window,cx:&mut Context<DevShellWindow>)->Self {
        let mut form=Self {kind,original,return_to:None,position:None,drag_offset:None,inputs:BTreeMap::new(),areas:BTreeMap::new(),choices:BTreeMap::new(),flags:BTreeMap::new(),colors:BTreeMap::new(),error:None,busy:false,expanded:Default::default(),image:None};
        for (key,placeholder,default) in [
            ("updateMirror","留空直连 GitHub",""),("quickCommand","ssh root@192.0.2.10 -p 22",""),("mode","0755","0755"),("name","例如：Tokyo-01",""),("host","主机名或 IP",""),("port","端口","22"),("user","用户名","root"),
            ("domain","留空使用本地账号",""),("keyPath","~/.ssh/id_ed25519",""),("secret","留空则连接时输入",""),
            ("proxyHost","127.0.0.1",""),("proxyPort","7890","1080"),("proxyUser","用户名（可选）",""),("proxyPassword","留空保留已保存密码",""),
            ("serialPort","例如 COM3",""),("baudRate","115200","115200"),("emoji","选用下方图标，或输入表情符号",""),("backgroundColor","跟随主题",""),
            ("passphrase","私钥口令（可选）",""),("sourcePath","选择私钥文件",""),("confirmName","输入名称以确认",""),
            ("authValue","密码或 2FA 验证码",""),("password","留空保留现有密码",""),("repeatPassword","再次输入密码",""),("passwordHint","密码提示（可选）",""),("idleSeconds","自动锁定秒数","300"),("code","验证码",""),
        ] {
            let value=form.original.get(key).map(value_text).filter(|s|!s.is_empty()).unwrap_or(default.into());
            let masked=matches!(key,"secret"|"proxyPassword"|"passphrase"|"authValue"|"password"|"repeatPassword");
            let placeholder=if kind=="file-action"&&key=="name" {""} else {placeholder};
            form.inputs.insert(key.into(),cx.new(|cx|InputState::new(window,cx).default_value(value).placeholder(placeholder).masked(masked)));
        }
        for (key,placeholder) in [("notes","续费网址、到期时间、用途等"),("autoCommands","cd /data\ntmux attach -t dev || tmux new -s dev"),("privateKey","粘贴 PEM 或 OpenSSH 私钥")] {
            let value=match form.original.get(key) {Some(Value::Array(lines))=>lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),Some(value)=>value_text(value),None=>String::new()};
            form.areas.insert(key.into(),cx.new(|cx|TextareaState::new(window,cx).default_value(value).placeholder(placeholder).auto_grow(3,6)));
        }
        for key in ["autoRun","rdpClipboard","clearProxyPassword","generate","enabled","passwordEnabled","twoFactorEnabled"] {form.flags.insert(key.into(),form.original.get(key).and_then(Value::as_bool).unwrap_or(false));}
        form.flags.insert("remember".into(),true);
        let options=[
            ("protocol",vec![("","SSH 终端"),("rdp","RDP 远程桌面 · 测试版"),("serial","串口终端")],""),
            ("auth",vec![("password","密码"),("key","私钥"),("agent","SSH Agent")],"password"),
            ("proxyType",vec![("direct","直连"),("socks5","SOCKS5"),("http","HTTP CONNECT")],"direct"),
            ("dataBits",vec![("5","5"),("6","6"),("7","7"),("8","8")],"8"),
            ("parity",vec![("none","无"),("odd","奇"),("even","偶"),("mark","标记"),("space","空格")],"none"),
            ("stopBits",vec![("1","1"),("2","2")],"1"),
            ("flowControl",vec![("none","无"),("rtscts","RTS/CTS"),("xonxoff","XON/XOFF")],"none"),
            ("proofMethod",vec![("password","密码验证"),("totp","2FA 验证码验证")],"password"),
            ("lockMode",vec![("manual","手动锁定"),("idle","闲置后自动锁定")],"manual"),
        ];
        for (key,options,default) in options {
            let selected=form.original.get(key).map(value_text).unwrap_or(default.into());
            form.add_choices(key,options.into_iter().map(|(value,label)|(value.into(),label.into())).collect(),&selected,window,cx);
        }
        form
    }
    pub fn add_choices(&mut self,key:&str,options:Vec<(String,String)>,selected:&str,window:&mut Window,cx:&mut Context<DevShellWindow>) {
        let index=options.iter().position(|(value,_)|value==selected).unwrap_or(0);
        let items=options.into_iter().map(|(value,label)|Choice{value,label:label.into()}).collect::<Vec<_>>();
        let state=cx.new(|cx|SelectState::new(items,Some(IndexPath::new(index)),window,cx).searchable(true));
        let protocol=key=="protocol";let font_size=key=="terminalFontSize";
        cx.subscribe_in(&state,window,move|owner,_,event:&SelectEvent<Vec<Choice>>,window,cx|{
            if font_size {if let SelectEvent::Confirm(Some(value))=event {owner.ui.font_size=value.parse::<f32>().unwrap_or(14.).clamp(8.,40.);owner.save_appearance(serde_json::json!({"terminalFontSize":owner.ui.font_size}),cx);}}
            if protocol {if let SelectEvent::Confirm(Some(value))=event {if let Some(form)=&owner.form {let port=form.text("port",cx);if matches!(port.as_str(),"22"|"3389"){form.inputs["port"].update(cx,|input,cx|input.set_value(if value=="rdp" {"3389"}else{"22"},window,cx));}}}}
            cx.notify();
        }).detach();
        self.choices.insert(key.into(),state);
    }
    pub fn text(&self,key:&str,cx:&App)->String {self.inputs.get(key).map(|i|i.read(cx).value().to_string()).or_else(||self.areas.get(key).map(|i|i.read(cx).value().to_string())).unwrap_or_default()}
    pub fn selected(&self,key:&str,cx:&App)->String {self.choices.get(key).and_then(|i|i.read(cx).selected_value().cloned()).unwrap_or_default()}
    pub fn flag(&self,key:&str)->bool {self.flags.get(key).copied().unwrap_or(false)}
    pub fn field(&self,key:&str,label:&str)->Div {
        div().flex().flex_col().gap(px(5.)).min_w_0().text_size(px(12.)).line_height(px(16.8)).text_color(gpui_kit::rgb(if crate::ui_scale::dark(){0xe5e5e5}else{0x344f62})).child(label.to_owned())
            .child(gpui_kit::Styled::h(Input::new(&self.inputs[key]),px(36.)).px(px(10.)).py(px(7.)).text_size(px(12.)).line_height(px(20.)).bg(gpui_kit::rgb(if crate::ui_scale::dark(){0x131a21}else{0xf9fbfc})).rounded(px(8.)).border_color(gpui_kit::rgb(if crate::ui_scale::dark(){0x2d3944}else{0xe2eaf0})))
    }
    pub fn area(&self,key:&str,label:&str)->Div {div().flex().flex_col().gap(px(5.)).text_size(px(12.)).line_height(px(16.8)).child(label.to_owned()).child(Textarea::new(&self.areas[key]).h(px(if key=="autoCommands"{96.}else{72.})).px(px(10.)).py(px(8.)).bg(gpui_kit::rgb(if crate::ui_scale::dark(){0x131a21}else{0xf9fbfc})).rounded(px(9.)).border_color(gpui_kit::rgb(if crate::ui_scale::dark(){0x2d3944}else{0xe2eaf0})).text_size(px(12.)).line_height(px(18.)))}
    pub fn select(&self,key:&str,label:&str)->Div {div().flex().flex_col().gap(px(5.)).min_w_0().text_size(px(12.)).line_height(px(16.8)).child(label.to_owned()).child(Select::new(&self.choices[key]).h(px(36.)).appearance(false).bg(gpui_kit::rgb(if crate::ui_scale::dark(){0x131a21}else{0xf1f5f8})).rounded(px(8.)).px(px(10.)).text_size(px(12.)).line_height(px(20.)).accessibility_label(label.to_owned()))}
    pub fn toggle(&self,key:&'static str,label:&str,cx:&mut Context<DevShellWindow>)->Div {
        use gpui_kit::component::checkbox::Checkbox;
        div().min_h(px(24.)).flex().items_center().child(Checkbox::new(format!("form-flag-{key}")).small().accessibility_label(label.to_owned()).text_size(px(11.)).line_height(px(14.)).child(div().text_size(px(11.)).line_height(px(15.4)).child(label.to_owned())).checked(self.flag(key)).on_click(cx.listener(move|state,value:&bool,_,cx|{
            if let Some(form)=state.form.as_mut(){form.flags.insert(key.into(),*value);}cx.notify();
        })))
    }
    pub fn disclosure(&self,key:&'static str,label:&str,summary:&str,cx:&mut Context<DevShellWindow>)->KitButton {
        KitButton::new(format!("form-disclosure-{key}")).ghost().w_full().h(px(42.)).px(px(10.)).rounded(px(9.)).border_1().border_color(gpui_kit::rgb(if crate::ui_scale::dark(){0x2d3944}else{0xe7edf2})).child(div().w_full().flex().items_center().gap(px(8.)).child(div().text_size(px(12.)).line_height(px(20.)).child(label.to_owned())).child(div().flex_1()).child(div().text_size(px(11.)).line_height(px(20.)).child(summary.to_owned())).child(crate::workspace::icon("chevron",12.).when(self.expanded.contains(key),|svg|svg.with_transformation(gpui_kit::Transformation::rotate(gpui_kit::radians(std::f32::consts::FRAC_PI_2)))))).on_click(cx.listener(move|state,_,_,cx|{if let Some(form)=state.form.as_mut(){if !form.expanded.remove(key){form.expanded.insert(key.into());}}cx.notify();}))
    }
    pub fn error_panel(&self)->Div {div().when_some(self.error.clone(),|body,error|body.p_2().rounded_md().bg(gpui_kit::rgba(0xc64f4f18)).text_color(gpui_kit::rgb(0xc46c68)).text_size(px(12.)).child(error))}
}

pub(super) fn value_text(value:&Value)->String {match value{Value::String(s)=>s.clone(),Value::Null=>String::new(),_=>value.to_string()}}
pub(super) fn string(value:&Value,key:&str)->String {value.get(key).map(value_text).unwrap_or_default()}

/// Merge edits over the public original, retaining fields this editor does not
/// own (forward rules, protocol options, credentials and future schema fields).
pub(super) fn merge_profile(original:&Value, edits:Value)->Value {
    let mut profile=original.as_object().cloned().unwrap_or_default();
    if let Some(edits)=edits.as_object(){for(key,value)in edits {profile.insert(key.clone(),value.clone());}}
    for key in ["deletedAt","hasSecret","temporary","needsProxy","verbose","proxyType","proxyHost","proxyPort","proxyUser"] {profile.remove(key);}
    profile.retain(|key,_|!key.starts_with("_native"));
    Value::Object(profile)
}
