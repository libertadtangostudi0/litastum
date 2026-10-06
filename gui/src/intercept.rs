/// What the program's output splits into: text for the terminal engine,
/// and iTerm2 inline images (`OSC 1337 ; File=...`), which
/// `alacritty_terminal` ignores and the window draws itself.
#[derive(Debug, PartialEq, Eq)]
pub enum Segment {
    Bytes(Vec<u8>),
    /// Everything after `File=` up to the terminator: arguments, `:`, and
    /// the base64 payload.
    InlineImage(Vec<u8>),
}

const IMAGE_START: &[u8] = b"\x1b]1337;File=";
/// An image larger than this is dropped rather than buffered forever.
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;


/// Splits the output stream into `Segment`s, across reads: a start marker
/// or an image cut between two reads is held until the rest arrives.
#[derive(Default)]
pub struct Interceptor {
    /// A possible start marker cut off at the end of the last read.
    pending: Vec<u8>,
    /// An image whose terminator hasn't arrived yet.
    image: Option<Vec<u8>>,
}

impl Interceptor {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(bytes);
        let mut position = 0;

        if let Some(mut image) = self.image.take() {
            match terminator(&data) {
                Some((end, after)) => {
                    image.extend_from_slice(&data[..end]);
                    segments.push(Segment::InlineImage(image));
                    position = after;
                }
                None => {
                    image.extend_from_slice(&data);
                    if image.len() <= MAX_IMAGE_BYTES {
                        self.image = Some(image);
                    }
                    return segments;
                }
            }
        }

        let mut text_start = position;
        while let Some(offset) = data[position..].iter().position(|&byte| byte == 0x1b) {
            let escape = position + offset;
            let rest = &data[escape..];
            if rest.starts_with(IMAGE_START) {
                push_bytes(&mut segments, &data[text_start..escape]);
                let body = &data[escape + IMAGE_START.len()..];
                match terminator(body) {
                    Some((end, after)) => {
                        segments.push(Segment::InlineImage(body[..end].to_vec()));
                        position = escape + IMAGE_START.len() + after;
                        text_start = position;
                    }
                    None => {
                        self.image = Some(body.to_vec());
                        return segments;
                    }
                }
            } else if IMAGE_START.starts_with(rest) {
                // Possibly a start marker cut off by the read.
                push_bytes(&mut segments, &data[text_start..escape]);
                self.pending = rest.to_vec();
                return segments;
            } else {
                position = escape + 1;
            }
        }
        push_bytes(&mut segments, &data[text_start..]);
        segments
    }
}


fn push_bytes(segments: &mut Vec<Segment>, bytes: &[u8]) {
    if !bytes.is_empty() {
        segments.push(Segment::Bytes(bytes.to_vec()));
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

    fn bytes(text: &[u8]) -> Segment {
        Segment::Bytes(text.to_vec())
    }

    fn image(body: &[u8]) -> Segment {
        Segment::InlineImage(body.to_vec())
    }

    #[test]
    fn text_without_images_passes_through_whole() {
        let mut interceptor = Interceptor::default();
        assert_eq!(interceptor.feed(b"ab\x1b[31mc"), [bytes(b"ab\x1b[31mc")]);
    }

    #[test]
    fn an_image_is_cut_out_between_text() {
        let mut interceptor = Interceptor::default();
        assert_eq!(
            interceptor.feed(b"ab\x1b]1337;File=inline=1:QUJD\x07cd\x1b]1337;File=x:Rg==\x1b\\e"),
            [bytes(b"ab"), image(b"inline=1:QUJD"), bytes(b"cd"), image(b"x:Rg=="), bytes(b"e")]
        );
    }

    #[test]
    fn an_image_split_across_reads_is_put_together() {
        let mut interceptor = Interceptor::default();
        assert_eq!(interceptor.feed(b"ab\x1b]13"), [bytes(b"ab")], "a cut start marker waits");
        assert_eq!(interceptor.feed(b"37;File=inline=1:QU"), []);
        assert_eq!(interceptor.feed(b"JD\x07cd"), [image(b"inline=1:QUJD"), bytes(b"cd")]);
    }

    #[test]
    fn other_osc_sequences_stay_text() {
        let mut interceptor = Interceptor::default();
        assert_eq!(interceptor.feed(b"\x1b]0;title\x07x"), [bytes(b"\x1b]0;title\x07x")]);
        assert_eq!(interceptor.feed(b"\x1b]1"), [], "could still become an image");
        assert_eq!(interceptor.feed(b"0;rgb:00/00/00\x07"), [bytes(b"\x1b]10;rgb:00/00/00\x07")]);
    }
}
