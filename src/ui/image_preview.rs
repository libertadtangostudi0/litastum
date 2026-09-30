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

/// Draws the current image in place of the right panel's listing, in the
/// shared `preview::draw_preview_frame`. Resized with `Lanczos3`, not the
/// default `Nearest`, which aliased small text into blocky noise; it costs
/// more, but only once per image. `Loading`/`Failed` show only when there's
/// no previous image to keep on screen. History: docs/history/image-preview.md.
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
