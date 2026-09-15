use super::*;
use crate::protocol::{
    SurfaceGraphicsAsset, SurfaceGraphicsAssetKey, SurfaceGraphicsFormat, SurfaceGraphicsPlacement,
    SurfaceGraphicsSource, SurfaceGraphicsTarget,
};

fn image(
    target: SurfaceGraphicsTarget,
    x: u16,
    y: u16,
    id: u32,
) -> (SurfaceGraphicsAsset, SurfaceGraphicsPlacement) {
    let key = SurfaceGraphicsAssetKey {
        source: SurfaceGraphicsSource::Terminal {
            target,
            image_id: id,
        },
        image_width: 1,
        image_height: 1,
        format: SurfaceGraphicsFormat::Rgba,
        data_len: 4,
        data_fingerprint: u64::from(id),
    };
    (
        SurfaceGraphicsAsset {
            key: key.clone(),
            data: vec![1, 2, 3, 4],
        },
        SurfaceGraphicsPlacement {
            asset: key,
            logical_placement_id: id,
            x,
            y,
            cols: 1,
            rows: 1,
            source_x: 0,
            source_y: 0,
            source_width: 1,
            source_height: 1,
            x_offset: 0,
            y_offset: 0,
            z: 0,
            scrollback_offset: 0,
        },
    )
}

fn add_main_image(
    surface: &mut PaneSurfaceFrame,
    layout: ClientShellLayout,
    point: (u16, u16),
    id: u32,
) {
    let (asset, placement) = image(
        SurfaceGraphicsTarget::Pane {
            pane_id: "pane_1".into(),
        },
        point.0 - layout.pane_surface.x,
        point.1 - layout.pane_surface.y,
        id,
    );
    surface.graphics.assets.push(asset);
    surface.graphics.placements.push(placement);
}

#[cfg(unix)]
#[test]
fn failed_direct_ack_composition_restores_graphics_for_inline_retry() {
    use crate::kitty_graphics::surface::host_image_id;

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane = surface();
    let (mut asset, mut placement) = image(
        SurfaceGraphicsTarget::Pane {
            pane_id: "pane_1".into(),
        },
        0,
        0,
        71,
    );
    asset.key.source = SurfaceGraphicsSource::PaneLayer {
        pane_id: "pane_1".into(),
        layer_id: "direct-rollback".into(),
    };
    placement.asset = asset.key.clone();
    let key = asset.key.clone();
    pane.graphics.assets.push(asset);
    pane.graphics.placements.push(placement);
    state.set_pane_surface(pane);

    // Model an ACK arriving while projection pairing makes composition unavailable.
    state.pending_pane_surface = Some(surface());
    let image_id = host_image_id(state.graphics_scope(), &key);
    let graphics_before = format!("{:?}", state.graphics);
    let checkpoint = state.direct_graphics_checkpoint();
    assert!(state.trust_direct_graphics_asset(&key, image_id));
    assert!(state.compose(106, 20).is_none());
    state.restore_direct_graphics_checkpoint(checkpoint);
    assert_eq!(format!("{:?}", state.graphics), graphics_before);

    // Once pairing is available, fallback still owns the asset and uploads it normally.
    state.pending_pane_surface = None;
    let retry = state.compose(106, 20).expect("fallback frame");
    assert!(String::from_utf8_lossy(&retry.graphics.into_inline_bytes()).contains("a=t"));
}

fn is_placed(bytes: &[u8], point: (u16, u16)) -> bool {
    String::from_utf8_lossy(bytes).contains(&format!("\x1b[{};{}H", point.1 + 1, point.0 + 1))
}

fn assert_graphics_cover(state: &mut ClientShellState, covered: Rect, cols: u16, rows: u16) {
    let layout = state.layout(cols, rows);
    let covered = covered.intersection(layout.pane_surface);
    assert!(!covered.is_empty());
    let inside = (covered.right() - 1, covered.bottom() - 1);
    let outside = (layout.pane_surface.y..layout.pane_surface.bottom())
        .find_map(|y| {
            (layout.pane_surface.x..layout.pane_surface.right())
                .find(|&x| !covered.contains((x, y).into()))
                .map(|x| (x, y))
        })
        .unwrap();
    let mut surface = surface();
    add_main_image(&mut surface, layout, outside, 1);
    add_main_image(&mut surface, layout, inside, 2);
    state.set_pane_surface(surface);
    let frame = state.compose(cols, rows).unwrap();
    assert!(
        is_placed(&frame.graphics.clone().into_inline_bytes(), outside),
        "outside={outside:?} cover={covered:?}"
    );
    assert!(
        !is_placed(&frame.graphics.clone().into_inline_bytes(), inside),
        "inside={inside:?} cover={covered:?}"
    );
}

#[test]
fn notifications_and_clipboard_feedback_only_cover_their_drawn_corners() {
    use crate::config::{ToastClipboardPosition as Clipboard, ToastHerdrPosition as Toast};
    for (cols, rows) in [(106, 40), (40, 24)] {
        for position in [
            Toast::TopLeft,
            Toast::TopRight,
            Toast::BottomLeft,
            Toast::BottomRight,
        ] {
            let mut state =
                ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
            state.sidebar_collapsed = true;
            state.set_snapshot(Box::new(snapshot()));
            state.set_pane_surface(surface());
            state.visible_notification = Some(ClientVisibleNotification {
                endpoint_id: ClientEndpointId::Local,
                event: SemanticNotification {
                    kind: SemanticNotificationKind::Custom,
                    title: "notice".into(),
                    body: Some("body".into()),
                    sound: None,
                    agent: None,
                    workspace_id: None,
                    tab_id: None,
                    pane_id: None,
                    position: Some(position),
                },
                deadline: std::time::Instant::now(),
            });
            state.compose(cols, rows).unwrap();
            let rect = state.hits.notification_toast;
            assert_graphics_cover(&mut state, rect, cols, rows);
        }
        for position in [
            Clipboard::TopLeft,
            Clipboard::TopCenter,
            Clipboard::TopRight,
            Clipboard::BottomLeft,
            Clipboard::BottomCenter,
            Clipboard::BottomRight,
        ] {
            let mut state =
                ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
            state.sidebar_collapsed = true;
            state.config.clipboard_toast_position = position;
            state.set_snapshot(Box::new(snapshot()));
            state.set_pane_surface(surface());
            state.copy_feedback = Some(crate::app::state::CopyFeedback {
                message: "copied".into(),
            });
            let layout = state.layout(cols, rows);
            let area = if layout.mobile_header.is_empty() {
                layout.pane_surface
            } else {
                Rect::new(0, 0, cols, rows)
            };
            let mut buffer = Buffer::empty(Rect::new(0, 0, cols, rows));
            let rect = crate::ui::render_copy_feedback_buffer(
                &mut buffer,
                area,
                state.copy_feedback.as_ref().unwrap(),
                0,
                position,
                &state.config.palette,
            );
            assert_graphics_cover(&mut state, rect, cols, rows);
        }
    }
}

#[test]
fn endpoint_notice_and_multiline_diagnostic_cover_the_actual_rows() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.visible_endpoint_notice = Some(ClientVisibleEndpointNotice {
        key: ClientEndpointNoticeKey {
            boot_id: "boot-1".into(),
            kind: ClientEndpointNoticeKind::Rejected,
            code: "test".into(),
        },
        title: "error".into(),
        body: "body".into(),
        deadline: std::time::Instant::now(),
    });
    state.compose(106, 20).unwrap();
    let rect = state.hits.notification_toast;
    assert_graphics_cover(&mut state, rect, 106, 20);
    state.visible_endpoint_notice = None;
    state.config_diagnostic = Some("first\nsecond warning".into());
    assert_graphics_cover(&mut state, Rect::new(90, 1, 16, 1), 106, 20);
}

#[test]
fn every_dialog_and_menu_occludes_its_panel_not_the_whole_screen() {
    let palette = ClientShellConfig::from_config(&Config::default()).palette;
    let overlays = vec![
        ClientShellOverlay::Onboarding,
        ClientShellOverlay::ProductAnnouncement(crate::app::state::ProductAnnouncementState {
            version: "1".into(),
            id: "test".into(),
            title: "news".into(),
            body: "body".into(),
            scroll: 0,
            preview: false,
        }),
        ClientShellOverlay::ReleaseNotes(crate::app::state::ReleaseNotesState {
            version: "1".into(),
            body: "body".into(),
            scroll: 0,
            preview: false,
        }),
        ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "rename",
            input: "name".into(),
            target: ClientRenameTarget::Pane {
                pane_id: "pane_1".into(),
            },
        }),
        ClientShellOverlay::ConfirmClose(ClientConfirmCloseOverlay {
            workspace_id: "ws_1".into(),
            tab_target: None,
            title: "close".into(),
            detail: "confirm".into(),
        }),
        ClientShellOverlay::Help(ClientHelpOverlay {
            query: TextEditor::default(),
            search_focused: false,
            scroll: 0,
        }),
        ClientShellOverlay::Navigator(ClientNavigatorOverlay {
            query: TextEditor::default(),
            search_focused: false,
            selected: None,
            scroll: 0,
            filter: None,
        }),
        ClientShellOverlay::WorktreeCreate(ClientWorktreeCreateOverlay {
            source_workspace_id: "ws_1".into(),
            repo_name: "repo".into(),
            branch: "branch".into(),
            checkout_path: "path".into(),
            error: None,
            creating: false,
        }),
        ClientShellOverlay::WorktreeOpen(ClientWorktreeOpenOverlay {
            source_workspace_id: "ws_1".into(),
            entries: Vec::new(),
            selected: 0,
            query: TextEditor::default(),
            search_focused: false,
            error: None,
            opening: false,
        }),
        ClientShellOverlay::WorktreeRemove(ClientWorktreeRemoveOverlay {
            workspace_id: "ws_1".into(),
            path: "path".into(),
            error: None,
            removing: false,
            force_confirmation: false,
        }),
        ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Tab {
                tab_id: "tab_1".into(),
                workspace_id: "ws_1".into(),
            },
            x: 35,
            y: 8,
            highlighted: 0,
        }),
        ClientShellOverlay::GlobalMenu(ClientGlobalMenuOverlay { highlighted: 0 }),
        ClientShellOverlay::Settings(ClientSettingsOverlay {
            section: ClientSettingsSection::Theme,
            selected: 0,
            original_theme_name: String::new(),
            original_palette: palette,
            integrations: Vec::new(),
            integration_messages: Vec::new(),
            loading_integrations: false,
            installing_integrations: false,
        }),
    ];
    for overlay in overlays {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        state.set_snapshot(Box::new(snapshot()));
        state.set_pane_surface(surface());
        state.compose(106, 40).unwrap();
        let layout = state.layout(106, 40);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 106, 40));
        let snapshot = state.snapshot.as_deref().unwrap();
        let rendered = match &overlay {
            ClientShellOverlay::ContextMenu(menu) => {
                render::render_context_menu(&mut buffer, menu, &state.config.palette)
            }
            ClientShellOverlay::GlobalMenu(menu) => render::render_global_menu(
                &mut buffer,
                state.hits.global_launcher,
                menu,
                snapshot,
                &state.config.palette,
            ),
            _ => render::render_client_overlay(
                &mut buffer,
                &overlay,
                snapshot,
                &state.endpoints,
                &state.active_endpoint_id,
                &state.config.keybinds,
                &state.config.palette,
            ),
        }
        .unwrap();
        let area = rendered.area;
        assert!(!area.is_empty(), "{overlay:?}");
        let covered = area.intersection(layout.pane_surface);
        let outside = (layout.pane_surface.y..layout.pane_surface.bottom())
            .find_map(|y| {
                (layout.pane_surface.x..layout.pane_surface.right())
                    .find(|&x| !area.contains((x, y).into()))
                    .map(|x| (x, y))
            })
            .unwrap();
        let mut surface = surface();
        add_main_image(&mut surface, layout, outside, 1);
        let border = (!covered.is_empty()).then(|| (covered.right() - 1, covered.bottom() - 1));
        if let Some(border) = border {
            add_main_image(&mut surface, layout, border, 2);
        }
        state.set_pane_surface(surface);
        state.compose(106, 40).unwrap();
        state.overlay = Some(overlay);
        let frame = state.compose(106, 40).unwrap();
        assert!(
            is_placed(&frame.graphics.clone().into_inline_bytes(), outside),
            "{:?}",
            state.overlay
        );
        if let Some(border) = border {
            assert!(
                !is_placed(&frame.graphics.clone().into_inline_bytes(), border),
                "{:?}",
                state.overlay
            );
            assert!(
                String::from_utf8_lossy(&frame.graphics.clone().into_inline_bytes())
                    .contains("a=d,d=i")
            );
        }
        state.overlay = None;
        let restored = state.compose(106, 40).unwrap();
        assert!(is_placed(
            &restored.graphics.clone().into_inline_bytes(),
            outside
        ));
        if let Some(border) = border {
            assert!(is_placed(
                &restored.graphics.clone().into_inline_bytes(),
                border
            ));
        }
        assert!(
            !String::from_utf8_lossy(&restored.graphics.clone().into_inline_bytes())
                .contains("a=t")
        );
    }
}

#[test]
fn selection_copy_cursor_and_search_hide_only_the_highlighted_images() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let layout = state.layout(106, 20);
    let mut surface = surface();
    surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 0,
        viewport_rows: 2,
    });
    let points = [(0, 0), (1, 1), (2, 0), (3, 0)]
        .map(|(x, y)| (layout.pane_surface.x + x, layout.pane_surface.y + y));
    for (id, point) in points.iter().enumerate() {
        add_main_image(&mut surface, layout, *point, id as u32 + 1);
    }
    state.set_pane_surface(surface);
    state.compose(106, 20).unwrap();
    assert!(state.enter_copy_mode(&mut ClientShellInput::default()));
    state.selection = Some(crate::selection::Selection::absolute_range(
        "pane_1".into(),
        (0, 0),
        (0, 0),
    ));
    state
        .copy_mode
        .as_mut()
        .unwrap()
        .search_matches
        .push(crate::api::schema::PaneTextRange {
            start: crate::api::schema::PaneTextPoint { row: 0, col: 2 },
            end: crate::api::schema::PaneTextPoint { row: 0, col: 2 },
        });
    let frame = state.compose(106, 20).unwrap();
    for point in &points[..3] {
        assert!(
            !is_placed(&frame.graphics.clone().into_inline_bytes(), *point),
            "{point:?}"
        );
    }
    assert!(is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        points[3]
    ));
    state.mode = ClientShellMode::Terminal;
    state.copy_mode = None;
    state.selection = None;
    let frame = state.compose(106, 20).unwrap();
    for point in points {
        assert!(is_placed(
            &frame.graphics.clone().into_inline_bytes(),
            point
        ));
    }
    assert!(!String::from_utf8_lossy(&frame.graphics.clone().into_inline_bytes()).contains("a=t"));
}

#[test]
fn mobile_switcher_still_hides_the_entire_underlying_surface() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let layout = state.layout(40, 24);
    let point = (layout.pane_surface.x, layout.pane_surface.y);
    let mut surface = surface();
    add_main_image(&mut surface, layout, point, 1);
    state.set_pane_surface(surface);
    let frame = state.compose(40, 24).unwrap();
    assert!(is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        point
    ));
    state.mode = ClientShellMode::Navigate;
    let frame = state.compose(40, 24).unwrap();
    assert!(!is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        point
    ));
    assert!(
        String::from_utf8_lossy(&frame.graphics.clone().into_inline_bytes()).contains("a=d,d=i")
    );
    state.mode = ClientShellMode::Terminal;
    let frame = state.compose(40, 24).unwrap();
    assert!(is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        point
    ));
    assert!(!String::from_utf8_lossy(&frame.graphics.clone().into_inline_bytes()).contains("a=t"));
}

#[test]
fn popup_terminal_keeps_own_graphics_and_hides_only_background_overlap() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface_with_popup());
    state.compose(106, 20).unwrap();
    let popup = state.hits.popup.clone().unwrap();
    let layout = state.layout(106, 20);
    let outside = (layout.pane_surface.x, layout.pane_surface.y);
    let border = (popup.rect.x, popup.rect.y);
    let inside = (popup.inner_rect.x, popup.inner_rect.y);
    let mut surface = surface_with_popup();
    add_main_image(&mut surface, layout, outside, 1);
    add_main_image(&mut surface, layout, border, 2);
    let (asset, placement) = image(
        SurfaceGraphicsTarget::Popup {
            terminal_id: "terminal-popup".into(),
        },
        0,
        0,
        3,
    );
    surface.graphics.assets.push(asset);
    surface.graphics.placements.push(placement);
    state.set_pane_surface(surface);
    let frame = state.compose(106, 20).unwrap();
    assert!(is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        outside
    ));
    assert!(!is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        border
    ));
    assert!(is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        inside
    ));
    state.overlay = Some(ClientShellOverlay::Onboarding);
    let frame = state.compose(106, 20).unwrap();
    assert!(is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        outside
    ));
    assert!(!is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        inside
    ));
    state.overlay = None;
    let frame = state.compose(106, 20).unwrap();
    assert!(is_placed(
        &frame.graphics.clone().into_inline_bytes(),
        inside
    ));
    assert!(!String::from_utf8_lossy(&frame.graphics.clone().into_inline_bytes()).contains("a=t"));
}

fn tmux_graphics_state() -> ClientShellState {
    let mut state = ClientShellState::new(
        ClientShellConfig::from_config(&Config::default()).with_graphics_transport(
            crate::kitty_graphics::HostGraphicsTransport::TmuxPlaceholders,
        ),
    );
    state.set_snapshot(Box::new(snapshot()));
    let layout = state.layout(106, 20);
    let mut pane_surface = surface();
    let origin = (
        layout.pane_surface.x + pane_surface.panes[0].inner_rect.x,
        layout.pane_surface.y + pane_surface.panes[0].inner_rect.y,
    );
    add_main_image(&mut pane_surface, layout, origin, 1);
    state.set_pane_surface(pane_surface);
    state
}

fn placeholder_cells_in(frame: &FrameData) -> Vec<(u16, u16, u32, u32)> {
    frame
        .cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.symbol.starts_with('\u{10EEEE}'))
        .map(|(index, cell)| {
            (
                (index % usize::from(frame.width)) as u16,
                (index / usize::from(frame.width)) as u16,
                cell.fg,
                cell.underline_color,
            )
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn client_shell_paints_placeholder_cells_under_the_tmux_transport() {
    let mut state = tmux_graphics_state();

    let frame = state.compose(106, 20).expect("visible graphics frame");
    let graphics = String::from_utf8(frame.graphics.clone().into_inline_bytes()).unwrap();
    assert!(graphics.contains("a=t,t=d"), "{graphics:?}");
    assert!(graphics.contains("a=p,U=1,"), "{graphics:?}");
    assert!(!graphics.contains("\x1b["), "{graphics:?}");
    let pane = state.hits.panes[0].clone();
    let cells = placeholder_cells_in(&frame);
    assert_eq!(cells.len(), 1, "{cells:?}");
    assert_eq!(
        (cells[0].0, cells[0].1),
        (pane.inner_rect.x, pane.inner_rect.y)
    );
    // Terminal images take native ids above 24 bits: the low bits ride the
    // foreground color and the high byte a third diacritic.
    let image_id = crate::kitty_graphics::surface::native_host_image_id(
        state.graphics_scope(),
        &image(
            SurfaceGraphicsTarget::Pane {
                pane_id: "pane_1".into(),
            },
            0,
            0,
            1,
        )
        .0
        .key,
    );
    assert_eq!(
        cells[0].2,
        crate::protocol::color_to_u32(ratatui::style::Color::Rgb(
            (image_id >> 16) as u8,
            (image_id >> 8) as u8,
            image_id as u8,
        )),
        "image id rides the foreground"
    );
    let index = usize::from(cells[0].1) * usize::from(frame.width) + usize::from(cells[0].0);
    assert_eq!(
        frame.cells[index].symbol.chars().nth(3),
        crate::ghostty::kitty_placeholder_diacritic(image_id >> 24)
    );
    assert_ne!(cells[0].3, 0, "placement id rides the underline color");

    // An unchanged frame sends nothing new but keeps its cells.
    let again = state.compose(106, 20).expect("second frame");
    assert!(again.graphics.is_empty(), "{:?}", again.graphics);
    assert_eq!(placeholder_cells_in(&again), cells);
}

#[test]
fn regaining_focus_under_the_tmux_transport_retransmits_graphics() {
    let mut state = tmux_graphics_state();
    state.compose(106, 20).expect("first frame");
    assert!(state
        .compose(106, 20)
        .expect("settled frame")
        .graphics
        .is_empty());

    let outcome = state.handle_input_bytes(b"\x1b[I");
    assert!(outcome.repaint, "focus regain must recompose to re-upload");
    let frame = state.compose(106, 20).expect("focus frame");
    let graphics = String::from_utf8(frame.graphics.clone().into_inline_bytes()).unwrap();
    assert!(graphics.contains("a=d,d=I"), "{graphics:?}");
    assert!(graphics.contains("a=t,t=d"), "{graphics:?}");
    assert!(graphics.contains("a=p,U=1,"), "{graphics:?}");
    assert_eq!(placeholder_cells_in(&frame).len(), 1);
}

#[test]
fn regaining_focus_under_the_direct_transport_sends_no_graphics_reupload() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let layout = state.layout(106, 20);
    let mut pane_surface = surface();
    let origin = (
        layout.pane_surface.x + pane_surface.panes[0].inner_rect.x,
        layout.pane_surface.y + pane_surface.panes[0].inner_rect.y,
    );
    add_main_image(&mut pane_surface, layout, origin, 1);
    state.set_pane_surface(pane_surface);
    let first = state.compose(106, 20).expect("first frame");
    assert!(placeholder_cells_in(&first).is_empty());

    let _ = state.handle_input_bytes(b"\x1b[I");
    let frame = state.compose(106, 20).expect("focus frame");
    let graphics = String::from_utf8(frame.graphics.clone().into_inline_bytes()).unwrap();
    assert!(!graphics.contains("a=t,t=d"), "{graphics:?}");
    assert!(!graphics.contains("a=d,d=I"), "{graphics:?}");
    assert!(
        graphics.contains("a=p,i="),
        "direct placements replay every frame: {graphics:?}"
    );
    assert!(graphics.contains("C=1"), "{graphics:?}");
}
