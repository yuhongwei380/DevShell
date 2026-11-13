use super::*;
use crate::forms::string;
use gpui_kit::{Div,component::Sizable};

impl DevShellWindow {
    pub(super) fn open_help(&mut self,window:&mut Window,cx:&mut Context<Self>){self.open_native_form("help",serde_json::json!({"_section":"start"}),window,cx);self.settings_visible=false;}
    pub(super) fn help_fields(&self,p:workspace::Palette,cx:&mut Context<Self>)->Div{
        let sections:Vec<Value>=serde_json::from_str(include_str!("../assets/help.json")).unwrap();
        let selected=string(&self.form.as_ref().unwrap().original,"_section");
        let current=sections.iter().find(|s|string(s,"id")==selected).unwrap_or(&sections[0]);
        div().h(px(500.)).max_h(px(500.)).flex().gap_4()
            .child(div().id("manual-navigation").w(px(220.)).flex_shrink_0().overflow_y_scroll().flex().flex_col().gap_1().children(sections.iter().map(|s|{
                let id=string(s,"id");KitButton::new(format!("manual-{id}")).ghost().small().w_full().justify_start().label(string(s,"title")).when(id==selected,|b|b.secondary()).on_click(cx.listener(move|state,_,_,cx|{if let Some(f)=state.form.as_mut(){f.original["_section"]=id.clone().into();}cx.notify();}))
            })))
            .child(div().id("manual-content").flex_1().min_w_0().overflow_y_scroll().p_3().flex().flex_col().gap_4().child(heading(string(current,"title"))).child(div().text_size(px(13.)).line_height(px(22.)).text_color(gpui_kit::rgb(p.text)).child(string(current,"content"))))
    }
}
