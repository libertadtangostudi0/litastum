/// Cuts iTerm2 inline images (`OSC 1337 ; File=...`) out of the
/// program's output, which `alacritty_terminal` would ignore and the
/// window draws itself; everything else goes on to the terminal engine.
/// Works across reads: a start marker or an image cut between two reads
/// is held until the rest arrives. Text is handed over as slices of the
/// read, without copying, except around a cut.
#[derive(Default)]
pub struct Interceptor {
    /// A possible start marker cut off at the end of the last read.
    pending: Vec<u8>,
    state: State,
}

#[derive(Default)]
enum State {
    #[default]
    Text,
    /// Inside an image whose terminator hasn't arrived yet.
    Image(Vec<u8>),
    /// Inside an image too large to keep: dropped up to its terminator,
    /// rather than letting its payload through as text.
    Skipping,
}

const IMAGE_START: &[u8] = b"\x1b]1337;File=";
/// An image body larger than this is dropped.
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;


impl Interceptor {
    /// Feeds one read; `text` gets the terminal's bytes in order,
    /// `image` each complete image body (everything after `File=`).
    pub fn feed(&mut self, bytes: &[u8], mut text: impl FnMut(&[u8]), mut image: impl FnMut(Vec<u8>)) {
        if self.pending.is_empty() {
            self.feed_data(bytes, &mut text, &mut image);
        } else {
            let mut data = std::mem::take(&mut self.pending);
            data.extend_from_slice(bytes);
            self.feed_data(&data, &mut text, &mut image);
        }
    }

    fn feed_data(&mut self, data: &[u8], text: &mut impl FnMut(&[u8]), image: &mut impl FnMut(Vec<u8>)) {
        let mut position = self.finish_image(data, image);
        let mut text_start = position;
        while let Some(offset) = data[position..].iter().position(|&byte| byte == 0x1b) {
            let escape = position + offset;
            let rest = &data[escape..];
            if rest.starts_with(IMAGE_START) {
                flush(text, &data[text_start..escape]);
                self.state = State::Image(Vec::new());
                position = escape + IMAGE_START.len();
                position += self.finish_image(&data[position..], image);
                if !matches!(self.state, State::Text) {
                    return;
                }
                text_start = position;
            } else if IMAGE_START.starts_with(rest) {
                // Possibly a start marker cut off by the read.
                flush(text, &data[text_start..escape]);
                self.pending = rest.to_vec();
                return;
            } else {
                position = escape + 1;
            }
        }
        flush(text, &data[text_start..]);
    }

    /// Continues an image in progress with `data`: hands it over and
    /// returns where the text after its terminator starts, or keeps it
    /// all and returns `data.len()`. Nothing to do outside an image.
    fn finish_image(&mut self, data: &[u8], image: &mut impl FnMut(Vec<u8>)) -> usize {
        let state = std::mem::take(&mut self.state);
        let end = terminator(data);
        match (state, end) {
            (State::Text, _) => 0,
            (State::Image(mut body), Some((end, after))) => {
                body.extend_from_slice(&data[..end]);
                image(body);
                after
            }
            (State::Image(mut body), None) => {
                let (data, cut) = self.hold_cut_terminator(data);
                if body.len() + data.len() > MAX_IMAGE_BYTES {
                    self.state = State::Skipping;
                } else {
                    body.extend_from_slice(data);
                    self.state = State::Image(body);
                }
                data.len() + cut
            }
            (State::Skipping, Some((_, after))) => after,
            (State::Skipping, None) => {
                let (data, cut) = self.hold_cut_terminator(data);
                self.state = State::Skipping;
                data.len() + cut
            }
        }
    }
}


impl Interceptor {
    /// An `ESC` ending the read may be the first half of the `ESC \`
    /// terminator: held for the next read instead of going into the body,
    /// where the terminator would never be found. Returns the rest of
    /// `data` and how many bytes were held.
    fn hold_cut_terminator<'a>(&mut self, data: &'a [u8]) -> (&'a [u8], usize) {
        match data.split_last() {
            Some((0x1b, rest)) => {
                self.pending = vec![0x1b];
                (rest, 1)
            }
            _ => (data, 0),
        }
    }
}


fn flush(text: &mut impl FnMut(&[u8]), bytes: &[u8]) {
    if !bytes.is_empty() {
        text(bytes);
    }
}


/// Where an OSC ends: `BEL` or `ESC \`. Returns the body's end and the
/// index just past the terminator.
fn terminator(body: &[u8]) -> Option<(usize, usize)> {
    body.iter().enumerate().find_map(|(index, &byte)| match byte {
        0x07 => Some((index, index + 1)),
        0x1b if body.get(index + 1) == Some(&b'\\') => Some((index, index + 2)),
        _ => None,
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    /// What came out, in order: text runs merged, images marked.
    #[derive(Debug, PartialEq, Eq)]
    enum Out {
        Text(Vec<u8>),
        Image(Vec<u8>),
    }

    fn feed(interceptor: &mut Interceptor, bytes: &[u8]) -> Vec<Out> {
        let out = std::cell::RefCell::new(Vec::new());
        interceptor.feed(
            bytes,
            |text| {
                let mut out = out.borrow_mut();
                match out.last_mut() {
                    Some(Out::Text(previous)) => previous.extend_from_slice(text),
                    _ => out.push(Out::Text(text.to_vec())),
                }
            },
            |image| out.borrow_mut().push(Out::Image(image)),
        );
        out.into_inner()
    }

    fn text(bytes: &[u8]) -> Out {
        Out::Text(bytes.to_vec())
    }

    fn image(body: &[u8]) -> Out {
        Out::Image(body.to_vec())
    }

    #[test]
    fn text_without_images_passes_through_whole() {
        let mut interceptor = Interceptor::default();
        assert_eq!(feed(&mut interceptor, b"ab\x1b[31mc"), [text(b"ab\x1b[31mc")]);
    }

    #[test]
    fn an_image_is_cut_out_between_text() {
        let mut interceptor = Interceptor::default();
        assert_eq!(
            feed(&mut interceptor, b"ab\x1b]1337;File=inline=1:QUJD\x07cd\x1b]1337;File=x:Rg==\x1b\\e"),
            [text(b"ab"), image(b"inline=1:QUJD"), text(b"cd"), image(b"x:Rg=="), text(b"e")]
        );
    }

    #[test]
    fn an_image_split_across_reads_is_put_together() {
        let mut interceptor = Interceptor::default();
        assert_eq!(feed(&mut interceptor, b"ab\x1b]13"), [text(b"ab")], "a cut start marker waits");
        assert_eq!(feed(&mut interceptor, b"37;File=inline=1:QU"), []);
        assert_eq!(feed(&mut interceptor, b"JD\x07cd"), [image(b"inline=1:QUJD"), text(b"cd")]);
    }

    #[test]
    fn a_terminator_split_across_reads() {
        let mut interceptor = Interceptor::default();
        assert_eq!(feed(&mut interceptor, b"\x1b]1337;File=x:QQ==\x1b"), []);
        assert_eq!(feed(&mut interceptor, b"\\z"), [image(b"x:QQ=="), text(b"z")]);
    }

    #[test]
    fn other_osc_sequences_stay_text() {
        let mut interceptor = Interceptor::default();
        assert_eq!(feed(&mut interceptor, b"\x1b]0;title\x07x"), [text(b"\x1b]0;title\x07x")]);
        assert_eq!(feed(&mut interceptor, b"\x1b]1"), [], "could still become an image");
        assert_eq!(feed(&mut interceptor, b"0;rgb:00/00/00\x07"), [text(b"\x1b]10;rgb:00/00/00\x07")]);
    }

    /// An image over the size limit used to be dropped mid-stream, and the
    /// rest of its base64 payload then printed as text.
    #[test]
    fn an_oversized_image_is_skipped_to_its_end_not_printed() {
        let mut interceptor = Interceptor::default();
        assert_eq!(feed(&mut interceptor, b"a\x1b]1337;File=x:"), [text(b"a")]);
        let chunk = vec![b'Q'; MAX_IMAGE_BYTES / 2 + 1];
        assert_eq!(feed(&mut interceptor, &chunk), []);
        assert_eq!(feed(&mut interceptor, &chunk), [], "now over the limit");
        assert_eq!(feed(&mut interceptor, b"QQQ\x07b"), [text(b"b")], "skipped to the terminator, nothing printed");
    }
}
