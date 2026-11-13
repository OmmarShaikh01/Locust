use gpui::Keystroke;

pub fn key_to_char(keystroke: &Keystroke) -> Option<String> {
    if let Some(ch) = &keystroke.key_char {
        if !ch.is_empty() && !ch.chars().any(|c| c.is_control()) {
            return Some(ch.clone());
        }
    }

    if keystroke.modifiers.control || keystroke.modifiers.alt || keystroke.modifiers.platform {
        return None;
    }

    match keystroke.key.as_str() {
        "space" => Some(" ".to_string()),
        "minus" => Some(if keystroke.modifiers.shift {
            "_".to_string()
        } else {
            "-".to_string()
        }),
        "equal" => Some(if keystroke.modifiers.shift {
            "+".to_string()
        } else {
            "=".to_string()
        }),
        "comma" => Some(if keystroke.modifiers.shift {
            "<".to_string()
        } else {
            ",".to_string()
        }),
        "period" => Some(if keystroke.modifiers.shift {
            ">".to_string()
        } else {
            ".".to_string()
        }),
        "slash" => Some(if keystroke.modifiers.shift {
            "?".to_string()
        } else {
            "/".to_string()
        }),
        "backslash" => Some(if keystroke.modifiers.shift {
            "|".to_string()
        } else {
            "\\".to_string()
        }),
        "semicolon" => Some(if keystroke.modifiers.shift {
            ":".to_string()
        } else {
            ";".to_string()
        }),
        "quote" => Some(if keystroke.modifiers.shift {
            "\"".to_string()
        } else {
            "'".to_string()
        }),
        "backquote" | "grave" => Some(if keystroke.modifiers.shift {
            "~".to_string()
        } else {
            "`".to_string()
        }),
        "bracketleft" => Some(if keystroke.modifiers.shift {
            "{".to_string()
        } else {
            "[".to_string()
        }),
        "bracketright" => Some(if keystroke.modifiers.shift {
            "}".to_string()
        } else {
            "]".to_string()
        }),
        k if k.chars().count() == 1 => {
            let c = k.chars().next().unwrap();
            if keystroke.modifiers.shift {
                if c.is_ascii_lowercase() {
                    Some(c.to_ascii_uppercase().to_string())
                } else {
                    match c {
                        '1' => Some("!".to_string()),
                        '2' => Some("@".to_string()),
                        '3' => Some("#".to_string()),
                        '4' => Some("$".to_string()),
                        '5' => Some("%".to_string()),
                        '6' => Some("^".to_string()),
                        '7' => Some("&".to_string()),
                        '8' => Some("*".to_string()),
                        '9' => Some("(".to_string()),
                        '0' => Some(")".to_string()),
                        '-' => Some("_".to_string()),
                        '=' => Some("+".to_string()),
                        '[' => Some("{".to_string()),
                        ']' => Some("}".to_string()),
                        '\\' => Some("|".to_string()),
                        ';' => Some(":".to_string()),
                        '\'' => Some("\"".to_string()),
                        ',' => Some("<".to_string()),
                        '.' => Some(">".to_string()),
                        '/' => Some("?".to_string()),
                        '`' => Some("~".to_string()),
                        other => Some(other.to_string()),
                    }
                }
            } else {
                Some(k.to_string())
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_to_char_ascii_letters() {
        let mut k = Keystroke::default();
        k.key = "a".to_string();
        assert_eq!(key_to_char(&k), Some("a".to_string()));

        k.modifiers.shift = true;
        assert_eq!(key_to_char(&k), Some("A".to_string()));
    }

    #[test]
    fn test_key_to_char_space() {
        let mut k = Keystroke::default();
        k.key = "space".to_string();
        assert_eq!(key_to_char(&k), Some(" ".to_string()));
    }

    #[test]
    fn test_key_to_char_numbers_and_symbols() {
        let mut k = Keystroke::default();
        k.key = "1".to_string();
        assert_eq!(key_to_char(&k), Some("1".to_string()));

        k.modifiers.shift = true;
        assert_eq!(key_to_char(&k), Some("!".to_string()));

        let mut k2 = Keystroke::default();
        k2.key = "minus".to_string();
        assert_eq!(key_to_char(&k2), Some("-".to_string()));
        k2.modifiers.shift = true;
        assert_eq!(key_to_char(&k2), Some("_".to_string()));
    }

    #[test]
    fn test_key_to_char_with_key_char_populated() {
        let mut k = Keystroke::default();
        k.key = "s".to_string();
        k.key_char = Some("s".to_string());
        assert_eq!(key_to_char(&k), Some("s".to_string()));
    }

    #[test]
    fn test_key_to_char_control_keys_ignored() {
        let mut k = Keystroke::default();
        k.key = "c".to_string();
        k.modifiers.control = true;
        assert_eq!(key_to_char(&k), None);

        let mut k2 = Keystroke::default();
        k2.key = "v".to_string();
        k2.modifiers.alt = true;
        assert_eq!(key_to_char(&k2), None);
    }
}
