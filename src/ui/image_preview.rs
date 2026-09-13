use ratatui::{layout::Rect, text::Line, Frame};
use ratatui_image::{FilterType, Resize, StatefulImage};

use crate::explorer::ImagePreviewState;
use crate::theming::Theme;
use crate::ui::preview::{draw_preview_frame, file_title};

/// Renders `F3`'s currently-previewed image into `area` -- replaces the
/// right panel's own file listing entirely while `Mode::ImagePreview` is
/// active (`ui::draw`'s own panel-drawing site), rather than overlaying
/// a popup on top of it. Border/title chrome comes from
/// `ui::preview::draw_preview_frame`, shared with
/// `ui::markdown_preview::draw_markdown_preview`.
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
    let title = file_title(state.current_path());
    let inner = draw_preview_frame(frame, area, theme, Line::raw(title), None);

    let image = StatefulImage::default().resize(Resize::Fit(Some(FilterType::Lanczos3)));
    frame.render_stateful_widget(image, inner, state.protocol_mut());
}
