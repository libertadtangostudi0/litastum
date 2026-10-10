use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::FontSize;

/// Set by litastum's own window (`gui/`) for the console app it hosts:
/// the window's cell size in pixels, `"10x20"`. Its presence means "draw
/// images as iTerm2 inline images" -- the one graphics protocol ConPTY
/// passes through to the window. The image picker can't learn this by
/// asking: ConPTY answers the status query that ends its questions
/// before the window's cell-size reply arrives, and then falls back to
/// half-blocks. History: docs/history/launching.md.
pub(crate) const HOST_CELL_SIZE_ENV_VAR: &str = "LITASTUM_HOST_CELL_SIZE";


/// The host window's cell size, if litastum runs in it. Never in tests,
/// so the suite can't depend on how it was started.
pub(crate) fn host_cell_size() -> Option<(u16, u16)> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        parse_cell_size(&std::env::var(HOST_CELL_SIZE_ENV_VAR).ok()?)
    }
}


/// An image picker for the host window: iTerm2 inline images at its cell
/// size. `from_fontsize` is deprecated in favor of querying, which can't
/// work through ConPTY (`HOST_CELL_SIZE_ENV_VAR`).
pub(crate) fn hosted_picker(cell: (u16, u16)) -> Picker {
    #[allow(deprecated)]
    let mut picker = Picker::from_fontsize(FontSize::new(cell.0, cell.1));
    picker.set_protocol_type(ProtocolType::Iterm2);
    picker
}


/// The terminal user variable telling the window where the next image
/// goes, `"line,column"` (0-based cells).
pub(crate) const IMAGE_AT_VARIABLE: &str = "litastum_image_at";


/// Tells the window where the image drawn at `area`'s top-left goes,
/// right before it, in the same cell of `buffer`. The window used to put
/// an image at the cursor, but ConPTY forwards an image as soon as it
/// reads it and the cursor moves before it later, with its own redraw:
/// the image landed elsewhere and was wiped (reported: no previews in
/// the window). The two sequences go through ConPTY in order.
pub(crate) fn place_image_for_host(buffer: &mut Buffer, area: Rect) {
    if host_cell_size().is_none() {
        return;
    }
    mark_image_position(buffer, area);
}


fn mark_image_position(buffer: &mut Buffer, area: Rect) {
    let Some(cell) = buffer.cell_mut((area.x, area.y)) else {
        return;
    };
    if !cell.symbol().contains("\x1b]1337;File=") {
        return;
    }
    let position = format!("{},{}", area.y, area.x);
    let symbol = format!("\x1b]1337;SetUserVar={IMAGE_AT_VARIABLE}={}\x07{}", crate::event_loop::base64(position.as_bytes()), cell.symbol());
    cell.set_symbol(&symbol);
}


fn parse_cell_size(value: &str) -> Option<(u16, u16)> {
    let (width, height) = value.split_once('x')?;
    let size = (width.trim().parse().ok()?, height.trim().parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some(size)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_width_by_height_in_pixels() {
        assert_eq!(parse_cell_size("10x20"), Some((10, 20)));
        assert_eq!(parse_cell_size("10x0"), None);
        assert_eq!(parse_cell_size("ten"), None);
    }

    /// The window (`gui/src/session.rs`) sets the same name; they must
    /// agree.
    #[test]
    fn the_variable_name_matches_the_windows() {
        assert_eq!(HOST_CELL_SIZE_ENV_VAR, "LITASTUM_HOST_CELL_SIZE");
    }

    /// The window places the image by this, not by the cursor.
    #[test]
    fn an_image_cell_says_where_the_image_goes_first() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 10));
        buffer[(7, 3)].set_symbol("\x1b]1337;File=inline=1:AAAA\x07");
        buffer[(2, 2)].set_symbol("x");

        mark_image_position(&mut buffer, Rect::new(7, 3, 5, 5));
        mark_image_position(&mut buffer, Rect::new(2, 2, 5, 5));

        // "3,7" in base64.
        assert_eq!(buffer[(7, 3)].symbol(), "\x1b]1337;SetUserVar=litastum_image_at=Myw3\x07\x1b]1337;File=inline=1:AAAA\x07");
        assert_eq!(buffer[(2, 2)].symbol(), "x", "no image there");
    }

    #[test]
    fn the_hosted_picker_draws_iterm2_images() {
        assert_eq!(hosted_picker((11, 23)).protocol_type(), ProtocolType::Iterm2);
    }
}
