/// A short message shown as a toast in the bottom-right corner
/// (`ui::notice`) until the next key press, which still does its usual
/// job. For outcomes that used to be only logged: a failed delete, save
/// or clipboard write. `Overlay::Info` stays for messages that must be
/// acknowledged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Error,
}


impl Notice {
    pub fn info(text: impl Into<String>) -> Self {
        Self { kind: NoticeKind::Info, text: text.into() }
    }


    pub fn error(text: impl Into<String>) -> Self {
        Self { kind: NoticeKind::Error, text: text.into() }
    }
}
