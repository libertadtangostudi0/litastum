//! Terminal user variables (`OSC 1337 ; SetUserVar=name=<base64>`,
//! WezTerm's convention; other terminals ignore it): how litastum tells
//! its window what it can't learn otherwise -- the screen shown, for its
//! zoom (`event_loop::sync`), and where an image goes (`image_host`).
//! ConPTY passes them through. History: docs/history/launching.md.


/// The sequence setting user variable `name` to `value`.
pub(crate) fn set_user_var(name: &str, value: &str) -> String {
    format!("\x1b]1337;SetUserVar={name}={}\x07", base64(value.as_bytes()))
}


/// Standard base64, which `SetUserVar` wants its value in.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(ALPHABET[(value >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_go_out_in_base64() {
        assert_eq!(base64(b"main"), "bWFpbg==");
        assert_eq!(base64(b"compare"), "Y29tcGFyZQ==");
        assert_eq!(base64(b"conflict"), "Y29uZmxpY3Q=");
        assert_eq!(base64(b"3,7"), "Myw3");
    }

    #[test]
    fn a_user_variable_is_an_osc_1337() {
        assert_eq!(set_user_var("litastum_screen", "compare"), "\x1b]1337;SetUserVar=litastum_screen=Y29tcGFyZQ==\x07");
    }
}
