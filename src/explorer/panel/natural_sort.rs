use std::cmp::Ordering;

/// Case-insensitive, with digit runs compared as numbers (`2.txt` before
/// `10.txt`, like Far). Walks both strings together; each numeric segment
/// of `v2.1.3` compares on its own.
pub(super) fn natural_compare(a: &str, b: &str) -> Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();

    loop {
        return match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(ac), Some(bc)) if ac.is_ascii_digit() && bc.is_ascii_digit() => {
                let a_digits = take_digits(&mut a_chars);
                let b_digits = take_digits(&mut b_chars);
                match compare_digit_runs(&a_digits, &b_digits) {
                    Ordering::Equal => continue,
                    other => other,
                }
            }
            (Some(ac), Some(bc)) => match ac.to_ascii_lowercase().cmp(&bc.to_ascii_lowercase()) {
                Ordering::Equal => {
                    a_chars.next();
                    b_chars.next();
                    continue;
                }
                other => other,
            },
        };
    }
}

/// Consumes and returns the run of ASCII digits at the front of `chars`
/// (already confirmed non-empty by the caller).
fn take_digits(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut digits = String::new();
    while let Some(&c) = chars.peek() {
        if !c.is_ascii_digit() {
            break;
        }
        digits.push(c);
        chars.next();
    }
    digits
}

/// Compares two digit runs by value without parsing (no overflow on a huge
/// run): leading zeros trimmed, then longer is larger and equal lengths
/// compare lexicographically.
fn compare_digit_runs(a: &str, b: &str) -> Ordering {
    let a_trimmed = a.trim_start_matches('0');
    let b_trimmed = b.trim_start_matches('0');
    a_trimmed.len().cmp(&b_trimmed.len()).then_with(|| a_trimmed.cmp(b_trimmed))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digit_runs_compare_numerically_not_lexicographically() {
        let mut names = vec!["10.txt", "2.txt", "1.txt", "100.txt", "11.txt"];
        names.sort_by(|a, b| natural_compare(a, b));
        assert_eq!(names, vec!["1.txt", "2.txt", "10.txt", "11.txt", "100.txt"]);
    }

    #[test]
    fn plain_text_still_compares_case_insensitively() {
        assert_eq!(natural_compare("Banana", "apple"), Ordering::Greater);
        assert_eq!(natural_compare("apple", "Apple"), Ordering::Equal);
    }

    #[test]
    fn leading_zeros_compare_by_value_not_digit_count() {
        assert_eq!(natural_compare("007", "7"), Ordering::Equal);
        assert_eq!(natural_compare("007", "8"), Ordering::Less);
    }

    #[test]
    fn multiple_numeric_segments_each_compare_independently() {
        let mut names = vec!["v2.10.0", "v2.2.0", "v10.1.0"];
        names.sort_by(|a, b| natural_compare(a, b));
        assert_eq!(names, vec!["v2.2.0", "v2.10.0", "v10.1.0"]);
    }

    #[test]
    fn shorter_prefix_of_a_longer_string_sorts_first() {
        assert_eq!(natural_compare("file", "file2"), Ordering::Less);
    }
}
