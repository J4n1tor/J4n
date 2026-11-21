//! A view held in a chat row (`data.view`): the core says which field draws it (`field`: `meter`, `stat`, `chip`, `spark`, `line`, `bars`), its
//! `value` and `props`, as it would for a node of a page, and the transcript draws it in the width it has. The core sends the row again when the
//! value changes, so it stays up to date while it is on the screen.

use super::{gauge, plot};
use crate::theme::Theme;
use j4n_proto::{Field, FieldKind, Json};
use ratatui::text::Line;

/// The lines of the view `v` (`{field, title, value, props}`) in `width` columns; none for a field that is not one of these.
pub fn held(th: &Theme, v: &Json, width: usize) -> Vec<Line<'static>> {
    let (value, title) = (&v["value"], v["title"].as_str().unwrap_or_default());
    let kind = match v["field"].as_str().unwrap_or_default() {
        "meter" => FieldKind::Meter,
        "stat" => FieldKind::Stat,
        "chip" => FieldKind::Chip,
        "spark" => FieldKind::Spark,
        "line" => FieldKind::Line,
        "bars" => FieldKind::Bars,
        _ => return Vec::new(),
    };
    let field = Field { kind, bind: String::new(), props: v.get("props").cloned().unwrap_or(Json::Null) };
    match kind {
        FieldKind::Meter if value.is_array() => gauge::meters(th, value, width),
        FieldKind::Meter => gauge::meter(th, title, value, &field, width),
        FieldKind::Stat => gauge::stat(th, value, width),
        FieldKind::Spark => gauge::spark(th, title, value, &field, width),
        FieldKind::Chip => gauge::chip(th, value),
        _ => plot::draw(th, title, value, &field, width, plot::rows(&field, value)),
    }
}
