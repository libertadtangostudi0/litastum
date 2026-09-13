use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{Block, Borders},
    Frame,
};
use ratatui_image::{FilterType, Resize, StatefulImage};

use crate::explorer::ImagePreviewState;
use crate::theming::Theme;

/// Renders `F3`'s currently-previewed image into `area` -- replaces the
/// right panel's own file listing entirely while `Mode::ImagePreview` is
/// active (`ui::draw`'s own panel-drawing site), rather than overlaying
/// a popup on top of it. Same border styling `draw_panel` uses for the
/// *active* panel (`theme.accent`), since this panel is the one `F3`
/// switched focus to (`explorer::image_preview::open_preview`'s own doc
/// comment).
///
/// `Resize::Fit`'s own default filter (left unset) is
/// `FilterType::Nearest` -- reported directly as looking "terrible" on
/// a real screenshot: nearest-neighbor downscaling picks one source
/// pixel per half-block cell and throws the rest away, so any fine
/// detail (small text, thin lines) aliases into harsh, blocky noise
/// instead of blending into the muted half-block color a real
/// downsampled thumbnail would show. `FilterType::Lanczos3` (highest-
/// quality resampling `image` ships, averaging/blending source pixels
/// properly) fixes that -- costs more CPU per resize than Nearest, but
/// a resize only happens once per image open/`Left`/`Right` switch, not
/// per frame, so it's not worth trading quality for.
pub fn draw_image_preview(frame: &mut Frame, area: Rect, state: &mut ImagePreviewState, theme: &Theme) {
    let title = state.current_path().file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.accent)).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let image = StatefulImage::default().resize(Resize::Fit(Some(FilterType::Lanczos3)));
    frame.render_stateful_widget(image, inner, state.protocol_mut());
}
