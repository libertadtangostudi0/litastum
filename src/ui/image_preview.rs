use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use ratatui_image::Image;

use crate::explorer::{ImagePreviewState, PreviewFrame};
use crate::theming::Theme;
use crate::ui::preview::{draw_preview_frame, file_title};

/// Draws the current image in place of the right panel's listing, in the
/// shared `preview::draw_preview_frame`, as prepared for this size off the
/// UI thread (`ImagePreviewState::set_area`). `Loading`/`Failed` show only
/// when there's no previous image to keep on screen. History:
/// docs/history/image-preview.md.
pub fn draw_image_preview(frame: &mut Frame, area: Rect, state: &mut ImagePreviewState, theme: &Theme) {
    let title = file_title(state.current_path());
    let inner = draw_preview_frame(frame, area, theme, Line::raw(title), None);

    state.set_area(inner.as_size());
    match state.frame_mut() {
        PreviewFrame::Ready(protocol) => {
            frame.render_widget(Image::new(protocol), inner);
            crate::image_host::place_image_for_host(frame.buffer_mut(), inner);
        }
        PreviewFrame::Loading => {
            frame.render_widget(Paragraph::new(Line::from(Span::styled("Loading...", Style::default().fg(theme.text_dim)))), inner);
        }
        PreviewFrame::Failed => {
            frame.render_widget(Paragraph::new(Line::from(Span::styled("Failed to load image", Style::default().fg(theme.danger)))), inner);
        }
    }
}
