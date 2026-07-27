use std::borrow::Cow;

use ratatui::layout::Rect;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PopupSize {
    Cells(u16),
    Percent(u8),
}

impl PopupSize {
    pub(crate) fn resolve(self, available: u16) -> u16 {
        match self {
            Self::Cells(cells) => cells,
            Self::Percent(percent) => ((available as u32 * percent as u32) / 100) as u16,
        }
    }

    pub(crate) fn parse_cli(value: &str) -> Result<Self, String> {
        if let Some(percent) = value.strip_suffix('%') {
            let percent = percent
                .parse::<u8>()
                .map_err(|_| "must be a number of cells or a percentage like 80%".to_string())?;
            if !(1..=100).contains(&percent) {
                return Err("percentage must be between 1% and 100%".to_string());
            }
            return Ok(Self::Percent(percent));
        }
        value
            .parse::<u16>()
            .map(Self::Cells)
            .map_err(|_| "must be a number of cells or a percentage like 80%".to_string())
    }

    fn parse_percent_string(value: &str) -> Result<Self, String> {
        if value.ends_with('%') {
            return Self::parse_cli(value);
        }
        Err("string sizes must be percentages like 80%; use a number for cells".to_string())
    }
}

/// Popup presentation. `Pane` keeps the classic bordered pane with the title
/// in the border; `Modal` matches the built-in settings overlay: dimmed
/// backdrop, panel shell, and a bold title header row above the content.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PopupChrome {
    #[default]
    Pane,
    Modal,
}

impl PopupChrome {
    /// Rows reserved inside the panel above the content (title + separator).
    pub(crate) fn header_rows(self) -> u16 {
        match self {
            Self::Pane => 0,
            Self::Modal => 2,
        }
    }

    pub(crate) fn parse_cli(value: &str) -> Result<Self, String> {
        match value {
            "pane" => Ok(Self::Pane),
            "modal" => Ok(Self::Modal),
            _ => Err("must be pane or modal".to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PopupResolvedGeometry {
    pub outer: Rect,
    pub inner: Rect,
}

pub(crate) fn resolve_popup_geometry(
    width: Option<PopupSize>,
    height: Option<PopupSize>,
    area: Rect,
    chrome: PopupChrome,
) -> Option<PopupResolvedGeometry> {
    let header_rows = chrome.header_rows();
    let min_height = 4 + header_rows;
    let default_width = area.width.saturating_div(2).max(6);
    let default_height = area.height.saturating_div(2).max(min_height);
    let outer_width = width
        .map(|width| width.resolve(area.width))
        .unwrap_or(default_width)
        .max(6)
        .min(area.width);
    // The header rows live inside the panel, so charging them to a declared
    // height would silently cost the content two rows the moment a popup
    // switches to modal chrome. Grow the box by the chrome's own overhead
    // instead: the same declaration then yields the same usable area under
    // either chrome, and only the screen edge clamps it.
    let outer_height = height
        .map(|height| height.resolve(area.height))
        .unwrap_or(default_height)
        .saturating_add(header_rows)
        .max(min_height)
        .min(area.height);
    if outer_width < 6 || outer_height < min_height {
        return None;
    }

    let outer_x = area.x + (area.width.saturating_sub(outer_width)) / 2;
    let outer_y = area.y + (area.height.saturating_sub(outer_height)) / 2;
    let pane_inner_width = outer_width.saturating_sub(2);
    let pane_inner_height = outer_height.saturating_sub(2).saturating_sub(header_rows);
    let terminal_cols = if pane_inner_width <= 4 {
        pane_inner_width
    } else {
        pane_inner_width.saturating_sub(1)
    };
    let inner = Rect::new(
        outer_x.saturating_add(1),
        outer_y.saturating_add(1).saturating_add(header_rows),
        terminal_cols,
        pane_inner_height,
    );
    Some(PopupResolvedGeometry {
        outer: Rect::new(outer_x, outer_y, outer_width, outer_height),
        inner,
    })
}

impl Serialize for PopupSize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Cells(cells) => serializer.serialize_u16(*cells),
            Self::Percent(percent) => serializer.serialize_str(&format!("{percent}%")),
        }
    }
}

impl<'de> Deserialize<'de> for PopupSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PopupSizeVisitor;

        impl serde::de::Visitor<'_> for PopupSizeVisitor {
            type Value = PopupSize;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a cell count or percentage string like 80%")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let value =
                    u16::try_from(value).map_err(|_| E::custom("cell count must fit in u16"))?;
                Ok(PopupSize::Cells(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let value = u16::try_from(value)
                    .map_err(|_| E::custom("cell count must be between 0 and 65535"))?;
                Ok(PopupSize::Cells(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                PopupSize::parse_percent_string(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(PopupSizeVisitor)
    }
}

impl schemars::JsonSchema for PopupSize {
    fn schema_name() -> Cow<'static, str> {
        "PopupSize".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "oneOf": [
                {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 65535,
                    "description": "Outer popup size in terminal cells, including the border."
                },
                {
                    "type": "string",
                    "pattern": "^(100|[1-9][0-9]?)%$",
                    "description": "Outer popup size as a percentage of the terminal area, for example 80%."
                }
            ]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PopupSize;

    #[test]
    fn parses_cells_and_percent() {
        assert_eq!(PopupSize::parse_cli("120"), Ok(PopupSize::Cells(120)));
        assert_eq!(PopupSize::parse_cli("80%"), Ok(PopupSize::Percent(80)));
        assert_eq!(PopupSize::Percent(80).resolve(100), 80);
    }

    #[test]
    fn rejects_invalid_percent() {
        assert!(PopupSize::parse_cli("0%").is_err());
        assert!(PopupSize::parse_cli("101%").is_err());
        assert!(PopupSize::parse_cli("%").is_err());
    }

    #[test]
    fn string_deserialization_requires_percent() {
        assert!(serde_json::from_value::<PopupSize>(serde_json::json!("120")).is_err());
        assert_eq!(
            serde_json::from_value::<PopupSize>(serde_json::json!("80%")).unwrap(),
            PopupSize::Percent(80)
        );
    }

    #[test]
    fn serializes_percent_as_string() {
        assert_eq!(
            serde_json::to_value(PopupSize::Percent(80)).unwrap(),
            serde_json::json!("80%")
        );
        assert_eq!(
            serde_json::to_value(PopupSize::Cells(120)).unwrap(),
            serde_json::json!(120)
        );
    }

    #[test]
    fn resolves_requested_outer_size_and_inner_terminal_area() {
        let resolved = super::resolve_popup_geometry(
            Some(PopupSize::Percent(80)),
            Some(PopupSize::Percent(40)),
            ratatui::layout::Rect::new(0, 0, 100, 30),
            super::PopupChrome::Pane,
        )
        .unwrap();
        assert_eq!(resolved.outer, ratatui::layout::Rect::new(10, 9, 80, 12));
        assert_eq!(resolved.inner, ratatui::layout::Rect::new(11, 10, 77, 10));
    }

    #[test]
    fn allows_full_terminal_outer_size() {
        let resolved = super::resolve_popup_geometry(
            Some(PopupSize::Percent(100)),
            Some(PopupSize::Percent(100)),
            ratatui::layout::Rect::new(4, 2, 100, 30),
            super::PopupChrome::Pane,
        )
        .unwrap();

        assert_eq!(resolved.outer, ratatui::layout::Rect::new(4, 2, 100, 30));
        assert_eq!(resolved.inner, ratatui::layout::Rect::new(5, 3, 97, 28));
    }

    #[test]
    fn enforces_runtime_minimum_terminal_width() {
        let resolved = super::resolve_popup_geometry(
            Some(PopupSize::Cells(4)),
            None,
            ratatui::layout::Rect::new(0, 0, 80, 24),
            super::PopupChrome::Pane,
        )
        .unwrap();
        assert_eq!(resolved.outer.width, 6);
        assert_eq!(resolved.inner.width, 4);

        assert!(super::resolve_popup_geometry(
            None,
            None,
            ratatui::layout::Rect::new(0, 0, 5, 24),
            super::PopupChrome::Pane,
        )
        .is_none());
    }

    #[test]
    fn modal_chrome_reserves_header_rows_above_the_terminal() {
        let pane = super::resolve_popup_geometry(
            Some(PopupSize::Cells(60)),
            Some(PopupSize::Cells(20)),
            ratatui::layout::Rect::new(0, 0, 100, 30),
            super::PopupChrome::Pane,
        )
        .unwrap();
        let modal = super::resolve_popup_geometry(
            Some(PopupSize::Cells(60)),
            Some(PopupSize::Cells(20)),
            ratatui::layout::Rect::new(0, 0, 100, 30),
            super::PopupChrome::Modal,
        )
        .unwrap();

        // The panel grows by its own header instead of charging the content
        // for it, so switching chrome does not shrink the terminal inside.
        assert_eq!(modal.outer.height, pane.outer.height + 2);
        assert_eq!(modal.outer.width, pane.outer.width);
        assert_eq!(modal.inner.x, pane.inner.x);
        assert_eq!(modal.inner.width, pane.inner.width);
        assert_eq!(modal.inner.height, pane.inner.height);
    }

    #[test]
    fn modal_chrome_growth_stops_at_the_screen_edge() {
        let area = ratatui::layout::Rect::new(0, 0, 40, 12);
        let modal = super::resolve_popup_geometry(
            Some(PopupSize::Percent(100)),
            Some(PopupSize::Percent(100)),
            area,
            super::PopupChrome::Modal,
        )
        .unwrap();

        assert_eq!(modal.outer.height, area.height);
        assert_eq!(modal.inner.height, area.height - 4);
    }

    #[test]
    fn modal_chrome_raises_minimum_height() {
        assert!(super::resolve_popup_geometry(
            None,
            Some(PopupSize::Cells(5)),
            ratatui::layout::Rect::new(0, 0, 80, 5),
            super::PopupChrome::Modal,
        )
        .is_none());
    }

    #[test]
    fn chrome_parses_cli_values() {
        assert_eq!(
            super::PopupChrome::parse_cli("pane"),
            Ok(super::PopupChrome::Pane)
        );
        assert_eq!(
            super::PopupChrome::parse_cli("modal"),
            Ok(super::PopupChrome::Modal)
        );
        assert!(super::PopupChrome::parse_cli("fancy").is_err());
    }
}
