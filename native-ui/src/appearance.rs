use super::*;
use crate::forms::string;
use gpui_kit::Div;
use ely_gpui_component::terminal::RemoteAppearance;

pub(super) const ANSI_KEYS:[&str;16]=["black","red","green","yellow","blue","magenta","cyan","white","brightBlack","brightRed","brightGreen","brightYellow","brightBlue","brightMagenta","brightCyan","brightWhite"];
pub(super) const FONTS:[(&str,&str,&str);5]=[
    ("builtin:jetbrains-mono","JetBrains Mono","Deng JetBrains Mono"),
    ("builtin:fira-code","Fira Code","Fira Code"),
    ("builtin:ibm-plex-mono","IBM Plex Mono","Deng IBM Plex Mono"),
    ("builtin:source-code-pro","Source Code Pro","Deng Source Code Pro"),
    ("builtin:maple-mono-cn","Maple Mono CN","Deng UI Maple"),
];
pub(super) fn color(value:&Value)->Option<u32> {
    let text=value.as_str()?.strip_prefix('#')?;
    if text.len()!=6{return None;}u32::from_str_radix(text,16).ok()
}
pub(super) fn terminal_appearance(settings:&Value,size:f32,_background:u32)->RemoteAppearance {
    let id=settings["fontId"].as_str().unwrap_or(FONTS[0].0);
    let family=FONTS.iter().find(|f|f.0==id).unwrap_or(&FONTS[0]).2;
    let palettes:Value=serde_json::from_str(include_str!("../assets/ansi-palettes.json")).unwrap();
    let selected=settings["ansiPalette"].as_str().unwrap_or("soft");
    let scheme=settings["ansiSchemes"].as_array().and_then(|schemes|schemes.iter().find(|scheme|scheme["name"]==settings["ansiScheme"]));
    let custom=scheme.filter(|s|selected=="custom"&&ANSI_KEYS.iter().all(|key|color(&s["colors"][key]).is_some())).map(|s|&s["colors"]);
    let palette=custom.unwrap_or_else(||palettes.get(selected).unwrap_or(&palettes["soft"]));
    let ansi=ANSI_KEYS.map(|key|gpui_kit::rgb(color(&palette[key]).unwrap_or(0xd3e0e8)).into());
    let foreground=gpui_kit::rgb(color(&palette["foreground"]).or_else(||color(&settings["fontColors"][id])).unwrap_or(0xd3e0e8)).into();
    RemoteAppearance{family:family.into(),font_size:px(size.clamp(8.,40.)),bold:settings["fontBold"][id].as_bool().unwrap_or(false),foreground,cursor_accent:color(&palette["cursorAccent"]).map(gpui_kit::rgb).map(Into::into),selection_foreground:color(&palette["selectionForeground"]).map(gpui_kit::rgb).map(Into::into),cursor:color(&palette["cursor"]).map(gpui_kit::rgb).map(Into::into).unwrap_or(foreground),selection:color(&palette["selectionBackground"]).map(gpui_kit::rgb).map(Into::into).unwrap_or_else(||gpui_kit::rgba(0x7195aa66).into()),background:gpui_kit::rgba(0x00000000).into(),ansi}
}

impl DevShellWindow {
    pub(super) fn open_appearance(&mut self,kind:&'static str,window:&mut Window,cx:&mut Context<Self>) {
        let mut original=self.snapshot.appearance.clone();
        original["color"]=original["fontColors"][original["fontId"].as_str().unwrap_or(FONTS[0].0)].as_str().map(Value::from).unwrap_or(Value::from("#d3e0e8"));
        original["uiLight"]=original["uiTextColors"]["light"].as_str().map(Value::from).unwrap_or(Value::from("#243b4e"));
        original["uiDark"]=original["uiTextColors"]["dark"].as_str().map(Value::from).unwrap_or(Value::from("#d4dfe8"));
        self.open_native_form(kind,original,window,cx);
        let form=self.form.as_mut().unwrap();
        for (key,default) in [("color","#d3e0e8"),("uiLight","#243b4e"),("uiDark","#d4dfe8"),("terminalFontSize","14"),("promptUsernameColor",""),("promptHostnameColor","")] {
            let value=form.original.get(key).map(forms::value_text).unwrap_or(default.into());
            form.inputs.insert(key.into(),cx.new(|cx|InputState::new(window,cx).default_value(value).placeholder(default)));
        }
        let font_id=form.original["fontId"].as_str().unwrap_or(FONTS[0].0).to_owned();
        form.flags.insert("bold".into(),form.original["fontBold"][&font_id].as_bool().unwrap_or(false));
        let managed=self.snapshot.config["assets"].as_array().cloned().unwrap_or_default();
        form.add_choices("fontId",FONTS.iter().map(|(id,name,_)|(id.to_string(),name.to_string())).chain(managed.iter().filter(|asset|string(asset,"kind")=="font").map(|asset|(string(asset,"id"),string(asset,"name")))).collect(),&font_id,window,cx);
        form.add_choices("uiFontId",std::iter::once(("builtin:ui-alibaba-puhuiti-3-55".into(),"阿里巴巴普惠体 3.0".into())).chain(managed.iter().filter(|asset|string(asset,"kind")=="ui-font").map(|asset|(string(asset,"id"),string(asset,"name")))).collect(),&string(&form.original,"uiFontId"),window,cx);
        form.add_choices("uiStyle",vec![("classic".into(),"经典 · 清晰柔和".into()),("frosted".into(),"液态玻璃 · 半透明材质".into())],&string(&form.original,"uiStyle"),window,cx);
        let mut palettes=vec![("soft".into(),"柔和".into()),("vivid".into(),"高饱和".into()),("bright".into(),"鲜亮".into())];
        if let Some(schemes)=form.original["ansiSchemes"].as_array(){palettes.extend(schemes.iter().map(|s|(format!("scheme:{}",string(s,"name")),string(s,"name"))));}
        let selected=if string(&form.original,"ansiPalette")=="custom"{format!("scheme:{}",string(&form.original,"ansiScheme"))}else{string(&form.original,"ansiPalette")};
        form.add_choices("ansiPalette",palettes,&selected,window,cx);
        form.add_choices("terminalFontSize",(8..=40).map(|n|(n.to_string(),format!("{n} px"))).collect(),&self.ui.font_size.to_string(),window,cx);
        for key in ["upload","download","latency"] {
            for (suffix,default) in [("Color",""),("Width","1.5")] {let field=format!("{key}{suffix}");let value=form.original["chartStyles"][key][if suffix=="Color"{"color"}else{"width"}].as_str().map(ToString::to_string).or_else(||form.original["chartStyles"][key]["width"].as_f64().map(|v|v.to_string())).unwrap_or(if suffix=="Width"{match key{"upload"=>"2.3","download"=>"1.8",_=>"1.65"}}else{default}.into());form.inputs.insert(field,cx.new(|cx|InputState::new(window,cx).default_value(value).placeholder(default)));}
            let dashed=form.original["chartStyles"][key]["dashed"].as_bool().unwrap_or(key=="download");
            form.add_choices(&format!("{key}Dashed"),vec![("solid".into(),"实线".into()),("dashed".into(),"虚线".into())],if dashed{"dashed"}else{"solid"},window,cx);
            form.flags.insert(format!("{key}Theme"),color(&form.original["chartStyles"][key]["color"]).is_none());
            let value=color(&form.original["chartStyles"][key]["color"]).unwrap_or(match key{"upload"=>if self.ui.dark{0xf5b36c}else{0xac5418},"download"=>if self.ui.dark{0x73d2ec}else{0x087b99},_=>if self.ui.dark{0x9bc4b3}else{0x77a88f}});
            form.inputs[&format!("{key}Color")].update(cx,|input,cx|input.set_value(format!("#{value:06x}"),window,cx));
            let picker=cx.new(|cx|gpui_kit::component::color_picker::ColorPickerState::new(window,cx).default_value(gpui_kit::rgb(value)));
            cx.subscribe_in(&picker,window,move|state,_,event:&gpui_kit::component::color_picker::ColorPickerEvent,window,cx|{let gpui_kit::component::color_picker::ColorPickerEvent::Change(value)=event;if let (Some(f),Some(value))=(state.form.as_mut(),value){let rgba=gpui_kit::Rgba::from(*value);let hex=format!("#{:02x}{:02x}{:02x}",(rgba.r*255.).round() as u8,(rgba.g*255.).round() as u8,(rgba.b*255.).round() as u8);f.inputs[&format!("{key}Color")].update(cx,|input,cx|input.set_value(hex,window,cx));}cx.notify();}).detach();form.colors.insert(format!("{key}Color"),picker);
        }
        self.settings_visible=false;cx.notify();
    }
    pub(super) fn appearance_fields(&self,cx:&mut Context<Self>)->Div {
        let f=self.form.as_ref().unwrap();
        div().flex().flex_col().gap_3()
            .when(f.kind=="shell-fonts",|body|body.child(f.select("fontId","Shell 字体")).child(div().p_4().rounded_lg().bg(gpui_kit::rgb(0x223746)).text_color(gpui_kit::rgb(color(&Value::from(f.text("color",cx))).unwrap_or(0xd3e0e8))).font_family(FONTS.iter().find(|(id,_,_)|*id==f.selected("fontId",cx)).unwrap_or(&FONTS[0]).2).text_size(px(14.)).child("DevShell 012345\n中文目录 · 繁體終端"))
                .child(f.field("terminalFontSize","终端字号（8–40 px）")).child(f.field("color","字体颜色 · HEX")).child(f.toggle("bold","加粗",cx)))
            .when(f.kind=="ui-fonts",|body|body.child(f.select("uiFontId","界面字体")).child(paragraph("Alibaba PuHuiTi 3.0 · 简体中文 · 繁體中文 · English 012345")).child(f.field("uiLight","浅色模式文字颜色 · HEX")).child(f.field("uiDark","深色模式文字颜色 · HEX")))
            .when(matches!(f.kind,"shell-fonts"|"ui-fonts"),|body|{let kind=if f.kind=="ui-fonts"{"ui-font"}else{"font"};body.child(KitButton::new("font-online").secondary().label("在线字体库").on_click(cx.listener(move|state,_,window,cx|state.open_font_library(kind,window,cx))))
                .child(KitButton::new("font-import").secondary().label("导入字体文件").on_click(cx.listener(move|state,_,_,cx|state.import_font(kind,cx))))})
            .when(f.kind=="font-color",|body|body.child(f.field("color","字体颜色 · HEX")))
            .when(f.kind=="ui-style",|body|body.child(f.select("uiStyle","全局主题")))
            .when(f.kind=="charts",|body|body.children([("upload","上传曲线"),("download","下载曲线"),("latency","延迟曲线")].map(|(key,label)|div().flex().flex_col().gap_2().child(paragraph(label)).child(f.field(&format!("{key}Color"),"颜色（留空跟随主题）")).child(f.field(&format!("{key}Width"),"粗细 · 0.5–6 px")))))
            .when(f.kind=="ansi",|body|body.child(f.select("ansiPalette","终端 ANSI 颜色")).child(paragraph("仅调整终端的 16 色语义色，真彩色与 256 色保持原样。")))
            .when(f.kind=="prompt-colors",|body|body.child(f.field("promptUsernameColor","用户名 · @ 前（留空保持原样）")).child(f.field("promptHostnameColor","主机名 · @ 后（留空保持原样）")).child(paragraph("在下一次 Bash / Zsh / Fish 提示符出现时应用。")))
    }
    pub(super) fn save_appearance_form(&mut self,cx:&mut Context<Self>) {
        let Some(f)=&self.form else{return;};let mut patch=serde_json::json!({});
        match f.kind {
            "shell-fonts"=>{let id=f.selected("fontId",cx);let value=f.text("color",cx);if color(&Value::from(value.clone())).is_none(){self.form.as_mut().unwrap().error=Some("请输入 #RRGGBB 格式的颜色".into());cx.notify();return;}
                let mut colors=self.snapshot.appearance["fontColors"].as_object().cloned().unwrap_or_default();colors.insert(id.clone(),value.into());
                let mut bold=self.snapshot.appearance["fontBold"].as_object().cloned().unwrap_or_default();bold.insert(id.clone(),f.flag("bold").into());
                self.ui.font_size=f.text("terminalFontSize",cx).parse::<f32>().unwrap_or(14.).clamp(8.,40.);
                patch=serde_json::json!({"fontId":id,"fontColors":colors,"fontBold":bold,"terminalFontSize":self.ui.font_size});},
            "ui-fonts"=>{patch=serde_json::json!({"uiFontId":f.selected("uiFontId",cx),"uiTextColors":{"light":f.text("uiLight",cx),"dark":f.text("uiDark",cx)}});},
            "ui-style"=>patch=serde_json::json!({"uiStyle":f.selected("uiStyle",cx)}),
            "charts"=>{let mut styles=self.snapshot.appearance["chartStyles"].as_object().cloned().unwrap_or_default();for key in ["upload","download","latency"] {let width=f.text(&format!("{key}Width"),cx).parse::<f64>().ok().filter(|v|v.is_finite()&&*v>=0.5&&*v<=6.);let Some(width)=width else{self.form.as_mut().unwrap().error=Some("曲线粗细应为 0.5–6 px".into());cx.notify();return;};styles.insert(key.into(),serde_json::json!({"color":if f.flag(&format!("{key}Theme")){String::new()}else{f.text(&format!("{key}Color"),cx)},"width":width,"dashed":key!="latency"&&f.selected(&format!("{key}Dashed"),cx)=="dashed"}));}patch=serde_json::json!({"chartStyles":styles});},
            "font-color"=>{let value=f.text("color",cx);if color(&Value::from(value.clone())).is_none(){self.form.as_mut().unwrap().error=Some("请输入 #RRGGBB 格式的颜色".into());cx.notify();return;}let mut colors=self.snapshot.appearance["fontColors"].as_object().cloned().unwrap_or_default();colors.insert(string(&f.original,"_fontId"),value.into());patch=serde_json::json!({"fontColors":colors});},
            "ansi"=>{let palette=f.selected("ansiPalette",cx);patch=if let Some(name)=palette.strip_prefix("scheme:"){serde_json::json!({"ansiPalette":"custom","ansiScheme":name})}else{serde_json::json!({"ansiPalette":palette})};},
            "prompt-colors"=>{patch=serde_json::json!({"promptUsernameColor":f.text("promptUsernameColor",cx),"promptHostnameColor":f.text("promptHostnameColor",cx)});
                let mut sessions=self.parked.iter().filter_map(|s|s.terminal.as_ref()).collect::<Vec<_>>();if let Some(t)=&self.workspace.terminal{sessions.push(t);}
                let body=serde_json::json!({"usernameColor":f.text("promptUsernameColor",cx),"hostnameColor":f.text("promptHostnameColor",cx)});
                for terminal in sessions {if terminal.protocol!="ssh" {continue;}let base=self.snapshot.base.clone();let token=self.token.clone();let id=terminal.session_id.clone();let body=body.clone();cx.background_executor().spawn(async move{post_json(&base,&token,&format!("/api/sessions/{id}/prompt-style"),body)}).detach();}
            },_=>return,
        }
        self.form=None;self.save_appearance(patch,cx);cx.notify();
    }
}
