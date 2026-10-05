//! Optional delivery of SGR 58 underline colors beside a pane surface update.
//!
//! Every published surface encoding embeds the six-field cell, so the color
//! cannot join the cell without changing a frozen layout. A server instead
//! names the colored cells of an update in this control, written immediately
//! ahead of that update in the same client write, and the client paints them
//! onto the cells it decodes. Cells an update does not transmit keep the
//! color they had: the server reuses a cell only when its color is equal too.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{CellData, PaneSurfaceFrame, PaneSurfacePatch, ServerMessage};

pub(crate) const CAPABILITY: &str = "surface_underline_color";
pub(crate) const MESSAGE_KIND: &str = "endpoint.surface-underline.v1";
/// Colored runs one update may name. Cells past the cap underline in their
/// text color, as every cell does without this control.
pub(crate) const MAX_RUNS: usize = 4096;

/// Column, row, cell count and packed color of one run of equally colored cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Run(
    pub(crate) u16,
    pub(crate) u16,
    pub(crate) u16,
    pub(crate) u32,
);

// This JSON layout is frozen with MESSAGE_KIND.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SurfaceUnderline {
    pub(crate) boot_id: String,
    /// Revision of the surface update these colors belong to.
    pub(crate) surface_revision: u64,
    /// Runs relative to the pane surface frame.
    #[serde(default)]
    pub(crate) frame: Vec<Run>,
    /// Runs relative to the popup frame of a complete surface.
    #[serde(default)]
    pub(crate) popup: Vec<Run>,
}

/// Appends the colored runs of one row span; `false` once the cap is reached.
fn push_runs(runs: &mut Vec<Run>, x: u16, y: u16, cells: &[CellData]) -> bool {
    let mut index = 0;
    while index < cells.len() {
        let color = cells[index].underline_color;
        if color == 0 {
            index += 1;
            continue;
        }
        let start = index;
        while index < cells.len() && cells[index].underline_color == color {
            index += 1;
        }
        if runs.len() == MAX_RUNS {
            return false;
        }
        let Ok(offset) = u16::try_from(start) else {
            return false;
        };
        runs.push(Run(
            x.saturating_add(offset),
            y,
            (index - start).min(usize::from(u16::MAX)) as u16,
            color,
        ));
    }
    true
}

fn grid_runs(cells: &[CellData], width: u16) -> Vec<Run> {
    let mut runs = Vec::new();
    if width == 0 {
        return runs;
    }
    for (y, row) in cells.chunks(usize::from(width)).enumerate() {
        let Ok(y) = u16::try_from(y) else {
            break;
        };
        if !push_runs(&mut runs, 0, y, row) {
            break;
        }
    }
    runs
}

fn message(underline: &SurfaceUnderline) -> Option<ServerMessage> {
    if underline.frame.is_empty() && underline.popup.is_empty() {
        return None;
    }
    match serde_json::to_string(underline) {
        Ok(data) => Some(ServerMessage::EndpointControl {
            kind: MESSAGE_KIND.into(),
            data,
        }),
        Err(error) => {
            tracing::warn!(%error, "failed to encode surface underline colors");
            None
        }
    }
}

/// The control to write ahead of a complete surface, in any of its encodings,
/// or `None` when no cell has a colored underline.
pub(crate) fn surface_message(surface: &PaneSurfaceFrame) -> Option<ServerMessage> {
    message(&SurfaceUnderline {
        boot_id: surface.boot_id.clone(),
        surface_revision: surface.surface_revision,
        frame: grid_runs(&surface.frame.cells, surface.frame.width),
        popup: surface
            .popup
            .as_ref()
            .map(|popup| grid_runs(&popup.frame.cells, popup.frame.width))
            .unwrap_or_default(),
    })
}

/// The control to write ahead of a row patch, in either of its encodings, or
/// `None` when no patched cell has a colored underline.
pub(crate) fn patch_message(patch: &PaneSurfacePatch) -> Option<ServerMessage> {
    let mut frame = Vec::new();
    for row in &patch.rows {
        if !push_runs(&mut frame, row.x, row.y, &row.cells) {
            break;
        }
    }
    message(&SurfaceUnderline {
        boot_id: patch.boot_id.clone(),
        surface_revision: patch.surface_revision,
        frame,
        popup: Vec::new(),
    })
}

/// A malformed control is dropped like an unknown one: its update then shows
/// the underlines it transmits in their text color.
pub(crate) fn decode(data: &str) -> Option<SurfaceUnderline> {
    serde_json::from_str(data)
        .map_err(|error| tracing::debug!(%error, "ignoring malformed surface underline colors"))
        .ok()
}

/// Paints runs onto a row-major grid, skipping whatever falls outside it.
pub(crate) fn paint_grid(runs: &[Run], cells: &mut [CellData], width: u16) {
    for &Run(x, y, len, color) in runs {
        if x >= width {
            continue;
        }
        let start = usize::from(y) * usize::from(width) + usize::from(x);
        let len = usize::from(len.min(width - x));
        if let Some(run) = cells.get_mut(start..start + len) {
            for cell in run {
                cell.underline_color = color;
            }
        }
    }
}

impl SurfaceUnderline {
    pub(crate) fn belongs_to(&self, boot_id: &str, surface_revision: u64) -> bool {
        self.boot_id == boot_id && self.surface_revision == surface_revision
    }

    pub(crate) fn paint_surface(&self, surface: &mut PaneSurfaceFrame) {
        paint_grid(&self.frame, &mut surface.frame.cells, surface.frame.width);
        if let Some(popup) = &mut surface.popup {
            paint_grid(&self.popup, &mut popup.frame.cells, popup.frame.width);
        }
    }

    /// Paints the runs onto the cells a patch carries. A run outside them
    /// names a cell the client already holds in that color.
    pub(crate) fn paint_patch(&self, patch: &mut PaneSurfacePatch) {
        if self.frame.is_empty() {
            return;
        }
        let mut rows_at = HashMap::<u16, Vec<usize>>::new();
        for (index, row) in patch.rows.iter().enumerate() {
            rows_at.entry(row.y).or_default().push(index);
        }
        for &Run(x, y, len, color) in &self.frame {
            let Some(rows) = rows_at.get(&y) else {
                continue;
            };
            let (start, end) = (usize::from(x), usize::from(x) + usize::from(len));
            for &index in rows {
                let row = &mut patch.rows[index];
                let row_start = usize::from(row.x);
                let from = start.max(row_start);
                let to = end.min(row_start + row.cells.len());
                if from < to {
                    for cell in &mut row.cells[from - row_start..to - row_start] {
                        cell.underline_color = color;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{FrameData, PaneSurfacePatchRow, SurfaceGraphicsScene};

    const RED: u32 = 0x02ff_0000;
    const BLUE: u32 = 0x0200_00ff;

    fn cell(symbol: &str, underline_color: u32) -> CellData {
        CellData {
            symbol: symbol.into(),
            fg: 0,
            bg: 0,
            modifier: 0,
            skip: false,
            hyperlink: None,
            underline_color,
        }
    }

    fn frame(width: u16, colors: &[u32]) -> FrameData {
        FrameData {
            cells: colors.iter().map(|color| cell("x", *color)).collect(),
            width,
            height: (colors.len() / usize::from(width)) as u16,
            cursor: None,
            hyperlinks: Vec::new(),
            graphics: Vec::new(),
        }
    }

    fn surface(width: u16, colors: &[u32]) -> PaneSurfaceFrame {
        PaneSurfaceFrame {
            boot_id: "boot".into(),
            projection_revision: 1,
            surface_revision: 7,
            frame: frame(width, colors),
            panes: Vec::new(),
            splits: Vec::new(),
            popup: None,
            graphics: SurfaceGraphicsScene::default(),
        }
    }

    fn patch(rows: Vec<PaneSurfacePatchRow>) -> PaneSurfacePatch {
        PaneSurfacePatch {
            boot_id: "boot".into(),
            projection_revision: 1,
            base_surface_revision: 7,
            surface_revision: 8,
            rows,
            panes: Vec::new(),
            cursor: None,
        }
    }

    fn decoded(message: Option<ServerMessage>) -> SurfaceUnderline {
        let Some(ServerMessage::EndpointControl { kind, data }) = message else {
            panic!("expected an underline control");
        };
        assert_eq!(kind, MESSAGE_KIND);
        decode(&data).expect("control decodes")
    }

    fn colors(cells: &[CellData]) -> Vec<u32> {
        cells.iter().map(|cell| cell.underline_color).collect()
    }

    #[test]
    fn uncolored_updates_send_no_control() {
        assert!(surface_message(&surface(2, &[0, 0, 0, 0])).is_none());
        assert!(patch_message(&patch(vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell("a", 0)],
        }]))
        .is_none());
    }

    #[test]
    fn control_json_layout_is_frozen() {
        let Some(ServerMessage::EndpointControl { kind, data }) =
            surface_message(&surface(3, &[0, RED, RED, BLUE, 0, 0]))
        else {
            panic!("expected an underline control");
        };
        assert_eq!(kind, "endpoint.surface-underline.v1");
        assert_eq!(
            data,
            r#"{"boot_id":"boot","surface_revision":7,"frame":[[1,0,2,50266112],[0,1,1,33554687]],"popup":[]}"#
        );
        // Unknown fields and absent lists must keep decoding.
        let lenient =
            decode(r#"{"boot_id":"boot","surface_revision":7,"future":true}"#).expect("lenient");
        assert!(lenient.frame.is_empty() && lenient.popup.is_empty());
        assert!(decode("not json").is_none());
    }

    #[test]
    fn surface_runs_split_on_color_and_row_and_round_trip_through_paint() {
        let source = surface(4, &[RED, RED, BLUE, 0, 0, 0, 0, RED]);
        let underline = decoded(surface_message(&source));
        assert_eq!(
            underline.frame,
            vec![Run(0, 0, 2, RED), Run(2, 0, 1, BLUE), Run(3, 1, 1, RED)]
        );
        assert!(underline.belongs_to("boot", 7));
        assert!(!underline.belongs_to("boot", 8));
        assert!(!underline.belongs_to("other", 7));

        // What a client decodes from any published encoding has no colors.
        let mut received = surface(4, &[0; 8]);
        underline.paint_surface(&mut received);
        assert_eq!(received, source);
    }

    #[test]
    fn popup_cells_travel_in_their_own_grid() {
        let mut source = surface(2, &[0, 0]);
        source.popup = Some(Box::new(crate::protocol::ClientShellPopupSurface {
            terminal_id: "term".into(),
            title: String::new(),
            width: None,
            height: None,
            frame: frame(2, &[0, BLUE]),
            mouse_reporting: false,
            sgr_pixel_mouse: false,
            pixel_width: 0,
            pixel_height: 0,
        }));
        let underline = decoded(surface_message(&source));
        assert!(underline.frame.is_empty());
        assert_eq!(underline.popup, vec![Run(1, 0, 1, BLUE)]);

        let mut received = source.clone();
        received.popup.as_mut().unwrap().frame.cells[1].underline_color = 0;
        underline.paint_surface(&mut received);
        assert_eq!(received, source);
    }

    #[test]
    fn patch_runs_use_surface_coordinates_and_paint_only_carried_cells() {
        let source = patch(vec![
            PaneSurfacePatchRow {
                x: 3,
                y: 2,
                cells: vec![cell("a", 0), cell("b", RED), cell("c", RED)],
            },
            PaneSurfacePatchRow {
                x: 0,
                y: 5,
                cells: vec![cell("d", BLUE)],
            },
        ]);
        let underline = decoded(patch_message(&source));
        assert_eq!(underline.frame, vec![Run(4, 2, 2, RED), Run(0, 5, 1, BLUE)]);
        assert!(underline.belongs_to("boot", 8));

        // A scroll-aware decode can cut the same cells into different spans.
        let mut received = patch(vec![
            PaneSurfacePatchRow {
                x: 3,
                y: 2,
                cells: vec![cell("a", 0), cell("b", 0)],
            },
            PaneSurfacePatchRow {
                x: 5,
                y: 2,
                cells: vec![cell("c", 0)],
            },
            PaneSurfacePatchRow {
                x: 4,
                y: 4,
                cells: vec![cell("z", 0)],
            },
        ]);
        underline.paint_patch(&mut received);
        assert_eq!(colors(&received.rows[0].cells), vec![0, RED]);
        assert_eq!(colors(&received.rows[1].cells), vec![RED]);
        assert_eq!(colors(&received.rows[2].cells), vec![0]);
    }

    #[test]
    fn paint_grid_ignores_runs_outside_the_grid() {
        let mut cells = vec![cell("a", 0), cell("b", 0), cell("c", 0), cell("d", 0)];
        paint_grid(
            &[
                Run(1, 0, 9, RED),
                Run(2, 0, 1, BLUE),
                Run(0, 9, 1, BLUE),
                Run(0, 1, 0, BLUE),
            ],
            &mut cells,
            2,
        );
        assert_eq!(colors(&cells), vec![0, RED, 0, 0]);
    }

    #[test]
    fn runs_are_capped() {
        let alternating = (0..(MAX_RUNS + 10) * 2)
            .map(|index| if index % 2 == 0 { RED } else { 0 })
            .collect::<Vec<_>>();
        let underline = decoded(surface_message(&surface(64, &alternating)));
        assert_eq!(underline.frame.len(), MAX_RUNS);
    }
}
