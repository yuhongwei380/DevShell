//! Captures this application's own GPUI scene; never reads another window.
use super::*;
use crate::forms::NativeForm;

pub(super) fn start(
    directory: std::path::PathBuf,
    entity: &Entity<DevShellWindow>,
    window: &Window,
    cx: &mut App,
) {
    entity.update(cx, |_, cx| {
        cx.spawn_in(window, async move |this, cx| {
            fs::create_dir_all(&directory).expect("create native visual test output");
            cx.update(|_, cx| {
                // A wrong family silently selects the system fallback on Windows.
                let fira = cx
                    .text_system()
                    .resolve_font(&gpui_kit::font(appearance::FONTS[1].2));
                assert_eq!(
                    cx.text_system().units_per_em(fira),
                    2000,
                    "Fira Code fell back to another font"
                );
                let ui = cx
                    .text_system()
                    .resolve_font(&gpui_kit::font("Alibaba PuHuiTi 3.0"));
                assert!(
                    (f32::from(
                        cx.text_system()
                            .advance(ui, gpui_kit::px(100.), 'x')
                            .unwrap()
                            .width
                    ) - 51.6)
                        .abs()
                        < 0.05,
                    "UI font fallback"
                );
            })
            .ok();
            for dark in [false, true] {
                for stage in [
                    "main",
                    "servers",
                    "connection",
                    "settings",
                    "ui-style",
                    "charts",
                    "shell-fonts",
                    "ui-fonts",
                    "files-bottom",
                    "files-top",
                    "files-right",
                    "files-selection",
                    "files-menu",
                    "files-single-menu",
                    "files-create-menu",
                    "files-open-with",
                    "files-new",
                    "files-delete",
                    "files-permissions",
                    "files-upload",
                    "ansi",
                    "ansi-populated",
                    "keys",
                    "key-import",
                    "key-generate",
                    "security",
                    "monitor",
                ] {
                    this.update_in(cx, |state, window, cx| {
                        state.snapshot.appearance["theme"] =
                            Value::from(if dark { "dark" } else { "light" });
                        state.snapshot.appearance["uiScale"] = Value::from(1.);
                        state.snapshot.appearance["monitorVisible"] = Value::from(false);
                        state.snapshot.appearance["layout"]["conciseMode"] = Value::from(true);
                        state.ui.restore(&state.snapshot.appearance, dark);
                        state.ui.files_visible=false;state.ui.files_extent=None;
                        state.ui.connections_visible = matches!(stage, "servers" | "connection");
                        state.settings_visible = stage == "settings";
                        state.ui.settings_view = "main";
                        state.ui.settings.preferences = None;
                        state.ui.settings.keys_visible = false;
                        state.form = None;
                        state.feedback = None;
                        state.ui.manager.menu = None;
                        state.workspace = SessionWorkspace::default();
                        if matches!(stage,"keys"|"key-import"|"key-generate"){state.ui.settings.keys_visible=true;if stage!="keys"{state.open_key_editor(if stage=="key-import"{"import"}else{"generate"},serde_json::json!({}),window,cx);}}
                        if stage=="security"{state.open_security_settings(window,cx);}
                        state.snapshot.appearance["ansiSchemes"]=serde_json::json!([]);
                        state.snapshot.appearance["ansiPalette"]=Value::from("soft");
                        if stage=="ansi-populated"{let colors:Value=serde_json::from_str(include_str!("../assets/ansi-palettes.json")).unwrap();state.snapshot.appearance["ansiSchemes"]=serde_json::json!([{"name":"柔和测试方案","colors":colors["soft"]},{"name":"鲜亮测试方案","colors":colors["bright"]}]);state.snapshot.appearance["ansiPalette"]=Value::from("custom");state.snapshot.appearance["ansiScheme"]=Value::from("柔和测试方案");}
                        if stage.starts_with("files-")||stage=="monitor" {
                            let data:Value=serde_json::from_str(include_str!("../assets/file-fixture.json")).unwrap();
                            let screen=std::sync::Arc::new(std::sync::Mutex::new(screen::TerminalScreen::default()));
                            screen.lock().unwrap().feed(b"root@dev-server:/var/www# ");
                            let (sender,_receiver)=std::sync::mpsc::channel();
                            state.workspace.terminal=Some(TerminalState{protocol:"ssh".into(),profile_id:"native-fixture".into(),name:"Dev Server".into(),host:"192.0.2.10".into(),session_id:"native-fixture".into(),status:"已连接".into(),screen,input_tx:Some(sender),view:Some(cx.new(|cx|{let mut terminal=ely_gpui_component::terminal::Terminal::remote(120,30,|_|{},cx);terminal.feed(b"root@dev-server:/var/www# ",cx);terminal}))});
                            state.workspace.charts.started=true;
                            state.workspace.files=Some(parse_file_list(&data,"/var/www"));
                            state.workspace.file_ui.expanded=["/","/var","/var/www"].map(str::to_owned).into_iter().collect();
                            for (path,entries) in data["folders"].as_object().unwrap(){state.workspace.file_ui.folders.insert(path.clone(),parse_file_list(&serde_json::json!({"path":path,"entries":entries}),path).entries);}
                            state.ui.files_visible=true;state.ui.files_pane="files";state.ui.files_position=if stage=="files-right"{workspace::FilesPosition::Right}else if stage=="files-top"{workspace::FilesPosition::Top}else{workspace::FilesPosition::Bottom};state.ui.files_extent=Some(if stage=="files-right"{420.}else{300.});
                            if stage=="monitor"{let data:Value=serde_json::from_str(include_str!("../assets/monitor-fixture.json")).unwrap();state.workspace.monitor=Some(MonitorState{raw:data["stats"].clone(),load:"0.28 / 0.17 / 0.12".into(),status:"已连接".into()});state.workspace.charts.network=data["network"].clone();state.workspace.charts.latency=data["latency"].clone();state.workspace.charts.fixed_now=data["now"].as_i64();state.ui.monitor_visible=true;state.ui.files_visible=false;}
                            if matches!(stage,"files-selection"|"files-menu"){state.workspace.file_ui.selected=["app.js","index.html"].map(str::to_owned).into_iter().collect();}
                            if stage=="files-menu"{state.workspace.file_ui.menu=Some((gpui_kit::point(px(650.),px(545.)),state.selected_file_entries()));}
                            if matches!(stage,"files-single-menu"|"files-open-with") {
                                state.workspace.file_ui.selected=["app.js".to_owned()].into_iter().collect();
                                state.open_file_menu(gpui_kit::point(px(650.),px(300.)),state.selected_file_entries(),false,window,cx);
                                if stage=="files-open-with"{state.workspace.file_ui.submenu=Some("open-submenu");state.workspace.file_ui.menu_index=1;}
                            }
                            if stage=="files-create-menu"{state.open_file_menu(gpui_kit::point(px(1040.),px(565.)),Vec::new(),true,window,cx);}
                            if stage=="files-new"{state.file_context_action("new-file",window,cx);}
                            if stage=="files-delete"{state.workspace.file_ui.selected=["app.js","index.html"].map(str::to_owned).into_iter().collect();state.file_context_action("delete",window,cx);}
                            if stage=="files-permissions"{state.open_native_form("file-action",serde_json::json!({"_action":"permissions","_title":"文件权限","_server":"Dev Server","_sessionId":"native-fixture","_targets":["/var/www/app.js"],"_base":"/var/www","_permissions":{"mode":"0644","owner":"www / www"},"mode":"0644"}),window,cx);}
                            if stage=="files-upload"{state.form=Some(NativeForm::new("file-upload",serde_json::json!({"_sessionId":"native-fixture","directory":"/var/www","paths":[],"_conflicts":[{"path":"/var/www/app.js","canOverwrite":true},{"path":"/var/www/index.html","canOverwrite":true}]}),window,cx));}
                        }
                        if stage == "connection" {
                            let group = server_manager::group_order(&state.snapshot.config)
                                .first()
                                .map(|(group, _)| forms::string(group, "id"))
                                .unwrap_or_default();
                            state.open_native_form(
                                "connection",
                                serde_json::json!({"groupId":group}),
                                window,
                                cx,
                            );
                        }
                        if matches!(stage, "ui-style" | "charts" | "shell-fonts" | "ui-fonts" | "ansi" | "ansi-populated") {
                            state.open_appearance(if stage=="ansi-populated"{"ansi"}else{stage}, window, cx);
                        }
                        workspace::apply_theme(dark, Some(window), cx);
                        cx.notify();
                    })
                    .expect("set native visual test state");
                    for _ in 0..3 {
                        cx.background_executor()
                            .timer(Duration::from_millis(150))
                            .await;
                        cx.update(|window, cx| {
                            let _ = window.draw(cx);
                        })
                        .expect("draw native test frame");
                    }
                    let path = directory.join(format!(
                        "{stage}-{}.png",
                        if dark { "dark" } else { "light" }
                    ));
                    cx.update(|window, _| {
                        let image = window.render_to_image().expect("render GPUI scene");
                        eprintln!(
                            "{stage}: viewport={:?}, scale={}, raster={}x{}",
                            window.viewport_size(),
                            window.scale_factor(),
                            image.width(),
                            image.height()
                        );
                        image_test::imageops::resize(
                            &image,
                            1280,
                            800,
                            image_test::imageops::FilterType::Lanczos3,
                        )
                        .save(&path)
                        .expect("save GPUI scene");
                    })
                    .expect("capture native test state");
                }
            }
            this.update_in(cx,|state,window,cx|{state.open_file_menu(gpui_kit::point(px(500.),px(400.)),Vec::new(),false,window,cx);}).expect("open keyboard menu probe");
            cx.update(|window,cx|{let _=window.draw(cx);window.dispatch_keystroke(gpui_kit::Keystroke::parse("down").unwrap(),cx);}).expect("navigate file menu");
            this.update(cx,|state,_|{assert_eq!(state.workspace.file_ui.menu_index,2,"keyboard navigation selected a separator");}).expect("verify separator navigation");
            cx.update(|window,cx|{let _=window.draw(cx);window.dispatch_keystroke(gpui_kit::Keystroke::parse("right").unwrap(),cx);}).expect("open create submenu");
            this.update(cx,|state,_|{assert_eq!(state.workspace.file_ui.submenu,Some("create-submenu"));}).expect("verify create submenu");
            cx.update(|window,cx|{let _=window.draw(cx);window.dispatch_keystroke(gpui_kit::Keystroke::parse("down").unwrap(),cx);window.dispatch_keystroke(gpui_kit::Keystroke::parse("enter").unwrap(),cx);}).expect("choose new directory with keyboard");
            this.update(cx,|state,_|{assert_eq!(state.form.as_ref().unwrap().original["_action"],"mkdir");assert_eq!(state.form.as_ref().unwrap().original["_base"],"/var/www");}).expect("verify keyboard action target");
            this.update_in(cx,|state,_,cx|{state.form=None;state.browse_files("/__native_navigation_probe__".into(),cx);state.preview_file("/__native_preview_probe__".into(),cx);}).expect("dispatch leased-window file callbacks");
            cx.background_executor().timer(Duration::from_millis(500)).await;
            this.update(cx,|state,_|{assert_eq!(state.workspace.file_ui.navigation,1,"navigation callback did not run");assert_eq!(state.workspace.files.as_ref().unwrap().path,"/var/www","failed navigation replaced the old listing");}).expect("verify file callback regression");
            cx.update(|_, cx| cx.quit()).ok();
        })
        .detach();
    });
}
