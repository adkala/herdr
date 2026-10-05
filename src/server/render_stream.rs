//! Virtual rendering helpers for headless client frame streaming.

use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
use ratatui::layout::{Position, Rect, Size};

use crate::app::state::AppState;
use crate::protocol::render_ansi::{BlitEncoder, EncodedBlit};
use crate::protocol::{
    CursorState, FrameData, PaneSurfaceFrame, PaneSurfacePatch, RenderEncoding, ServerMessage,
    SurfaceGraphicsAssetKey, SurfaceGraphicsScene, TerminalFrame,
};
use crate::terminal::TerminalRuntimeRegistry;

/// Per-client render baseline for the negotiated render encoding.
pub(crate) enum ClientRenderState {
    /// Semantic clients compare full frame data and skip identical frames.
    Semantic {
        last_surface: Option<Box<PaneSurfaceFrame>>,
        surface_revision: u64,
        surface_reuse: bool,
        surface_delta: bool,
        surface_scroll: bool,
        surface_underline_color: bool,
        recompute_pending: bool,
    },
    /// Terminal-ANSI clients keep a terminal diff encoder and sequence number.
    TerminalAnsi {
        blit_encoder: BlitEncoder,
        seq: u64,
        repaint_pending: bool,
    },
}

impl ClientRenderState {
    pub(crate) fn new(render_encoding: RenderEncoding) -> Self {
        match render_encoding {
            RenderEncoding::SemanticFrame => Self::Semantic {
                last_surface: None,
                surface_revision: 0,
                surface_reuse: false,
                surface_delta: false,
                surface_scroll: false,
                surface_underline_color: false,
                recompute_pending: false,
            },
            RenderEncoding::TerminalAnsi => Self::TerminalAnsi {
                blit_encoder: BlitEncoder::new(),
                seq: 0,
                repaint_pending: false,
            },
        }
    }

    pub(crate) fn enable_surface_reuse(&mut self, enabled: bool) {
        if let Self::Semantic { surface_reuse, .. } = self {
            *surface_reuse = enabled;
        }
    }

    pub(crate) fn enable_surface_delta(&mut self, enabled: bool) {
        if let Self::Semantic { surface_delta, .. } = self {
            *surface_delta = enabled;
        }
    }

    pub(crate) fn enable_surface_scroll(&mut self, enabled: bool) {
        if let Self::Semantic { surface_scroll, .. } = self {
            *surface_scroll = enabled;
        }
    }

    pub(crate) fn enable_surface_underline_color(&mut self, enabled: bool) {
        if let Self::Semantic {
            surface_underline_color,
            ..
        } = self
        {
            *surface_underline_color = enabled;
        }
    }

    pub(crate) fn request_recompute(&mut self) {
        if let Self::Semantic {
            surface_delta: true,
            recompute_pending,
            ..
        } = self
        {
            *recompute_pending = true;
        } else {
            self.request_repaint();
        }
    }

    pub(crate) fn requires_recompute(&self) -> bool {
        matches!(
            self,
            Self::Semantic {
                recompute_pending: true,
                ..
            }
        )
    }

    pub(crate) fn reset_baseline(&mut self) {
        match self {
            Self::Semantic { last_surface, .. } => *last_surface = None,
            Self::TerminalAnsi {
                blit_encoder,
                repaint_pending,
                ..
            } => {
                *blit_encoder = BlitEncoder::new();
                *repaint_pending = false;
            }
        }
    }

    pub(crate) fn request_repaint(&mut self) {
        match self {
            Self::Semantic { last_surface, .. } => *last_surface = None,
            Self::TerminalAnsi {
                repaint_pending, ..
            } => *repaint_pending = true,
        }
    }

    pub(crate) fn prepare_frame(&mut self, frame: FrameData) -> Option<PreparedRender> {
        match self {
            Self::Semantic { .. } => None,
            Self::TerminalAnsi {
                blit_encoder,
                seq,
                repaint_pending,
            } => {
                if !*repaint_pending && blit_encoder.is_current(&frame) {
                    crate::render_prof::event("prepare_frame.ansi.skip_current");
                    return None;
                }
                let mut encoded = blit_encoder.encode(&frame, *repaint_pending);
                crate::render_prof::event("prepare_frame.ansi.changed");
                crate::render_prof::counter("prepare_frame.ansi.bytes", encoded.bytes.len() as u64);
                if encoded.full {
                    crate::render_prof::event("prepare_frame.ansi.full");
                } else {
                    crate::render_prof::event("prepare_frame.ansi.partial");
                }
                insert_graphics_before_sync_end(&mut encoded.bytes, &frame.graphics);
                crate::render_prof::counter(
                    "prepare_frame.graphics.bytes",
                    frame.graphics.len() as u64,
                );
                Some(PreparedRender::TerminalAnsi {
                    message: ServerMessage::Terminal(TerminalFrame {
                        seq: *seq + 1,
                        width: frame.width,
                        height: frame.height,
                        full: encoded.full,
                        bytes: encoded.bytes.clone(),
                    }),
                    frame,
                    encoded: Some(encoded),
                })
            }
        }
    }

    pub(crate) fn last_pane_surface(&self) -> Option<&PaneSurfaceFrame> {
        match self {
            Self::Semantic { last_surface, .. } => last_surface.as_deref(),
            Self::TerminalAnsi { .. } => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn prepare_pane_surface(
        &mut self,
        surface: PaneSurfaceFrame,
    ) -> Option<PreparedRender> {
        self.prepare_pane_surface_with_file(surface, false)
    }

    pub(crate) fn prepare_pane_surface_with_file(
        &mut self,
        mut surface: PaneSurfaceFrame,
        has_file_upload: bool,
    ) -> Option<PreparedRender> {
        let Self::Semantic {
            last_surface,
            surface_revision,
            surface_reuse,
            surface_delta,
            surface_underline_color,
            recompute_pending,
            ..
        } = self
        else {
            return None;
        };
        if !has_file_upload
            && !*recompute_pending
            && surface.graphics.assets.is_empty()
            && last_surface.as_deref().is_some_and(|last| {
                last.projection_revision == surface.projection_revision
                    && last.frame == surface.frame
                    && last.panes == surface.panes
                    && last.splits == surface.splits
                    && last.popup == surface.popup
                    && last.graphics.placements == surface.graphics.placements
                    && last.graphics.retained_assets == surface.graphics.retained_assets
            })
        {
            return None;
        }
        surface.surface_revision = surface_revision.saturating_add(1);
        let assets = std::mem::take(&mut surface.graphics.assets);
        let queued_graphics_assets = assets.iter().map(|asset| asset.key.clone()).collect();
        let committed_surface = surface.clone();
        surface.graphics.assets = assets;
        let underline = (*surface_underline_color)
            .then(|| crate::protocol::surface_underline::surface_message(&surface))
            .flatten();
        let mut message = ServerMessage::PaneSurface(surface);
        let delta = (*surface_delta)
            .then_some(last_surface.as_deref())
            .flatten()
            .and_then(|last| {
                crate::protocol::surface_delta::message(last, &mut message)
                    .map_err(|error| tracing::warn!(%error, "failed to encode surface delta"))
                    .ok()
                    .flatten()
            });
        let reused = if let ServerMessage::PaneSurface(surface) = &mut message {
            (delta.is_none() && *surface_reuse)
                .then_some(last_surface.as_deref())
                .flatten()
                .filter(|last| {
                    last.boot_id == surface.boot_id
                        && last.frame == surface.frame
                        // Popup cells are not part of the reusable grid; keep their compact codec.
                        && surface.popup.is_none()
                        && surface.graphics.assets.is_empty()
                })
                .and_then(|last| {
                    crate::protocol::surface_reuse::message(last.surface_revision, surface)
                        .map_err(|error| tracing::warn!(%error, "failed to encode surface reuse"))
                        .ok()
                        .flatten()
                })
        } else {
            None
        };
        Some(PreparedRender::Semantic {
            message: delta.or(reused).unwrap_or(message),
            underline,
            committed_surface: Box::new(committed_surface),
            queued_graphics_assets,
        })
    }

    pub(crate) fn prepare_pane_surface_patch(
        &self,
        mut patch: PaneSurfacePatch,
    ) -> Option<PreparedRender> {
        let Self::Semantic {
            last_surface,
            surface_revision,
            surface_scroll,
            surface_underline_color,
            ..
        } = self
        else {
            return None;
        };
        if self.requires_recompute() {
            return None;
        }
        let last = last_surface.as_deref()?;
        if last.boot_id != patch.boot_id
            || last.projection_revision != patch.projection_revision
            || last.surface_revision != patch.base_surface_revision
        {
            return None;
        }
        let next_revision = surface_revision.saturating_add(1);
        patch.surface_revision = next_revision;
        let underline = (*surface_underline_color)
            .then(|| crate::protocol::surface_underline::patch_message(&patch))
            .flatten();
        let scrolled = (*surface_scroll)
            .then(|| crate::protocol::surface_scroll::message(last, &patch))
            .flatten();
        Some(match scrolled {
            Some(message) => PreparedRender::SemanticPatch {
                message,
                underline,
                encoded: Some(Box::new(patch)),
            },
            None => PreparedRender::SemanticPatch {
                message: ServerMessage::PaneSurfacePatch(patch),
                underline,
                encoded: None,
            },
        })
    }

    pub(crate) fn commit_sent_frame(&mut self, prepared: PreparedRender) {
        match (self, prepared) {
            (
                Self::Semantic {
                    last_surface,
                    surface_revision,
                    recompute_pending,
                    ..
                },
                PreparedRender::Semantic {
                    committed_surface, ..
                },
            ) => {
                *surface_revision = committed_surface.surface_revision;
                *last_surface = Some(committed_surface);
                *recompute_pending = false;
            }
            (
                Self::Semantic {
                    last_surface,
                    surface_revision,
                    ..
                },
                PreparedRender::SemanticPatch {
                    message, encoded, ..
                },
            ) => {
                let patch = match (encoded, message) {
                    (Some(patch), _) => *patch,
                    (None, ServerMessage::PaneSurfacePatch(patch)) => patch,
                    (None, _) => unreachable!("a plain semantic patch carries its pane patch"),
                };
                let surface = last_surface
                    .as_deref_mut()
                    .expect("prepared patch baseline");
                apply_pane_surface_patch(surface, &patch);
                *surface_revision = patch.surface_revision;
            }
            (
                Self::TerminalAnsi {
                    blit_encoder,
                    seq,
                    repaint_pending,
                },
                PreparedRender::TerminalAnsi {
                    frame,
                    encoded: Some(encoded),
                    ..
                },
            ) => {
                blit_encoder.commit(frame, encoded);
                *seq += 1;
                *repaint_pending = false;
            }
            _ => {}
        }
    }
}

// Planning validates all rows and pane IDs before any send. The server does not yield
// between planning and commit, so applying the accepted patch cannot fail partway through.
pub(super) fn apply_pane_surface_patch(surface: &mut PaneSurfaceFrame, patch: &PaneSurfacePatch) {
    debug_assert_eq!(surface.boot_id, patch.boot_id);
    debug_assert_eq!(surface.projection_revision, patch.projection_revision);
    debug_assert_eq!(surface.surface_revision, patch.base_surface_revision);
    for row in &patch.rows {
        let start = usize::from(row.y) * usize::from(surface.frame.width) + usize::from(row.x);
        surface.frame.cells[start..start + row.cells.len()].clone_from_slice(&row.cells);
    }
    for updated in &patch.panes {
        let pane = surface
            .panes
            .iter_mut()
            .find(|pane| pane.pane_id == updated.pane_id)
            .expect("planned patch pane");
        pane.clone_from(updated);
    }
    surface.frame.cursor.clone_from(&patch.cursor);
    surface.surface_revision = patch.surface_revision;
}

fn insert_graphics_before_sync_end(encoded: &mut Vec<u8>, graphics: &[u8]) {
    if graphics.is_empty() {
        return;
    }

    if let Some(sync_end) = crate::protocol::render_ansi::final_sync_output_end(encoded) {
        encoded.splice(sync_end..sync_end, graphics.iter().copied());
    } else {
        encoded.extend_from_slice(graphics);
    }
}

/// A prepared client render message plus any baseline state needed after send.
pub(crate) enum PreparedRender {
    Semantic {
        message: ServerMessage,
        /// Underline colors to write ahead of `message` in the same client write.
        underline: Option<ServerMessage>,
        committed_surface: Box<PaneSurfaceFrame>,
        queued_graphics_assets: Vec<SurfaceGraphicsAssetKey>,
    },
    SemanticPatch {
        message: ServerMessage,
        /// Underline colors to write ahead of `message` in the same client write.
        underline: Option<ServerMessage>,
        /// The pane patch a compact `message` encodes; `None` when `message` is that patch.
        encoded: Option<Box<PaneSurfacePatch>>,
    },
    TerminalAnsi {
        message: ServerMessage,
        frame: FrameData,
        encoded: Option<EncodedBlit>,
    },
}

impl PreparedRender {
    pub(crate) fn message(&self) -> &ServerMessage {
        match self {
            Self::Semantic { message, .. }
            | Self::SemanticPatch { message, .. }
            | Self::TerminalAnsi { message, .. } => message,
        }
    }

    /// The underline-color control that must reach the client immediately
    /// ahead of [`Self::message`], if the client asked for one and any cell of
    /// this update has a colored underline.
    pub(crate) fn underline(&self) -> Option<&ServerMessage> {
        match self {
            Self::Semantic { underline, .. } | Self::SemanticPatch { underline, .. } => {
                underline.as_ref()
            }
            Self::TerminalAnsi { .. } => None,
        }
    }

    /// Graphics metadata represented by this semantic update plus only the
    /// asset keys whose pixel payloads were queued. This is independent of the
    /// selected wire codec and avoids cloning asset byte vectors.
    pub(crate) fn queued_surface_graphics(
        &self,
    ) -> Option<(&SurfaceGraphicsScene, &[SurfaceGraphicsAssetKey])> {
        match self {
            Self::Semantic {
                committed_surface,
                queued_graphics_assets,
                ..
            } => Some((&committed_surface.graphics, queued_graphics_assets)),
            Self::SemanticPatch { .. } | Self::TerminalAnsi { .. } => None,
        }
    }

    pub(crate) fn has_queued_surface_assets(&self) -> bool {
        matches!(self, Self::Semantic { queued_graphics_assets, .. } if !queued_graphics_assets.is_empty())
    }

    /// Removes the largest inline payload from a full semantic surface while
    /// preserving placement metadata. Largest-first guarantees that a fitting
    /// smaller asset is not discarded behind an oversized one. Equal sizes use
    /// deterministic scene order. Encoded delta/reuse messages return `None`; callers
    /// can invalidate that baseline and retry as a full surface.
    pub(crate) fn pop_pane_surface_asset(&mut self) -> Option<SurfaceGraphicsAssetKey> {
        let Self::Semantic {
            message: ServerMessage::PaneSurface(surface),
            queued_graphics_assets,
            ..
        } = self
        else {
            return None;
        };
        let index = surface
            .graphics
            .assets
            .iter()
            .enumerate()
            .max_by_key(|(index, asset)| (asset.data.len(), *index))?
            .0;
        let asset = surface.graphics.assets.remove(index);
        let key = asset.key;
        if let Some(index) = queued_graphics_assets
            .iter()
            .position(|queued| *queued == key)
        {
            queued_graphics_assets.remove(index);
        }
        Some(key)
    }
}

struct CursorTrackingBackend {
    inner: TestBackend,
    rendered_cursor: Option<Position>,
}

impl CursorTrackingBackend {
    fn new(width: u16, height: u16) -> Self {
        Self {
            inner: TestBackend::new(width, height),
            rendered_cursor: None,
        }
    }

    fn buffer(&self) -> &ratatui::buffer::Buffer {
        self.inner.buffer()
    }

    fn rendered_cursor(&self) -> Option<CursorState> {
        self.rendered_cursor.map(|pos| CursorState {
            x: pos.x,
            y: pos.y,
            visible: true,
            shape: 0,
        })
    }
}

impl Backend for CursorTrackingBackend {
    type Error = std::convert::Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        self.inner.draw(content)
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()?;
        self.rendered_cursor = None;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let position = position.into();
        self.inner.set_cursor_position(position)?;
        self.rendered_cursor = Some(position);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

pub(crate) type RenderedTabSurface = (
    ratatui::buffer::Buffer,
    Option<CursorState>,
    Vec<((u16, u16), String, String)>,
    crate::ui::TabSurfaceLayout,
);

/// Renders only the active tab's pane surface at an origin-relative client viewport.
pub(crate) fn render_tab_surface_virtual(
    app_state: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    layout: crate::ui::TabSurfaceLayout,
    area: Rect,
) -> RenderedTabSurface {
    let surface = crate::ui::TabSurfaceView {
        target: layout.target,
        pane_infos: &layout.pane_infos,
        split_borders: &layout.split_borders,
    };
    let cursor = crate::ui::tab_surface_cursor(app_state, terminal_runtimes, surface);
    let hyperlinks = crate::ui::tab_surface_hyperlinks(app_state, terminal_runtimes, surface);

    let backend = CursorTrackingBackend::new(area.width, area.height);
    let mut terminal = ratatui::Terminal::new(backend).expect("TestBackend::new should never fail");
    terminal
        .draw(|frame| {
            crate::ui::render_tab_surface(app_state, terminal_runtimes, surface, frame);
        })
        .expect("render to TestBackend should never fail");

    (
        terminal.backend().buffer().clone(),
        cursor,
        hyperlinks,
        layout,
    )
}

/// Renders one server-owned terminal directly for `terminal attach` clients.
pub(crate) fn render_terminal_virtual(
    runtime: &crate::terminal::TerminalRuntime,
    area: Rect,
) -> (ratatui::buffer::Buffer, Option<CursorState>) {
    let suppress_cursor = runtime.synchronized_output_active();
    let backend = CursorTrackingBackend::new(area.width, area.height);
    let mut terminal = ratatui::Terminal::new(backend).expect("TestBackend::new should never fail");

    terminal
        .draw(|frame| {
            runtime.render(frame, area, true);
        })
        .expect("render to TestBackend should never fail");

    let buffer = terminal.backend().buffer().clone();
    let cursor = (!suppress_cursor)
        .then(|| runtime.cursor_state(area, true))
        .flatten()
        .map(|cursor| CursorState {
            x: cursor.x,
            y: cursor.y,
            visible: cursor.visible && !crate::ui::pane_is_scrolled_back(runtime),
            shape: cursor.shape,
        })
        .or_else(|| {
            (!suppress_cursor)
                .then(|| terminal.backend().rendered_cursor())
                .flatten()
        });

    (buffer, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ClientShellPopupSurface;

    fn popup_surface(content: &str) -> PaneSurfaceFrame {
        let pane = ratatui::buffer::Buffer::with_lines(["pane"]);
        let popup = ratatui::buffer::Buffer::with_lines([content]);
        PaneSurfaceFrame {
            boot_id: "boot-1".into(),
            projection_revision: 1,
            surface_revision: 1,
            frame: FrameData::from_ratatui_buffer_with_hyperlinks(&pane, None, &[]),
            panes: Vec::new(),
            splits: Vec::new(),
            popup: Some(Box::new(ClientShellPopupSurface {
                terminal_id: "popup-terminal".into(),
                title: "popup".into(),
                width: None,
                height: None,
                frame: FrameData::from_ratatui_buffer_with_hyperlinks(&popup, None, &[]),
                mouse_reporting: false,
                sgr_pixel_mouse: false,
                pixel_width: 0,
                pixel_height: 0,
            })),
            graphics: crate::protocol::SurfaceGraphicsScene::default(),
        }
    }

    #[test]
    fn surface_delta_recompute_preserves_wire_baseline_but_epoch_reset_drops_it() {
        for enabled in [false, true] {
            let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
            state.enable_surface_delta(enabled);
            let mut surface = popup_surface("popup");
            surface.popup = None;
            surface.frame = FrameData::from_ratatui_buffer(
                &ratatui::buffer::Buffer::empty(Rect::new(0, 0, 120, 40)),
                None,
            );
            let initial = state.prepare_pane_surface(surface.clone()).unwrap();
            state.commit_sent_frame(initial);
            state.request_recompute();
            assert_eq!(state.last_pane_surface().is_some(), enabled);
            assert_eq!(state.requires_recompute(), enabled);
            // A freshness request still emits a new revision when every cell is equal.
            let fresh = state.prepare_pane_surface(surface.clone()).unwrap();
            assert_eq!(
                matches!(fresh.message(), ServerMessage::EndpointControl { kind, .. }
                if kind == crate::protocol::surface_delta::MESSAGE_KIND),
                enabled
            );
            assert_eq!(
                state.requires_recompute(),
                enabled,
                "prepare must not commit"
            );
            state.commit_sent_frame(fresh);
            assert!(!state.requires_recompute());
            assert_eq!(state.last_pane_surface().unwrap().surface_revision, 2);
            state.request_repaint();
            assert!(state.last_pane_surface().is_none());
            let recovery = state.prepare_pane_surface(surface).unwrap();
            assert!(
                matches!(recovery.message(), ServerMessage::PaneSurface(frame) if frame.surface_revision == 3)
            );
        }
    }

    #[test]
    fn surface_reuse_preserves_projection_and_patch_baselines_without_resending_cells() {
        for enabled in [false, true] {
            let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
            state.enable_surface_reuse(enabled);
            let mut decoder = crate::protocol::surface_reuse::Decoder::default();
            let mut surface = popup_surface("popup");
            surface.popup = None;
            let buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 240, 100));
            surface.frame = FrameData::from_ratatui_buffer(&buffer, None);
            let initial = state.prepare_pane_surface(surface.clone()).unwrap();
            decoder.decode(initial.message().clone()).unwrap();
            state.commit_sent_frame(initial);

            surface.projection_revision += 1;
            let update = state.prepare_pane_surface(surface.clone()).unwrap();
            let mut bytes = Vec::new();
            crate::protocol::write_message(&mut bytes, update.message()).unwrap();
            if enabled {
                assert!(
                    matches!(update.message(), ServerMessage::EndpointControl { kind, .. }
                    if kind == crate::protocol::surface_reuse::MESSAGE_KIND)
                );
                assert!(
                    bytes.len() < 2000,
                    "metadata update was {} bytes",
                    bytes.len()
                );
            } else {
                assert!(matches!(update.message(), ServerMessage::PaneSurface(_)));
                assert!(bytes.len() > 100_000);
            }
            let ServerMessage::PaneSurface(decoded) =
                decoder.decode(update.message().clone()).unwrap()
            else {
                panic!("decoded full surface");
            };
            assert_eq!(decoded.frame, surface.frame);
            assert_eq!(decoded.projection_revision, surface.projection_revision);
            assert_eq!(decoded.surface_revision, 2);
            state.commit_sent_frame(update);

            let mut changed_cell = surface.frame.cells[0].clone();
            changed_cell.symbol = "x".into();
            let patch = state
                .prepare_pane_surface_patch(PaneSurfacePatch {
                    boot_id: surface.boot_id.clone(),
                    projection_revision: surface.projection_revision,
                    base_surface_revision: 2,
                    surface_revision: 0,
                    rows: vec![crate::protocol::PaneSurfacePatchRow {
                        x: 0,
                        y: 0,
                        cells: vec![changed_cell.clone()],
                    }],
                    panes: Vec::new(),
                    cursor: None,
                })
                .unwrap();
            decoder.decode(patch.message().clone()).unwrap();
            state.commit_sent_frame(patch);
            surface.frame.cells[0] = changed_cell;
            surface.projection_revision += 1;
            let update = state.prepare_pane_surface(surface.clone()).unwrap();
            let ServerMessage::PaneSurface(decoded) =
                decoder.decode(update.message().clone()).unwrap()
            else {
                panic!("decoded surface after patch");
            };
            assert_eq!(decoded.frame, surface.frame);
            assert_eq!(decoded.surface_revision, 4);
            state.commit_sent_frame(update);

            // A changed border or terminal cell must still reach the client.
            surface.frame.cells[0].symbol = "y".into();
            let changed = state.prepare_pane_surface(surface.clone()).unwrap();
            assert!(matches!(changed.message(), ServerMessage::PaneSurface(_)));
            let ServerMessage::PaneSurface(decoded) =
                decoder.decode(changed.message().clone()).unwrap()
            else {
                panic!("changed full surface");
            };
            assert_eq!(decoded.frame, surface.frame);
            state.commit_sent_frame(changed);

            state.request_repaint();
            assert!(matches!(
                state.prepare_pane_surface(surface).unwrap().message(),
                ServerMessage::PaneSurface(_)
            ));
        }
    }

    #[test]
    fn surface_reuse_keeps_popup_cells_on_the_binary_codec() {
        let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
        state.enable_surface_reuse(true);
        let mut surface = popup_surface("popup");
        let buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 400, 100));
        surface.popup.as_mut().unwrap().frame = FrameData::from_ratatui_buffer(&buffer, None);
        let initial = state.prepare_pane_surface(surface.clone()).unwrap();
        state.commit_sent_frame(initial);
        surface.projection_revision += 1;
        let update = state.prepare_pane_surface(surface).unwrap();
        assert!(matches!(update.message(), ServerMessage::PaneSurface(_)));
        let mut bytes = Vec::new();
        crate::protocol::write_message(&mut bytes, update.message()).unwrap();
        assert!(bytes.len() < crate::protocol::MAX_FRAME_SIZE);
    }

    #[test]
    fn surface_reuse_falls_back_when_json_metadata_exceeds_the_frame_limit() {
        let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
        state.enable_surface_reuse(true);
        let mut surface = popup_surface("popup");
        surface.popup = None;
        surface.frame.hyperlinks = vec!["\"".repeat(crate::protocol::MAX_FRAME_SIZE / 2)];
        let initial = state.prepare_pane_surface(surface.clone()).unwrap();
        state.commit_sent_frame(initial);
        surface.projection_revision += 1;
        let update = state.prepare_pane_surface(surface).unwrap();
        assert!(matches!(update.message(), ServerMessage::PaneSurface(_)));
        let mut bytes = Vec::new();
        crate::protocol::write_message(&mut bytes, update.message()).unwrap();
        assert!(bytes.len() < crate::protocol::MAX_FRAME_SIZE);
    }

    #[test]
    fn deferred_file_upload_keeps_identical_metadata_and_retries_without_committing() {
        for reuse in [false, true] {
            let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
            state.enable_surface_reuse(reuse);
            let surface = popup_surface("native");
            let first = state.prepare_pane_surface(surface.clone()).unwrap();
            state.commit_sent_frame(first);
            assert!(state.prepare_pane_surface(surface.clone()).is_none());
            let file = state
                .prepare_pane_surface_with_file(surface.clone(), true)
                .unwrap();
            let retry = state
                .prepare_pane_surface_with_file(surface.clone(), true)
                .unwrap();
            let config = bincode::config::standard();
            assert_eq!(
                bincode::serde::encode_to_vec(file.message(), config).unwrap(),
                bincode::serde::encode_to_vec(retry.message(), config).unwrap()
            );
            state.commit_sent_frame(retry);
            assert!(state.prepare_pane_surface(surface).is_none());
        }
    }

    #[test]
    fn popup_only_surface_changes_are_not_deduplicated() {
        let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
        let prepared = state
            .prepare_pane_surface(popup_surface("first"))
            .expect("initial surface");
        state.commit_sent_frame(prepared);

        assert!(state
            .prepare_pane_surface(popup_surface("second"))
            .is_some());
    }

    #[test]
    fn forced_full_surface_keeps_the_connection_revision_monotonic() {
        let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
        let prepared = state
            .prepare_pane_surface(popup_surface("first"))
            .expect("initial surface");
        state.commit_sent_frame(prepared);
        state.request_repaint();

        let prepared = state
            .prepare_pane_surface(popup_surface("replacement"))
            .expect("forced replacement surface");
        assert!(matches!(
            prepared.message(),
            ServerMessage::PaneSurface(surface) if surface.surface_revision == 2
        ));
        state.commit_sent_frame(prepared);
        assert_eq!(state.last_pane_surface().unwrap().surface_revision, 2);
    }

    const RED: u32 = 0x02ff_0000;
    const BLUE: u32 = 0x0200_00ff;
    const PANE: crate::protocol::SurfaceRect = crate::protocol::SurfaceRect {
        x: 2,
        y: 1,
        width: 20,
        height: 10,
    };

    /// A 30x12 surface whose one pane shows distinct lines, like a build log.
    fn underline_surface() -> PaneSurfaceFrame {
        let mut buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 30, 12));
        for row in 0..PANE.height {
            buffer.set_string(
                PANE.x,
                PANE.y + row,
                format!("line {row:>2} of output"),
                ratatui::style::Style::default(),
            );
        }
        let mut surface = popup_surface("popup");
        surface.popup = None;
        surface.frame = FrameData::from_ratatui_buffer(&buffer, None);
        surface.panes = vec![crate::protocol::PaneSurfacePane {
            pane_id: "w1:p1".into(),
            content_revision: 1,
            rect: crate::protocol::SurfaceRect {
                x: 1,
                y: 0,
                width: 22,
                height: 12,
            },
            inner_rect: PANE,
            scrollbar_rect: None,
            scroll: None,
            focused: true,
            mouse_reporting: false,
            sgr_pixel_mouse: false,
            alternate_screen_active: false,
            pixel_width: 0,
            pixel_height: 0,
        }];
        surface
    }

    fn pane_cell(frame: &mut FrameData, row: u16, col: u16) -> &mut crate::protocol::CellData {
        let index =
            usize::from(PANE.y + row) * usize::from(frame.width) + usize::from(PANE.x + col);
        &mut frame.cells[index]
    }

    /// Writes a prepared update the way the server does, underline colors
    /// first and both through the wire codec, and returns what a client's
    /// decoder hands on.
    fn deliver(
        decoder: &mut crate::protocol::surface_reuse::Decoder,
        prepared: &PreparedRender,
    ) -> ServerMessage {
        let mut bytes = Vec::new();
        if let Some(underline) = prepared.underline() {
            crate::protocol::write_message(&mut bytes, underline).unwrap();
        }
        crate::protocol::write_message(&mut bytes, prepared.message()).unwrap();
        let mut reader = bytes.as_slice();
        let mut decoded = None;
        while !reader.is_empty() {
            let message: ServerMessage =
                crate::protocol::read_message(&mut reader, crate::protocol::MAX_FRAME_SIZE)
                    .unwrap();
            decoded = Some(decoder.decode(message).unwrap());
        }
        decoded.expect("an update was written")
    }

    fn control_kind(prepared: &PreparedRender) -> Option<&str> {
        match prepared.message() {
            ServerMessage::EndpointControl { kind, .. } => Some(kind),
            _ => None,
        }
    }

    #[test]
    fn underline_colors_survive_every_surface_encoding() {
        for encodings in [false, true] {
            let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
            state.enable_surface_reuse(encodings);
            state.enable_surface_delta(encodings);
            state.enable_surface_scroll(encodings);
            state.enable_surface_underline_color(true);
            let mut decoder = crate::protocol::surface_reuse::Decoder::new(encodings, encodings);

            // A red undercurl and one blue cell, like editor diagnostics.
            let mut surface = underline_surface();
            for col in 4..9 {
                pane_cell(&mut surface.frame, 3, col).underline_color = RED;
            }
            pane_cell(&mut surface.frame, 7, 0).underline_color = BLUE;
            let initial = state.prepare_pane_surface(surface.clone()).unwrap();
            assert!(initial.underline().is_some());
            let ServerMessage::PaneSurface(decoded) = deliver(&mut decoder, &initial) else {
                panic!("initial surface");
            };
            assert_eq!(decoded.frame, surface.frame, "encodings={encodings}");
            state.commit_sent_frame(initial);

            // A projection change resends no cells; the colors must stay.
            surface.projection_revision += 1;
            let update = state.prepare_pane_surface(surface.clone()).unwrap();
            assert_eq!(control_kind(&update).is_some(), encodings);
            assert!(update.underline().is_some());
            let ServerMessage::PaneSurface(decoded) = deliver(&mut decoder, &update) else {
                panic!("reused surface");
            };
            assert_eq!(decoded.frame, surface.frame, "encodings={encodings}");
            state.commit_sent_frame(update);

            // Recoloring or clearing an underline is a cell change of its own.
            pane_cell(&mut surface.frame, 0, 0).symbol = "L".into();
            pane_cell(&mut surface.frame, 3, 4).underline_color = BLUE;
            pane_cell(&mut surface.frame, 3, 8).underline_color = 0;
            let update = state.prepare_pane_surface(surface.clone()).unwrap();
            assert_eq!(
                control_kind(&update) == Some(crate::protocol::surface_delta::MESSAGE_KIND),
                encodings
            );
            let ServerMessage::PaneSurface(decoded) = deliver(&mut decoder, &update) else {
                panic!("changed surface");
            };
            assert_eq!(decoded.frame, surface.frame, "encodings={encodings}");
            state.commit_sent_frame(update);
            let mut client = decoded.frame;

            // Output scrolls one line: shifted rows keep their colors and the
            // new bottom line brings a colored cell of its own.
            let width = usize::from(surface.frame.width);
            let mut rows = Vec::new();
            for row in 0..PANE.height {
                let cells = if row + 1 < PANE.height {
                    let start = usize::from(PANE.y + row + 1) * width + usize::from(PANE.x);
                    surface.frame.cells[start..start + usize::from(PANE.width)].to_vec()
                } else {
                    let mut line = vec![surface.frame.cells[0].clone(); usize::from(PANE.width)];
                    line[2].symbol = "!".into();
                    line[2].underline_color = RED;
                    line
                };
                rows.push(crate::protocol::PaneSurfacePatchRow {
                    x: PANE.x,
                    y: PANE.y + row,
                    cells,
                });
            }
            crate::protocol::surface_delta::apply_rows(
                &mut surface.frame.cells,
                surface.frame.width,
                &rows,
            );
            let patch = state
                .prepare_pane_surface_patch(PaneSurfacePatch {
                    boot_id: surface.boot_id.clone(),
                    projection_revision: surface.projection_revision,
                    base_surface_revision: 3,
                    surface_revision: 0,
                    rows,
                    panes: surface.panes.clone(),
                    cursor: None,
                })
                .unwrap();
            assert_eq!(
                control_kind(&patch) == Some(crate::protocol::surface_scroll::MESSAGE_KIND),
                encodings
            );
            assert!(patch.underline().is_some());
            let ServerMessage::PaneSurfacePatch(decoded) = deliver(&mut decoder, &patch) else {
                panic!("scrolled patch");
            };
            crate::protocol::surface_delta::apply_rows(
                &mut client.cells,
                client.width,
                &decoded.rows,
            );
            assert_eq!(client, surface.frame, "encodings={encodings}");
            state.commit_sent_frame(patch);

            // The decoder's own baseline took the patch colors too.
            surface.projection_revision += 1;
            let update = state.prepare_pane_surface(surface.clone()).unwrap();
            let ServerMessage::PaneSurface(decoded) = deliver(&mut decoder, &update) else {
                panic!("surface after patch");
            };
            assert_eq!(decoded.frame, surface.frame, "encodings={encodings}");
            assert_eq!(decoded.surface_revision, 5);
        }
    }

    #[test]
    fn clients_that_did_not_ask_get_no_underline_control_and_unchanged_bytes() {
        let mut colored = underline_surface();
        pane_cell(&mut colored.frame, 3, 4).underline_color = RED;
        let plain = underline_surface();

        let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
        let prepared = state.prepare_pane_surface(colored.clone()).unwrap();
        assert!(prepared.underline().is_none());
        let mut bytes = Vec::new();
        crate::protocol::write_message(&mut bytes, prepared.message()).unwrap();

        let mut plain_state = ClientRenderState::new(RenderEncoding::SemanticFrame);
        let plain_prepared = plain_state.prepare_pane_surface(plain).unwrap();
        let mut plain_bytes = Vec::new();
        crate::protocol::write_message(&mut plain_bytes, plain_prepared.message()).unwrap();
        assert_eq!(bytes, plain_bytes, "the published cell layout is unchanged");

        // Without the control every decoded underline takes the text color.
        let mut decoder = crate::protocol::surface_reuse::Decoder::default();
        let ServerMessage::PaneSurface(decoded) = deliver(&mut decoder, &prepared) else {
            panic!("surface");
        };
        assert!(decoded
            .frame
            .cells
            .iter()
            .all(|cell| cell.underline_color == 0));

        // Asking for it changes nothing while no underline is colored.
        let mut asked = ClientRenderState::new(RenderEncoding::SemanticFrame);
        asked.enable_surface_underline_color(true);
        assert!(asked
            .prepare_pane_surface(underline_surface())
            .unwrap()
            .underline()
            .is_none());
    }

    #[test]
    fn underline_colors_for_another_update_are_not_applied() {
        let mut state = ClientRenderState::new(RenderEncoding::SemanticFrame);
        state.enable_surface_underline_color(true);
        let mut surface = underline_surface();
        pane_cell(&mut surface.frame, 3, 4).underline_color = RED;
        let first = state.prepare_pane_surface(surface.clone()).unwrap();
        let stale = first.underline().cloned().unwrap();
        state.commit_sent_frame(first);

        // The colors of a dropped update must not land on the next one.
        pane_cell(&mut surface.frame, 3, 4).underline_color = 0;
        let second = state.prepare_pane_surface(surface.clone()).unwrap();
        assert!(second.underline().is_none());
        let mut decoder = crate::protocol::surface_reuse::Decoder::default();
        decoder.decode(stale).unwrap();
        let ServerMessage::PaneSurface(decoded) = decoder.decode(second.message().clone()).unwrap()
        else {
            panic!("surface");
        };
        assert_eq!(decoded.frame, surface.frame);
    }
}
