use std::fs;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::{App, Mode, Overlay};
use crate::test_support::unique_scratch_dir;

use super::links::{resolve_link_target, LinkTarget};
use super::render::render_markdown;
use super::*;

mod wrap_markdown_line_tests;
mod render_markdown_tests;
mod markdown_preview_state_tests;
mod open_edit_preview_tests;
mod handle_markdown_edit_preview_key_tests;
mod markdown_link_search_state_tests;
mod handle_markdown_link_search_key_tests;
mod resolve_link_target_tests;
mod handle_markdown_preview_mouse_tests;
