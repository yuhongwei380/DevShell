use super::*;

#[test]
fn pasted_unicode_and_mouse_reports_survive_bounded_transport_frames(){
    let mut bytes="中文🙂\r\n".repeat(12000).into_bytes();bytes.extend_from_slice(&[0x1b,b'[',b'M',32,255,128]);
    let frames=terminal_input_frames(&bytes);assert!(frames.len()>1);let mut restored=Vec::new();
    for frame in frames {let value:Value=serde_json::from_str(&frame).unwrap();assert_eq!(value["type"],"input");let chunk=base64::engine::general_purpose::STANDARD.decode(value["dataBase64"].as_str().unwrap()).unwrap();assert!(chunk.len()<=32768);restored.extend(chunk);}
    assert_eq!(restored,bytes);
    use ely_gpui_component::terminal::mouse_report;
    assert_eq!(mouse_report(true,false,0,79,23,false),b"\x1b[<0;80;24M");
    assert_eq!(mouse_report(true,false,0,79,23,true),b"\x1b[<0;80;24m");
    assert_eq!(mouse_report(false,false,0,222,95,false),vec![27,b'[',b'M',32,255,128]);
}

#[test]
fn imported_woff_fonts_are_decoded_and_keep_their_real_family(){
    let (bytes,family)=fonts::decode_font(include_bytes!("../../web/assets/fonts/jetbrains-mono.woff2").to_vec()).unwrap();
    assert!(family.contains("JetBrains"));assert!(ttf_parser::Face::parse(&bytes,0).is_ok());
    let mut bomb=vec![0;44];bomb[..4].copy_from_slice(b"wOF2");bomb[16..20].copy_from_slice(&u32::MAX.to_be_bytes());assert!(fonts::decode_font(bomb).is_err());
}

#[test]
fn ui_scale_preserves_the_minimum_workspace_and_handles_invalid_values(){
    assert_eq!(ui_scale::effective(2.,1280.,800.),1.16);
    assert_eq!(ui_scale::effective(2.,720.,480.),0.65);
    assert_eq!(ui_scale::effective(f32::NAN,1280.,800.),1.);
    assert_eq!(ui_scale::effective_for(1.,1280.,800.,true),0.83);
}

#[test]
fn editing_a_profile_retains_forwarding_and_protocol_options() {
    let original=serde_json::json!({"id":"one","name":"old","protocol":"rdp","domain":"ACME","forwards":[{"direction":"local","listenPort":5432}],"future":{"value":true},"hasSecret":true,"deletedAt":"yesterday"});
    let merged=forms::merge_profile(&original,serde_json::json!({"name":"new","secret":"","clearSecret":false}));
    assert_eq!(merged["forwards"],original["forwards"]);
    assert_eq!(merged["future"],original["future"]);
    assert_eq!(merged["domain"],"ACME");
    assert_eq!(merged["name"],"new");
    assert!(!merged["clearSecret"].as_bool().unwrap());
    assert!(merged.get("hasSecret").is_none());
    assert!(merged.get("deletedAt").is_none());
}

#[test]
fn server_tree_scope_keeps_nested_groups_and_handles_cycles() {
    let config=serde_json::json!({"groupNodes":[{"id":"root","name":"Root"},{"id":"child","parentId":"root","name":"Child"},{"id":"deep","parentId":"child","name":"Deep"},{"id":"other","name":"Other"},{"id":"loop-a","parentId":"loop-b"},{"id":"loop-b","parentId":"loop-a"}]});
    let groups=server_manager::group_order(&config);
    assert_eq!(groups.len(),6);
    let scope=server_manager::group_scope(&groups,"root",true);
    assert_eq!(scope.into_iter().collect::<Vec<_>>(),vec!["child","deep","root"]);
    assert_eq!(server_manager::group_scope(&groups,"root",false).len(),1);
    assert_eq!(server_manager::group_scope(&groups,"",true).len(),1);
    assert_eq!(server_manager::group_ancestors(&groups,"deep"),vec!["child","root"]);
    assert_eq!(server_manager::group_ancestors(&groups,"loop-a"),vec!["loop-b"]);
}

#[test]
fn credential_prompt_respects_protocol_and_managed_key_resolution() {
    for profile in [serde_json::json!({"auth":"password"}),serde_json::json!({"protocol":"rdp"}),serde_json::json!({"auth":"key","keyPath":"id_ed25519"})] {assert!(needs_credential(&profile));}
    for profile in [serde_json::json!({"auth":"password","hasSecret":true}),serde_json::json!({"protocol":"serial"}),serde_json::json!({"auth":"agent"}),serde_json::json!({"auth":"key","keyId":"managed"})] {assert!(!needs_credential(&profile));}
}

#[test]
fn terminal_uses_pinned_fonts_and_exact_baseline_palette_with_safe_fallback() {
    let mut settings=serde_json::json!({"fontId":"builtin:jetbrains-mono","fontBold":{"builtin:jetbrains-mono":true},"fontColors":{"builtin:jetbrains-mono":"#a9dfbf"},"ansiPalette":"custom","ansiScheme":"incomplete","ansiSchemes":[{"name":"incomplete","colors":{"red":"#ff0000"}}]});
    let look=appearance::terminal_appearance(&settings,14.,0x223746);
    assert_eq!(look.family.as_ref(),"Deng JetBrains Mono");assert!(look.bold);
    assert_eq!(look.foreground,gpui_kit::rgb(0xa9dfbf).into());
    assert_eq!(look.ansi[1],gpui_kit::rgb(0xdf9c96).into(),"incomplete schemes fall back atomically");
    settings["ansiPalette"]=Value::from("vivid");
    assert_eq!(appearance::terminal_appearance(&settings,50.,0).ansi[1],gpui_kit::rgb(0xd4726a).into());
    assert_eq!(appearance::terminal_appearance(&settings,50.,0).font_size,px(40.));
}

fn session(id: &str, path: &str) -> SessionWorkspace {
    let screen = std::sync::Arc::new(std::sync::Mutex::new(screen::TerminalScreen::default()));
    screen.lock().unwrap().feed(id.as_bytes());
    SessionWorkspace {
        terminal: Some(TerminalState {
            protocol: "ssh".into(),
            profile_id: id.into(),
            name: id.into(),
            host: id.into(),
            session_id: id.into(),
            screen,
            status: "已连接".into(),
            input_tx: None,
            view: None,
        }),
        files: Some(FileListState {
            path: path.into(),
            entries: vec![],
            status: "已读取".into(),
        }),
        file_ui:crate::file_view::FileUi{selected:[format!("{id}.txt")].into_iter().collect(),..Default::default()},
        ..Default::default()
    }
}

#[test]
fn tab_switch_preserves_terminal_directory_and_confirmation() {
    let mut active = session("first", "/first");
    let mut parked = vec![session("second", "/second")];
    assert!(select_session(&mut active, &mut parked, "second"));
    assert_eq!(active.files.as_ref().unwrap().path, "/second");
    assert_eq!(
        active
            .terminal
            .as_ref()
            .unwrap()
            .screen
            .lock()
            .unwrap()
            .text(),
        "second"
    );
    assert_eq!(
        active.file_ui.selected.iter().next().map(String::as_str),
        Some("second.txt")
    );
    assert!(select_session(&mut active, &mut parked, "first"));
    assert_eq!(active.files.as_ref().unwrap().path, "/first");
    assert_eq!(
        active
            .terminal
            .as_ref()
            .unwrap()
            .screen
            .lock()
            .unwrap()
            .text(),
        "first"
    );
    assert_eq!(parked.len(), 1);
}

#[test]
fn missing_or_active_tab_does_not_replace_current_session() {
    let mut active = session("first", "/first");
    let mut parked = vec![session("second", "/second")];
    assert!(!select_session(&mut active, &mut parked, "first"));
    assert!(!select_session(&mut active, &mut parked, "missing"));
    assert_eq!(active.terminal.as_ref().unwrap().session_id, "first");
    assert_eq!(parked.len(), 1);
}

#[test]
fn background_file_result_updates_original_tab_only() {
    let mut active = session("first", "/first");
    let mut parked = vec![session("second", "/second")];
    find_session(&mut active, &mut parked, "second")
        .unwrap()
        .files
        .as_mut()
        .unwrap()
        .path = "/completed-in-background".into();
    assert_eq!(active.files.as_ref().unwrap().path, "/first");
    assert_eq!(
        parked[0].files.as_ref().unwrap().path,
        "/completed-in-background"
    );
    assert!(find_session(&mut active, &mut parked, "closed-session").is_none());
}

#[test]
fn long_commands_and_unicode_paste_use_extended_websocket_frames() {
    let text = "中文命令".repeat(100);
    let frame = websocket_frame(1, text.as_bytes()).unwrap();
    assert_eq!(frame[0], 0x81);
    assert_eq!(frame[1], 0xfe);
    assert_eq!(
        u16::from_be_bytes([frame[2], frame[3]]) as usize,
        text.len()
    );
    let decoded = frame[8..]
        .iter()
        .enumerate()
        .map(|(i, byte)| byte ^ frame[4 + i % 4])
        .collect::<Vec<_>>();
    assert_eq!(decoded, text.as_bytes());
    assert!(websocket_frame(9, text.as_bytes()).is_err());
}

#[test]
fn sftp_layout_reclaims_space_and_respects_minimum_terminal_size() {
    use workspace::{FilesPosition, ShellLayout};
    for (width, height) in [
        (720., 480.),
        (999., 600.),
        (1000., 800.),
        (1280., 800.),
        (1920., 1080.),
    ] {
        let hidden = ShellLayout::new(width, height, true, false, FilesPosition::Bottom, None);
        for position in [
            FilesPosition::Bottom,
            FilesPosition::Top,
            FilesPosition::Right,
        ] {
            let visible = ShellLayout::new(width, height, true, true, position, None);
            assert!(
                visible.terminal_width >= 330.,
                "{width}×{height}: {position:?}"
            );
            assert!(
                visible.terminal_height >= 220.,
                "{width}×{height}: {position:?}"
            );
            if position == FilesPosition::Right {
                assert_eq!(visible.terminal_height, hidden.terminal_height);
                assert_eq!(
                    visible.terminal_width + visible.files_extent + 8.,
                    hidden.terminal_width
                );
            } else {
                assert_eq!(visible.terminal_width, hidden.terminal_width);
                assert_eq!(
                    visible.terminal_height + visible.files_extent + 8.,
                    hidden.terminal_height
                );
            }
        }
    }
    let dragged = ShellLayout::new(1000., 800., true, true, FilesPosition::Right, Some(10_000.));
    assert!(dragged.terminal_width >= 330.);
    let dragged = ShellLayout::new(1280., 800., true, true, FilesPosition::Top, Some(10_000.));
    assert_eq!(dragged.terminal_height, 220.);
}

#[test]
fn file_sort_preserves_folders_and_orders_sizes_and_modification_times() {
    let entry = |name: &str, kind: &str, bytes, modified_at| RemoteEntry {
        name: name.into(),
        kind: kind.into(),
        link: false,
        bytes,
        modified_at,
        modified: String::new(),
        mode: String::new(),
        owner: String::new(),
    };
    let entries = vec![
        entry("large", "file", 100, 10),
        entry("folder", "folder", 0, 0),
        entry("small", "file", 1, 20),
    ];
    for descending in [true, false] {
        let mut sorted = entries.clone();
        sorted.sort_by(|a, b| file_browser::compare_entries(a, b, "size", descending));
        assert_eq!(sorted[0].name, "folder");
        assert_eq!(sorted[1].name, if descending { "large" } else { "small" });
        sorted.sort_by(|a, b| file_browser::compare_entries(a, b, "time", descending));
        assert_eq!(sorted[0].name, "folder");
        assert_eq!(sorted[1].name, if descending { "small" } else { "large" });
    }
}

#[test]
fn file_selection_respects_order_modifiers_and_keeps_the_range_anchor(){
    use std::collections::BTreeSet;
    let names=["dir","a.txt","b.txt","c.txt"].map(str::to_owned);let mut selected=BTreeSet::new();let mut anchor=None;
    crate::file_view::select_names(&mut selected,&mut anchor,&names,"a.txt",false,false);
    crate::file_view::select_names(&mut selected,&mut anchor,&names,"c.txt",false,true);
    assert_eq!(selected,["a.txt","b.txt","c.txt"].map(str::to_owned).into_iter().collect());assert_eq!(anchor.as_deref(),Some("a.txt"));
    crate::file_view::select_names(&mut selected,&mut anchor,&names,"b.txt",true,false);
    assert!(!selected.contains("b.txt"));assert!(selected.contains("a.txt"));
    crate::file_view::select_names(&mut selected,&mut anchor,&names,"dir",false,false);
    assert_eq!(selected,["dir".to_owned()].into_iter().collect());
}
#[test]
fn remote_delete_rejects_root_nul_and_parent_traversal(){
    for path in ["/","///","/./","/var/../","/var/a\0b","relative"]{assert!(!crate::file_actions::valid_delete_target(path),"{path:?}");}
    assert!(crate::file_actions::valid_delete_target("/var/www/中文.txt"));assert!(crate::file_actions::valid_delete_target("/.env"));
}

#[test]
fn file_list_retains_symlinks_and_server_timestamps() {
    let files = parse_file_list(&serde_json::json!({"path":"/var","entries":[{"name":"current","kind":"file","link":true,"modifiedAt":1793701200123_i64},{"name":"plain","kind":"file"}]}),"/");
    assert_eq!(files.path,"/var");
    assert!(files.entries[0].link);
    assert!(!files.entries[1].link);
    assert_eq!(files.entries[0].modified_at,1793701200123);
}

#[test]
fn security_editor_keeps_manual_lock_and_defaults_to_a_password_method() {
    let draft=crate::security_panel::security_draft(&serde_json::json!({"enabled":false,"idleSeconds":0,"passwordConfigured":false,"twoFactorConfigured":false}));
    assert_eq!(draft["lockMode"],"manual");assert_eq!(draft["idleSeconds"],300);assert_eq!(draft["passwordEnabled"],true);
    let draft=crate::security_panel::security_draft(&serde_json::json!({"enabled":true,"idleSeconds":600,"passwordConfigured":false,"twoFactorConfigured":true}));
    assert_eq!(draft["lockMode"],"idle");assert_eq!(draft["idleSeconds"],600);assert_eq!(draft["passwordEnabled"],false);assert_eq!(draft["twoFactorEnabled"],true);
}

#[test]
fn security_status_rejects_delayed_unlock_and_incomplete_replies() {
    let locked=serde_json::json!({"enabled":true,"locked":true,"revision":8});
    assert!(!crate::lock_panel::accept_status(&locked,&serde_json::json!({"enabled":true,"locked":false,"revision":7})));
    assert!(!crate::lock_panel::accept_status(&locked,&serde_json::json!({"revision":9})));
    assert!(crate::lock_panel::accept_status(&locked,&serde_json::json!({"enabled":true,"locked":false,"revision":9})));
    assert!(crate::lock_panel::accept_status(&locked,&serde_json::json!({"enabled":true,"locked":true,"revision":8,"retryAfterSeconds":12})));
}

#[test]
fn unlock_choices_only_include_configured_methods() {
    let only_totp=crate::lock_panel::unlock_methods(&serde_json::json!({"passwordConfigured":false,"twoFactorConfigured":true}));
    assert_eq!(only_totp.iter().map(|(id,_)|id.as_str()).collect::<Vec<_>>(),vec!["totp"]);
    assert!(crate::lock_panel::unlock_methods(&serde_json::json!({})).is_empty());
    assert_eq!(crate::lock_panel::unlock_methods(&serde_json::json!({"passwordConfigured":true,"twoFactorConfigured":true})).len(),2);
}

#[test]
fn old_directory_replies_and_failed_reads_preserve_the_current_directory(){
    let mut workspace=session("first","/var/www");workspace.file_ui.navigation=3;workspace.file_ui.selected.insert("keep.txt".into());
    assert!(!workspace.accept_listing(2,"/old".into(),Ok(FileListState{path:"/old".into(),entries:vec![],status:"已加载".into()})));assert_eq!(workspace.files.as_ref().unwrap().path,"/var/www");
    assert!(workspace.accept_listing(3,"/denied".into(),Err("Permission denied".into())));assert_eq!(workspace.files.as_ref().unwrap().path,"/var/www");assert!(workspace.file_ui.selected.contains("keep.txt"));assert!(workspace.files.as_ref().unwrap().status.contains("Permission denied"));
    assert!(workspace.accept_listing(3,"/new".into(),Ok(FileListState{path:"/new".into(),entries:vec![],status:"已加载".into()})));assert!(workspace.file_ui.selected.is_empty());assert_eq!(workspace.files.as_ref().unwrap().path,"/new");
}

#[test]
fn terminal_ground_is_transparent_and_imported_palette_extras_are_retained(){
    let settings=serde_json::json!({});let look=appearance::terminal_appearance(&settings,14.,0x223746);assert_eq!(look.background,gpui_kit::rgba(0x00000000).into());assert_eq!(look.cursor,look.foreground);assert_eq!(look.selection,gpui_kit::rgba(0x7195aa66).into());
    let palettes:Value=serde_json::from_str(include_str!("../assets/ansi-palettes.json")).unwrap();let mut colors=palettes["soft"].clone();colors["foreground"]=Value::from("#aabbcc");colors["background"]=Value::from("#112233");colors["cursor"]=Value::from("#223344");colors["selectionBackground"]=Value::from("#334455");
    let custom=appearance::terminal_appearance(&serde_json::json!({"ansiPalette":"custom","ansiScheme":"theme","ansiSchemes":[{"name":"theme","colors":colors}]}),14.,0);
    assert_eq!(custom.background,gpui_kit::rgba(0x00000000).into());assert_eq!(custom.foreground,gpui_kit::rgb(0xaabbcc).into());assert_eq!(custom.cursor,gpui_kit::rgb(0x223344).into());assert_eq!(custom.selection,gpui_kit::rgb(0x334455).into());
}
