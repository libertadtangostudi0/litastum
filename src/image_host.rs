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

    #[test]
    fn the_hosted_picker_draws_iterm2_images() {
        assert_eq!(hosted_picker((11, 23)).protocol_type(), ProtocolType::Iterm2);
    }
}
