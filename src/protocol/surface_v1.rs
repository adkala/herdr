//! Frozen `shell.surface.v1` wire mirrors.
//!
//! Endpoint generation 1 froze the bincode layout of `ServerMessage::PaneSurface`
//! (tag 13) and `ServerMessage::PaneSurfacePatch` (tag 19). The in-memory
//! surface types in `wire.rs` (`CellData` and everything that embeds it) have
//! since grown an SGR 58 underline color, published as `shell.surface.v2`.
//! Clients that only negotiated v1 keep receiving byte-identical v1 frames
//! through the mirrors below: the server projects each frame into them at
//! encode time and the client widens them back on decode.
//!
//! Field names, order, and types here must match the generation-1 originals
//! exactly and are append-closed. Do not add fields; introduce a new codec
//! instead. The frozen digests in `wire.rs` pin this layout.

use serde::{Deserialize, Serialize};

use super::{
    color_to_u32, CellData, ClientShellPopupSize, ClientShellPopupSurface, CursorState, FrameData,
    PaneSurfaceFrame, PaneSurfacePane, PaneSurfacePatch, PaneSurfacePatchRow, PaneSurfaceSplit,
    SurfaceGraphicsScene,
};

/// `CellData` as frozen in `shell.surface.v1`: no underline color.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellDataV1 {
    pub symbol: String,
    pub fg: u32,
    pub bg: u32,
    pub modifier: u16,
    pub skip: bool,
    pub hyperlink: Option<u32>,
}

/// `FrameData` as frozen in `shell.surface.v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameDataV1 {
    pub cells: Vec<CellDataV1>,
    pub width: u16,
    pub height: u16,
    pub cursor: Option<CursorState>,
    pub hyperlinks: Vec<String>,
    pub graphics: Vec<u8>,
}

/// `ClientShellPopupSurface` as frozen in `shell.surface.v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientShellPopupSurfaceV1 {
    pub terminal_id: String,
    pub title: String,
    pub width: Option<ClientShellPopupSize>,
    pub height: Option<ClientShellPopupSize>,
    pub frame: FrameDataV1,
    pub mouse_reporting: bool,
    pub sgr_pixel_mouse: bool,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// `PaneSurfaceFrame` as frozen in `shell.surface.v1` (`ServerMessage::PaneSurface`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSurfaceFrameV1 {
    pub boot_id: String,
    pub projection_revision: u64,
    pub surface_revision: u64,
    pub frame: FrameDataV1,
    pub panes: Vec<PaneSurfacePane>,
    pub splits: Vec<PaneSurfaceSplit>,
    pub popup: Option<Box<ClientShellPopupSurfaceV1>>,
    pub graphics: SurfaceGraphicsScene,
}

/// `PaneSurfacePatchRow` as frozen in `shell.surface.v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSurfacePatchRowV1 {
    pub x: u16,
    pub y: u16,
    pub cells: Vec<CellDataV1>,
}

/// `PaneSurfacePatch` as frozen in `shell.surface.v1` (`ServerMessage::PaneSurfacePatch`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSurfacePatchV1 {
    pub boot_id: String,
    pub projection_revision: u64,
    pub base_surface_revision: u64,
    pub surface_revision: u64,
    pub rows: Vec<PaneSurfacePatchRowV1>,
    pub panes: Vec<PaneSurfacePane>,
    pub cursor: Option<CursorState>,
}

/// Underline color a widened v1 cell reports: `Reset`, so the underline takes
/// the text color exactly as it did before `shell.surface.v2` existed.
fn reset_underline_color() -> u32 {
    color_to_u32(ratatui::style::Color::Reset)
}

// Server side: encode-time projection for clients that negotiated v1. The
// projection drops only `underline_color`; every other field is copied as is.

impl From<&CellData> for CellDataV1 {
    fn from(cell: &CellData) -> Self {
        Self {
            symbol: cell.symbol.clone(),
            fg: cell.fg,
            bg: cell.bg,
            modifier: cell.modifier,
            skip: cell.skip,
            hyperlink: cell.hyperlink,
        }
    }
}

impl From<&FrameData> for FrameDataV1 {
    fn from(frame: &FrameData) -> Self {
        Self {
            cells: frame.cells.iter().map(CellDataV1::from).collect(),
            width: frame.width,
            height: frame.height,
            cursor: frame.cursor.clone(),
            hyperlinks: frame.hyperlinks.clone(),
            graphics: frame.graphics.clone(),
        }
    }
}

impl From<&ClientShellPopupSurface> for ClientShellPopupSurfaceV1 {
    fn from(popup: &ClientShellPopupSurface) -> Self {
        Self {
            terminal_id: popup.terminal_id.clone(),
            title: popup.title.clone(),
            width: popup.width,
            height: popup.height,
            frame: FrameDataV1::from(&popup.frame),
            mouse_reporting: popup.mouse_reporting,
            sgr_pixel_mouse: popup.sgr_pixel_mouse,
            pixel_width: popup.pixel_width,
            pixel_height: popup.pixel_height,
        }
    }
}

impl From<&PaneSurfaceFrame> for PaneSurfaceFrameV1 {
    fn from(surface: &PaneSurfaceFrame) -> Self {
        Self {
            boot_id: surface.boot_id.clone(),
            projection_revision: surface.projection_revision,
            surface_revision: surface.surface_revision,
            frame: FrameDataV1::from(&surface.frame),
            panes: surface.panes.clone(),
            splits: surface.splits.clone(),
            popup: surface
                .popup
                .as_deref()
                .map(|popup| Box::new(ClientShellPopupSurfaceV1::from(popup))),
            graphics: surface.graphics.clone(),
        }
    }
}

impl From<&PaneSurfacePatchRow> for PaneSurfacePatchRowV1 {
    fn from(row: &PaneSurfacePatchRow) -> Self {
        Self {
            x: row.x,
            y: row.y,
            cells: row.cells.iter().map(CellDataV1::from).collect(),
        }
    }
}

impl From<&PaneSurfacePatch> for PaneSurfacePatchV1 {
    fn from(patch: &PaneSurfacePatch) -> Self {
        Self {
            boot_id: patch.boot_id.clone(),
            projection_revision: patch.projection_revision,
            base_surface_revision: patch.base_surface_revision,
            surface_revision: patch.surface_revision,
            rows: patch.rows.iter().map(PaneSurfacePatchRowV1::from).collect(),
            panes: patch.panes.clone(),
            cursor: patch.cursor.clone(),
        }
    }
}

// Client side: widen a decoded v1 frame into the internal types. Ownership
// moves; only the underline color is synthesized.

impl From<CellDataV1> for CellData {
    fn from(cell: CellDataV1) -> Self {
        Self {
            symbol: cell.symbol,
            fg: cell.fg,
            bg: cell.bg,
            modifier: cell.modifier,
            skip: cell.skip,
            hyperlink: cell.hyperlink,
            underline_color: reset_underline_color(),
        }
    }
}

impl From<FrameDataV1> for FrameData {
    fn from(frame: FrameDataV1) -> Self {
        Self {
            cells: frame.cells.into_iter().map(CellData::from).collect(),
            width: frame.width,
            height: frame.height,
            cursor: frame.cursor,
            hyperlinks: frame.hyperlinks,
            graphics: frame.graphics,
        }
    }
}

impl From<ClientShellPopupSurfaceV1> for ClientShellPopupSurface {
    fn from(popup: ClientShellPopupSurfaceV1) -> Self {
        Self {
            terminal_id: popup.terminal_id,
            title: popup.title,
            width: popup.width,
            height: popup.height,
            frame: FrameData::from(popup.frame),
            mouse_reporting: popup.mouse_reporting,
            sgr_pixel_mouse: popup.sgr_pixel_mouse,
            pixel_width: popup.pixel_width,
            pixel_height: popup.pixel_height,
        }
    }
}

impl From<PaneSurfaceFrameV1> for PaneSurfaceFrame {
    fn from(surface: PaneSurfaceFrameV1) -> Self {
        Self {
            boot_id: surface.boot_id,
            projection_revision: surface.projection_revision,
            surface_revision: surface.surface_revision,
            frame: FrameData::from(surface.frame),
            panes: surface.panes,
            splits: surface.splits,
            popup: surface
                .popup
                .map(|popup| Box::new(ClientShellPopupSurface::from(*popup))),
            graphics: surface.graphics,
        }
    }
}

impl From<PaneSurfacePatchRowV1> for PaneSurfacePatchRow {
    fn from(row: PaneSurfacePatchRowV1) -> Self {
        Self {
            x: row.x,
            y: row.y,
            cells: row.cells.into_iter().map(CellData::from).collect(),
        }
    }
}

impl From<PaneSurfacePatchV1> for PaneSurfacePatch {
    fn from(patch: PaneSurfacePatchV1) -> Self {
        Self {
            boot_id: patch.boot_id,
            projection_revision: patch.projection_revision,
            base_surface_revision: patch.base_surface_revision,
            surface_revision: patch.surface_revision,
            rows: patch
                .rows
                .into_iter()
                .map(PaneSurfacePatchRow::from)
                .collect(),
            panes: patch.panes,
            cursor: patch.cursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn cell(symbol: &str, underline_color: Color) -> CellData {
        CellData {
            symbol: symbol.into(),
            fg: color_to_u32(Color::Rgb(1, 2, 3)),
            bg: color_to_u32(Color::Indexed(4)),
            modifier: 0b101,
            skip: symbol == " ",
            hyperlink: (symbol == "l").then_some(0),
            underline_color: color_to_u32(underline_color),
        }
    }

    fn surface(underline_color: Color) -> PaneSurfaceFrame {
        PaneSurfaceFrame {
            boot_id: "boot".into(),
            projection_revision: 3,
            surface_revision: 4,
            frame: FrameData {
                cells: vec![
                    cell("a", underline_color),
                    cell(" ", underline_color),
                    cell("l", underline_color),
                    cell("🦀", Color::Reset),
                ],
                width: 2,
                height: 2,
                cursor: Some(CursorState {
                    x: 1,
                    y: 0,
                    visible: true,
                    shape: 2,
                }),
                hyperlinks: vec!["https://example.invalid".into()],
                graphics: vec![1, 2, 3],
            },
            panes: Vec::new(),
            splits: Vec::new(),
            popup: Some(Box::new(ClientShellPopupSurface {
                terminal_id: "popup".into(),
                title: "popup".into(),
                width: Some(ClientShellPopupSize::Cells(10)),
                height: Some(ClientShellPopupSize::Percent(50)),
                frame: FrameData {
                    cells: vec![cell("p", underline_color)],
                    width: 1,
                    height: 1,
                    cursor: None,
                    hyperlinks: Vec::new(),
                    graphics: Vec::new(),
                },
                mouse_reporting: true,
                sgr_pixel_mouse: false,
                pixel_width: 7,
                pixel_height: 9,
            })),
            graphics: SurfaceGraphicsScene::default(),
        }
    }

    fn patch(underline_color: Color) -> PaneSurfacePatch {
        PaneSurfacePatch {
            boot_id: "boot".into(),
            projection_revision: 3,
            base_surface_revision: 4,
            surface_revision: 5,
            rows: vec![PaneSurfacePatchRow {
                x: 1,
                y: 0,
                cells: vec![cell("z", underline_color)],
            }],
            panes: Vec::new(),
            cursor: None,
        }
    }

    #[test]
    fn projection_drops_only_the_underline_color() {
        let colored = surface(Color::Rgb(255, 0, 0));
        let plain = surface(Color::Reset);
        assert_ne!(colored, plain);
        assert_eq!(
            PaneSurfaceFrameV1::from(&colored),
            PaneSurfaceFrameV1::from(&plain),
            "v1 clients must not observe underline colors"
        );
        assert_eq!(
            PaneSurfacePatchV1::from(&patch(Color::Indexed(9))),
            PaneSurfacePatchV1::from(&patch(Color::Reset))
        );
    }

    #[test]
    fn widening_a_projection_restores_a_reset_underline() {
        let plain = surface(Color::Reset);
        let widened = PaneSurfaceFrame::from(PaneSurfaceFrameV1::from(&plain));
        assert_eq!(widened, plain, "v1 round trip is lossless without colors");

        let colored = surface(Color::Rgb(255, 0, 0));
        let widened = PaneSurfaceFrame::from(PaneSurfaceFrameV1::from(&colored));
        assert!(widened
            .frame
            .cells
            .iter()
            .all(|cell| cell.underline_color == color_to_u32(Color::Reset)));
        assert_eq!(
            widened.popup.as_ref().unwrap().frame.cells[0].underline_color,
            color_to_u32(Color::Reset)
        );

        let widened = PaneSurfacePatch::from(PaneSurfacePatchV1::from(&patch(Color::Reset)));
        assert_eq!(widened, patch(Color::Reset));
    }
}
