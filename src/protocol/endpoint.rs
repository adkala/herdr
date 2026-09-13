//! Stable endpoint compatibility contract for client-owned shells.
//!
//! The endpoint generation is intentionally independent from the private
//! binary protocol used by same-install CLI, direct-terminal, and handoff
//! paths. Generation 1 is the compatibility floor for Local, SSH, and Cloud
//! shell endpoints and must remain available indefinitely unless retired for a
//! security reason. New JSON fields must be optional or have serde defaults;
//! new enum values need an `Unknown` fallback. Unknown named controls are
//! optional and ignored unless negotiated as part of the core.

use serde::{Deserialize, Serialize};

use super::{ClientMessage, ClientShellSnapshot, ClientSurfaceSize, ServerMessage};

pub const ENDPOINT_PROTOCOL_GENERATION: u32 = 1;
pub const ENDPOINT_HELLO_KIND: &str = "endpoint.hello.v1";
pub const ENDPOINT_WELCOME_KIND: &str = "endpoint.welcome.v1";
pub const SNAPSHOT_CODEC_V1: &str = "shell.snapshot.v1";
pub const ENDPOINT_SNAPSHOT_KIND: &str = SNAPSHOT_CODEC_V1;
pub const SURFACE_CODEC_V1: &str = "shell.surface.v1";
/// `shell.surface.v1` plus a per-cell SGR 58 underline color, carried by the
/// appended `ServerMessage::PaneSurfaceV2` / `PaneSurfacePatchV2` variants.
/// Those tags only reach a shell whose hello negotiated this codec.
pub const SURFACE_CODEC_V2: &str = "shell.surface.v2";
pub const INPUT_CODEC_V1: &str = "shell.input.semantic.v1";
pub const BLOB_CODEC_V1: &str = "shell.blob.v1";
pub const SURFACE_INTEREST_CAPABILITY: &str = "surface_interest";
pub const PRESENTATION_EFFECTS_FENCE_CAPABILITY: &str = "presentation_effects_fence";
pub const PRESENTATION_EFFECTS_SYNC_KIND: &str = "endpoint.presentation.sync.v1";
pub const PRESENTATION_EFFECTS_READY_KIND: &str = "endpoint.presentation.ready.v1";
pub const HEALTH_CHECK_CAPABILITY: &str = "health_check";
pub const HEALTH_PING_KIND: &str = "endpoint.health.ping.v1";
pub const HEALTH_PONG_KIND: &str = "endpoint.health.pong.v1";
/// Client hello capability: the client shell relays OSC 52 clipboard read
/// queries to its outer terminal and answers with `HOST_CLIPBOARD_REPLY_KIND`.
/// Servers only forward `advanced.osc52_paste = "terminal"` queries to a
/// foreground shell that advertised it; other panes get an immediate empty
/// reply instead of waiting for a timeout.
pub const HOST_CLIPBOARD_QUERY_CAPABILITY: &str = "host_clipboard_query";
/// Server to client: query the outer terminal's clipboard with OSC 52.
/// `data` is empty and reserved.
pub const HOST_CLIPBOARD_QUERY_KIND: &str = "endpoint.clipboard.query.v1";
/// Client to server: the outer terminal's OSC 52 reply as [`HostClipboardReply`]
/// JSON. Malformed or oversized payloads decode as an empty reply.
pub const HOST_CLIPBOARD_REPLY_KIND: &str = "endpoint.clipboard.reply.v1";
/// Upper bound on a relayed clipboard reply payload (base64 text). Matches the
/// client raw-input framer's cap so a runaway terminal reply cannot grow
/// server-side buffers; larger payloads decode as an empty reply.
pub const MAX_HOST_CLIPBOARD_REPLY_BYTES: usize = 512 * 1024;

fn default_true() -> bool {
    true
}

/// Pane surface codec negotiated for one client-owned shell connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceCodec {
    /// Generation-1 floor: `ServerMessage::PaneSurface` / `PaneSurfacePatch`
    /// with the frozen `CellDataV1` layout.
    #[default]
    V1,
    /// `ServerMessage::PaneSurfaceV2` / `PaneSurfacePatchV2` with underline colors.
    V2,
}

impl SurfaceCodec {
    /// Codecs this build speaks, most capable first.
    pub const SUPPORTED: [SurfaceCodec; 2] = [SurfaceCodec::V2, SurfaceCodec::V1];

    pub const fn name(self) -> &'static str {
        match self {
            Self::V1 => SURFACE_CODEC_V1,
            Self::V2 => SURFACE_CODEC_V2,
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::SUPPORTED
            .into_iter()
            .find(|codec| codec.name() == name)
    }

    /// Picks the first codec in the client's preference order that this server
    /// speaks. `None` means the client offered nothing usable;
    /// `EndpointClientHello::supports_required_codecs` separately guarantees
    /// that v1 is offered.
    pub fn negotiate(offered: &[String]) -> Option<Self> {
        offered.iter().find_map(|name| Self::parse(name))
    }

    /// Names to advertise in a hello, most capable first.
    pub fn offered_names() -> Vec<String> {
        Self::SUPPORTED
            .into_iter()
            .map(|codec| codec.name().to_owned())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointClientHello {
    pub generation: u32,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
    pub surface_size: ClientSurfaceSize,
    pub pixel_mouse: bool,
    pub direct_graphics: bool,
    pub endpoint_keybindings: bool,
    pub mouse_capture: bool,
    #[serde(default = "default_true")]
    pub surface_active: bool,
    #[serde(default)]
    pub snapshot_codecs: Vec<String>,
    #[serde(default)]
    pub surface_codecs: Vec<String>,
    #[serde(default)]
    pub input_codecs: Vec<String>,
    #[serde(default)]
    pub blob_codecs: Vec<String>,
    /// Optional client features the server may use (for example
    /// `host_clipboard_query`). Unknown names are ignored; an absent list means
    /// none, so generation-1 hellos without it stay valid.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Payload of a `HOST_CLIPBOARD_REPLY_KIND` control: the outer terminal's OSC 52
/// answer as base64, empty when the terminal had no clipboard data.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HostClipboardReply {
    #[serde(default)]
    pub data: String,
}

/// Builds the server-to-client control asking the foreground shell to query its
/// outer terminal's clipboard with OSC 52.
pub fn host_clipboard_query_message() -> ServerMessage {
    ServerMessage::EndpointControl {
        kind: HOST_CLIPBOARD_QUERY_KIND.into(),
        data: String::new(),
    }
}

/// Builds the client-to-server control relaying an outer terminal's OSC 52 reply.
pub fn host_clipboard_reply_message(data: String) -> ClientMessage {
    let reply = HostClipboardReply { data };
    // A one-field struct of `String` cannot fail to serialize; fall back to an
    // empty reply rather than panicking if that ever changes.
    let data = serde_json::to_string(&reply).unwrap_or_else(|_| r#"{"data":""}"#.to_owned());
    ClientMessage::EndpointControl {
        kind: HOST_CLIPBOARD_REPLY_KIND.into(),
        data,
    }
}

/// Decodes a `HOST_CLIPBOARD_REPLY_KIND` payload into its base64 text. Invalid
/// JSON and payloads over [`MAX_HOST_CLIPBOARD_REPLY_BYTES`] become an empty
/// reply so the waiting pane still unblocks without pasting garbage.
pub fn decode_host_clipboard_reply(data: &str) -> String {
    // The JSON wrapper adds a fixed envelope around the base64 text; reject
    // clearly oversized bodies before parsing them.
    const JSON_ENVELOPE_SLACK: usize = 64;
    if data.len() > MAX_HOST_CLIPBOARD_REPLY_BYTES + JSON_ENVELOPE_SLACK {
        return String::new();
    }
    match serde_json::from_str::<HostClipboardReply>(data) {
        Ok(reply) if reply.data.len() <= MAX_HOST_CLIPBOARD_REPLY_BYTES => reply.data,
        Ok(_) | Err(_) => String::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointHandshakeError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointServerWelcome {
    pub generation: u32,
    pub server_version: String,
    pub snapshot_codec: String,
    pub surface_codec: String,
    pub input_codec: String,
    pub blob_codec: String,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<EndpointHandshakeError>,
}

pub fn snapshot_message(snapshot: &ClientShellSnapshot) -> serde_json::Result<ServerMessage> {
    Ok(ServerMessage::EndpointControl {
        kind: ENDPOINT_SNAPSHOT_KIND.into(),
        data: serde_json::to_string(snapshot)?,
    })
}

impl EndpointClientHello {
    pub fn supports_required_codecs(&self) -> bool {
        self.snapshot_codecs
            .iter()
            .any(|codec| codec == SNAPSHOT_CODEC_V1)
            && self
                .surface_codecs
                .iter()
                .any(|codec| codec == SURFACE_CODEC_V1)
            && self
                .input_codecs
                .iter()
                .any(|codec| codec == INPUT_CODEC_V1)
            && self.blob_codecs.iter().any(|codec| codec == BLOB_CODEC_V1)
    }

    /// Whether the client shell relays OSC 52 clipboard queries to its terminal.
    pub fn supports_host_clipboard_query(&self) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability == HOST_CLIPBOARD_QUERY_CAPABILITY)
    }
}

impl EndpointServerWelcome {
    pub fn compatible(methods: Vec<String>, surface_codec: SurfaceCodec) -> Self {
        Self {
            generation: ENDPOINT_PROTOCOL_GENERATION,
            server_version: crate::build_info::version(),
            snapshot_codec: SNAPSHOT_CODEC_V1.into(),
            surface_codec: surface_codec.name().into(),
            input_codec: INPUT_CODEC_V1.into(),
            blob_codec: BLOB_CODEC_V1.into(),
            methods,
            capabilities: vec![
                SURFACE_INTEREST_CAPABILITY.into(),
                PRESENTATION_EFFECTS_FENCE_CAPABILITY.into(),
                HEALTH_CHECK_CAPABILITY.into(),
            ],
            error: None,
        }
    }

    pub fn incompatible(code: &str, message: impl Into<String>) -> Self {
        Self {
            generation: ENDPOINT_PROTOCOL_GENERATION,
            server_version: crate::build_info::version(),
            snapshot_codec: SNAPSHOT_CODEC_V1.into(),
            surface_codec: SURFACE_CODEC_V1.into(),
            input_codec: INPUT_CODEC_V1.into(),
            blob_codec: BLOB_CODEC_V1.into(),
            methods: Vec::new(),
            capabilities: Vec::new(),
            error: Some(EndpointHandshakeError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> EndpointClientHello {
        EndpointClientHello {
            generation: ENDPOINT_PROTOCOL_GENERATION,
            cell_width_px: 8,
            cell_height_px: 16,
            surface_size: ClientSurfaceSize { cols: 80, rows: 24 },
            pixel_mouse: true,
            direct_graphics: false,
            endpoint_keybindings: false,
            mouse_capture: true,
            surface_active: true,
            snapshot_codecs: vec![SNAPSHOT_CODEC_V1.into()],
            surface_codecs: vec![SURFACE_CODEC_V1.into()],
            input_codecs: vec![INPUT_CODEC_V1.into()],
            blob_codecs: vec![BLOB_CODEC_V1.into()],
            capabilities: Vec::new(),
        }
    }

    fn snapshot() -> ClientShellSnapshot {
        ClientShellSnapshot {
            boot_id: "boot".into(),
            revision: 1,
            config_diagnostic: None,
            product_announcement: None,
            update_available: None,
            update_install_command: "herdr update".into(),
            server_keybindings_toml: None,
            latest_release_notes_available: false,
            integration_updates_available: false,
            worktree_directory: String::new(),
            release_notes: None,
            focused_workspace_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            tab_bar_right: Vec::new(),
            tab_bar_right_separator: String::new(),
            agent_view_label: None,
            agent_order: Vec::new(),
            workspaces: Vec::new(),
            tabs: Vec::new(),
            panes: Vec::new(),
            agents: Vec::new(),
            commands: Vec::new(),
        }
    }

    #[test]
    fn hello_ignores_future_named_fields() {
        let mut value = serde_json::to_value(hello()).unwrap();
        value["future_feature"] = serde_json::json!({"enabled": true});
        let decoded: EndpointClientHello = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, hello());
    }

    #[test]
    fn frozen_generation_one_handshake_decodes() {
        let hello: EndpointClientHello = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/endpoint-hello-v1.json"
        )))
        .unwrap();
        assert!(hello.supports_required_codecs());

        let welcome: EndpointServerWelcome = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/endpoint-welcome-v1.json"
        )))
        .unwrap();
        assert_eq!(welcome.generation, ENDPOINT_PROTOCOL_GENERATION);
        assert_eq!(welcome.snapshot_codec, SNAPSHOT_CODEC_V1);
        assert_eq!(welcome.surface_codec, SURFACE_CODEC_V1);
        assert_eq!(welcome.input_codec, INPUT_CODEC_V1);
        assert_eq!(welcome.blob_codec, BLOB_CODEC_V1);
        assert!(welcome.capabilities.is_empty());
    }

    #[test]
    fn frozen_generation_one_snapshot_decodes() {
        let snapshot: ClientShellSnapshot = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/endpoint-snapshot-v1.json"
        )))
        .unwrap();
        assert_eq!(snapshot.boot_id, "boot-v1");
        assert_eq!(
            snapshot.workspaces[0].agent_status,
            crate::api::schema::AgentStatus::Unknown
        );
    }

    #[test]
    fn snapshot_message_uses_named_json_control() {
        let snapshot = snapshot();
        let ServerMessage::EndpointControl { kind, data } = snapshot_message(&snapshot).unwrap()
        else {
            panic!("snapshot should use endpoint control");
        };
        assert_eq!(kind, ENDPOINT_SNAPSHOT_KIND);
        let decoded: ClientShellSnapshot = serde_json::from_str(&data).unwrap();
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn snapshot_json_tolerates_future_fields_and_command_actions() {
        let mut snapshot = match snapshot_message(&snapshot()).unwrap() {
            ServerMessage::EndpointControl { data, .. } => {
                serde_json::from_str::<serde_json::Value>(&data).unwrap()
            }
            _ => unreachable!(),
        };
        snapshot["future_projection"] = serde_json::json!({"enabled": true});
        snapshot["commands"] = serde_json::json!([{
            "command_id": "future",
            "binding_label": "x",
            "binding_labels": ["x"],
            "action": "FutureAction",
            "description": null
        }]);

        let decoded: ClientShellSnapshot = serde_json::from_value(snapshot).unwrap();
        assert_eq!(
            decoded.commands[0].action,
            crate::protocol::ClientShellCommandAction::Unknown
        );
    }

    #[test]
    fn hello_without_capabilities_decodes_with_none() {
        // The frozen generation-1 hello predates the capability list.
        let hello: EndpointClientHello = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/endpoint-hello-v1.json"
        )))
        .unwrap();
        assert!(hello.capabilities.is_empty());
        assert!(!hello.supports_host_clipboard_query());

        let mut value = serde_json::to_value(hello).unwrap();
        value["capabilities"] = serde_json::json!(["future_feature", "host_clipboard_query"]);
        let decoded: EndpointClientHello = serde_json::from_value(value).unwrap();
        assert!(decoded.supports_host_clipboard_query());
    }

    #[test]
    fn host_clipboard_reply_round_trips_as_named_control() {
        let ClientMessage::EndpointControl { kind, data } =
            host_clipboard_reply_message("aGVsbG8=".into())
        else {
            panic!("reply should use endpoint control");
        };
        assert_eq!(kind, HOST_CLIPBOARD_REPLY_KIND);
        assert_eq!(data, r#"{"data":"aGVsbG8="}"#);
        assert_eq!(decode_host_clipboard_reply(&data), "aGVsbG8=");

        let ServerMessage::EndpointControl { kind, data } = host_clipboard_query_message() else {
            panic!("query should use endpoint control");
        };
        assert_eq!(kind, HOST_CLIPBOARD_QUERY_KIND);
        assert!(data.is_empty());
    }

    #[test]
    fn host_clipboard_reply_decode_tolerates_bad_payloads() {
        assert_eq!(decode_host_clipboard_reply("{}"), "");
        assert_eq!(decode_host_clipboard_reply("not json"), "");
        assert_eq!(decode_host_clipboard_reply(r#"{"data":"","future":1}"#), "");
        assert_eq!(
            decode_host_clipboard_reply(r#"{"data":"YQ==","future":true}"#),
            "YQ=="
        );

        let oversized = "A".repeat(MAX_HOST_CLIPBOARD_REPLY_BYTES + 1);
        let payload = serde_json::to_string(&HostClipboardReply { data: oversized }).unwrap();
        assert_eq!(decode_host_clipboard_reply(&payload), "");

        let at_cap = "A".repeat(MAX_HOST_CLIPBOARD_REPLY_BYTES);
        let payload = serde_json::to_string(&HostClipboardReply {
            data: at_cap.clone(),
        })
        .unwrap();
        assert_eq!(decode_host_clipboard_reply(&payload), at_cap);
    }

    #[test]
    fn legacy_hello_defaults_to_an_active_surface() {
        let mut value = serde_json::to_value(hello()).unwrap();
        value.as_object_mut().unwrap().remove("surface_active");
        let decoded: EndpointClientHello = serde_json::from_value(value).unwrap();
        assert!(decoded.surface_active);
    }

    #[test]
    fn surface_codec_negotiation_follows_client_preference() {
        let offered = |names: &[&str]| {
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            SurfaceCodec::negotiate(&offered(&[SURFACE_CODEC_V1])),
            Some(SurfaceCodec::V1)
        );
        assert_eq!(
            SurfaceCodec::negotiate(&offered(&[SURFACE_CODEC_V2, SURFACE_CODEC_V1])),
            Some(SurfaceCodec::V2)
        );
        assert_eq!(
            SurfaceCodec::negotiate(&offered(&[
                "shell.surface.v9",
                SURFACE_CODEC_V1,
                SURFACE_CODEC_V2
            ])),
            Some(SurfaceCodec::V1),
            "unknown future codecs are skipped and the client's order wins"
        );
        assert_eq!(
            SurfaceCodec::negotiate(&offered(&["shell.surface.v9"])),
            None
        );
        assert_eq!(SurfaceCodec::default(), SurfaceCodec::V1);
        assert_eq!(
            SurfaceCodec::offered_names(),
            vec![SURFACE_CODEC_V2.to_owned(), SURFACE_CODEC_V1.to_owned()]
        );

        let welcome = EndpointServerWelcome::compatible(Vec::new(), SurfaceCodec::V2);
        assert_eq!(welcome.surface_codec, SURFACE_CODEC_V2);
        let welcome = EndpointServerWelcome::compatible(Vec::new(), SurfaceCodec::V1);
        assert_eq!(welcome.surface_codec, SURFACE_CODEC_V1);
    }

    #[test]
    fn compatible_server_advertises_endpoint_lifecycle_capabilities() {
        let welcome = EndpointServerWelcome::compatible(Vec::new(), SurfaceCodec::V1);
        assert_eq!(
            welcome.capabilities,
            vec![
                SURFACE_INTEREST_CAPABILITY.to_string(),
                PRESENTATION_EFFECTS_FENCE_CAPABILITY.to_string(),
                HEALTH_CHECK_CAPABILITY.to_string(),
            ]
        );
    }

    #[test]
    fn required_codecs_are_explicit() {
        let mut value = hello();
        assert!(value.supports_required_codecs());
        value.snapshot_codecs.clear();
        assert!(!value.supports_required_codecs());

        let mut value = hello();
        value.surface_codecs.clear();
        assert!(!value.supports_required_codecs());

        let mut value = hello();
        value.input_codecs.clear();
        assert!(!value.supports_required_codecs());

        let mut value = hello();
        value.blob_codecs.clear();
        assert!(!value.supports_required_codecs());
    }

    #[test]
    fn welcome_ignores_future_named_fields() {
        let welcome =
            EndpointServerWelcome::compatible(vec!["pane.close".into()], SurfaceCodec::V1);
        let mut value = serde_json::to_value(&welcome).unwrap();
        value["future_service"] = serde_json::json!("v2");
        let decoded: EndpointServerWelcome = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, welcome);
    }
}
