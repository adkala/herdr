use super::*;

const RED: u32 = 0x02ff_0000;
const BLUE: u32 = 0x0200_00ff;

/// Every message of one client write, in order.
fn read_server_messages(bytes: Vec<u8>) -> Vec<ServerMessage> {
    let mut cursor = std::io::Cursor::new(bytes);
    let mut messages = Vec::new();
    while (cursor.position() as usize) < cursor.get_ref().len() {
        messages.push(protocol::read_message(&mut cursor, MAX_FRAME_SIZE).expect("decode message"));
    }
    messages
}

fn is_underline_control(message: &ServerMessage) -> bool {
    matches!(
        message,
        ServerMessage::EndpointControl { kind, .. }
            if kind == protocol::surface_underline::MESSAGE_KIND
    )
}

fn underline_colors(frame: &FrameData) -> Vec<u32> {
    frame
        .cells
        .iter()
        .map(|cell| cell.underline_color)
        .filter(|color| *color != 0)
        .collect()
}

#[tokio::test]
async fn underline_colors_reach_a_negotiated_shell_beside_surfaces_and_patches() {
    let (mut server, _control_rx, render_rx, pane_id) =
        retained_test_server_with_control(b"\x1b[4:3m\x1b[58:2::255:0:0mred\x1b[0m plain");
    server
        .clients
        .get_mut(&1)
        .expect("underline client")
        .render_state
        .enable_surface_underline_color(true);
    server.render_and_stream();

    // One write: the colors, then the surface they belong to.
    let messages = read_server_messages(render_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(messages.len(), 2);
    assert!(is_underline_control(&messages[0]));
    assert!(matches!(messages[1], ServerMessage::PaneSurface(_)));
    let mut decoder = protocol::surface_reuse::Decoder::default();
    let mut decoded = messages
        .into_iter()
        .map(|message| decoder.decode(message).expect("decode"))
        .collect::<Vec<_>>();
    let Some(ServerMessage::PaneSurface(mut shell)) = decoded.pop() else {
        panic!("expected the initial pane surface");
    };
    assert_eq!(underline_colors(&shell.frame), vec![RED; 3]);
    assert_eq!(
        shell.frame.cells,
        server.clients[&1]
            .render_state
            .last_pane_surface()
            .expect("committed surface")
            .frame
            .cells
    );

    // A retained row patch carries the colors of the cells it rewrites.
    write_shared_test_pane(
        &mut server,
        pane_id,
        b"\r\n\x1b[4m\x1b[58:2::0:0:255mblue\x1b[0m",
    );
    assert!(server.render_retained_pane_surface_and_stream(&HashSet::from([pane_id])));
    let messages = read_server_messages(render_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(messages.len(), 2);
    assert!(is_underline_control(&messages[0]));
    let mut decoded = messages
        .into_iter()
        .map(|message| decoder.decode(message).expect("decode"))
        .collect::<Vec<_>>();
    let Some(ServerMessage::PaneSurfacePatch(patch)) = decoded.pop() else {
        panic!("expected a pane patch");
    };
    protocol::surface_delta::apply_rows(&mut shell.frame.cells, shell.frame.width, &patch.rows);
    let committed = server.clients[&1]
        .render_state
        .last_pane_surface()
        .expect("committed surface");
    assert_eq!(shell.frame.cells, committed.frame.cells);
    let mut colors = underline_colors(&shell.frame);
    colors.sort_unstable();
    assert_eq!(colors, [vec![BLUE; 4], vec![RED; 3]].concat());

    // Output without colored underlines goes back to a single message.
    write_shared_test_pane(&mut server, pane_id, b"\r\nplain again");
    assert!(server.render_retained_pane_surface_and_stream(&HashSet::from([pane_id])));
    let messages = read_server_messages(render_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0], ServerMessage::PaneSurfacePatch(_)));
    shutdown_test_runtimes(&mut server);
}

#[tokio::test]
async fn underline_colors_are_not_sent_to_a_peer_that_did_not_negotiate_them() {
    let (mut server, _control_rx, render_rx, pane_id) =
        retained_test_server_with_control(b"\x1b[4:3m\x1b[58:2::255:0:0mred\x1b[0m plain");
    server.render_and_stream();
    let messages = read_server_messages(render_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(messages.len(), 1);
    let ServerMessage::PaneSurface(surface) = &messages[0] else {
        panic!("expected the initial pane surface");
    };
    assert!(underline_colors(&surface.frame).is_empty());

    write_shared_test_pane(
        &mut server,
        pane_id,
        b"\r\n\x1b[4m\x1b[58:2::0:0:255mblue\x1b[0m",
    );
    assert!(server.render_retained_pane_surface_and_stream(&HashSet::from([pane_id])));
    let messages = read_server_messages(render_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0], ServerMessage::PaneSurfacePatch(_)));
    shutdown_test_runtimes(&mut server);
}
