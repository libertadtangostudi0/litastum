/// Appends `c` to the typed command.
pub fn insert_char(line: &mut String, c: char) {
    line.push(c);
}

/// Removes the last character, if any. A no-op on an empty line.
pub fn backspace(line: &mut String) {
    line.pop();
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_char_appends() {
        let mut line = String::from("di");
        insert_char(&mut line, 'r');
        assert_eq!(line, "dir");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut line = String::from("dir");
        backspace(&mut line);
        assert_eq!(line, "di");
    }

    #[test]
    fn backspace_on_empty_line_is_a_noop() {
        let mut line = String::new();
        backspace(&mut line);
        assert_eq!(line, "");
    }
}
