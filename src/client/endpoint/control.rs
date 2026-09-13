use super::ClientEndpointId;

pub(crate) enum EndpointControlMessage {
    HealthPong,
    Snapshot(Box<crate::protocol::ClientShellSnapshot>),
    /// The server asks this shell to query its outer terminal's clipboard with
    /// OSC 52 (`advanced.osc52_paste = "terminal"`).
    HostClipboardQuery,
    Ignored,
}

pub(crate) fn decode_endpoint_control(
    kind: &str,
    data: &str,
) -> Result<EndpointControlMessage, String> {
    if kind == crate::protocol::endpoint::HEALTH_PONG_KIND {
        return Ok(EndpointControlMessage::HealthPong);
    }
    if kind == crate::protocol::endpoint::HOST_CLIPBOARD_QUERY_KIND {
        return Ok(EndpointControlMessage::HostClipboardQuery);
    }
    if kind == crate::protocol::endpoint::ENDPOINT_SNAPSHOT_KIND {
        let snapshot = serde_json::from_str(data)
            .map_err(|error| format!("invalid endpoint snapshot: {error}"))?;
        return Ok(EndpointControlMessage::Snapshot(Box::new(snapshot)));
    }
    if kind.starts_with("shell.snapshot.") {
        return Err(format!(
            "unsupported mandatory endpoint snapshot codec {kind:?}"
        ));
    }
    Ok(EndpointControlMessage::Ignored)
}

pub(crate) fn protocol_failure_is_fatal(endpoint_id: &ClientEndpointId) -> bool {
    endpoint_id.is_local()
}

/// Writes the OSC 52 clipboard read query to the host terminal. Its reply
/// arrives on stdin as `RawInputEvent::HostClipboardReply` and is relayed back
/// with `HOST_CLIPBOARD_REPLY_KIND`.
pub(crate) fn write_host_clipboard_query<W: std::io::Write>(out: &mut W) -> std::io::Result<()> {
    out.write_all(crate::selection::OSC52_CLIPBOARD_QUERY)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::endpoint::ProfileId;

    #[test]
    fn unknown_optional_controls_are_ignored() {
        assert!(matches!(
            decode_endpoint_control("future.optional", "not json").unwrap(),
            EndpointControlMessage::Ignored
        ));
    }

    #[test]
    fn host_clipboard_query_control_is_recognized_regardless_of_data() {
        for data in ["", "{}", "future"] {
            assert!(matches!(
                decode_endpoint_control(crate::protocol::endpoint::HOST_CLIPBOARD_QUERY_KIND, data)
                    .unwrap(),
                EndpointControlMessage::HostClipboardQuery
            ));
        }
    }

    #[test]
    fn host_clipboard_query_writes_the_osc52_read_request() {
        let mut out = Vec::new();
        write_host_clipboard_query(&mut out).unwrap();
        assert_eq!(out, b"\x1b]52;c;?\x07");
    }

    #[test]
    fn unknown_snapshot_codecs_are_rejected() {
        assert_eq!(
            decode_endpoint_control("shell.snapshot.v2", "{}")
                .err()
                .as_deref(),
            Some("unsupported mandatory endpoint snapshot codec \"shell.snapshot.v2\"")
        );
    }

    #[test]
    fn only_local_protocol_failures_end_the_client() {
        let remote =
            ClientEndpointId::Ssh(ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap());
        assert!(protocol_failure_is_fatal(&ClientEndpointId::Local));
        assert!(!protocol_failure_is_fatal(&remote));
    }
}
