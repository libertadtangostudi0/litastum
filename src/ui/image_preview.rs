use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use ratatui_image::{FilterType, Resize, StatefulImage};

use crate::explorer::{ImagePreviewState, PreviewFrame};
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
///
/// `state.frame_mut()` (`explorer::PreviewFrame`) can be `Loading` or
/// `Failed` now, not just a ready protocol -- decoding moved to a
/// background thread (`ImagePreviewState`'s own doc comment) once both
/// the very first `F3` open and every `Left`/`Right` switch were
/// reported as blocking the whole UI for however long a real photo
/// took to decode/resize. `Loading` only ever shows for the very first
/// image in a session (`Left`/`Right` keeps the previous image's own
/// pixels on screen while the next one decodes, so switching itself
/// never shows this) or right after a decode failure with nothing
/// earlier to fall back to.
pub fn draw_image_preview(frame: &mut Frame, area: Rect, state: &mut ImagePreviewState, theme: &Theme) {
    let title = file_title(state.current_path());
    let inner = draw_preview_frame(frame, area, theme, Line::raw(title), None);

    match state.frame_mut() {
        PreviewFrame::Ready(protocol) => {
            let image = StatefulImage::default().resize(Resize::Fit(Some(FilterType::Lanczos3)));
            frame.render_stateful_widget(image, inner, protocol);
        }
        PreviewFrame::Loading => {
            frame.render_widget(Paragraph::new(Line::from(Span::styled("Loading...", Style::default().fg(theme.text_dim)))), inner);
        }
        PreviewFrame::Failed => {
            frame.render_widget(Paragraph::new(Line::from(Span::styled("Failed to load image", Style::default().fg(theme.danger)))), inner);
        }
    }
}
