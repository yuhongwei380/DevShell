use std::cell::Cell;
thread_local! {static SCALE:Cell<f32>=const{Cell::new(1.)};static ICON:Cell<u32>=const{Cell::new(0x344f62)};static DARK:Cell<bool>=const{Cell::new(false)};}

// GPUI layout uses device-independent pixels. The original UI's px metrics
// must scale together with rem-based native controls and hit-test geometry.
pub(super) fn current()->f32{SCALE.with(Cell::get)}
pub(super) fn set(value:f32){SCALE.with(|scale|scale.set(value));}
#[cfg(test)]
pub(super) fn effective(requested:f32,width:f32,height:f32)->f32 {effective_for(requested,width,height,false)}
pub(super) fn effective_for(requested:f32,width:f32,height:f32,right:bool)->f32 {
    let requested=if requested.is_finite()&&requested>0.{requested.clamp(0.5,2.)}else{1.};
    let fit=((width/(1100.+if right{428.}else{0.})).min(height/420.)*100.).floor()/100.;requested.min(fit.max(0.5))
}

pub(super) fn pixels(value:f32)->gpui_kit::Pixels {gpui_kit::px(value*current())}

pub(super) fn icon_color()->u32{ICON.with(Cell::get)}
pub(super) fn set_icon_color(value:u32){ICON.with(|color|color.set(value));}

pub(super) fn dark()->bool{DARK.with(Cell::get)}
pub(super) fn set_dark(value:bool){DARK.with(|dark|dark.set(value));}
