//! Mask secrets (API keys, tokens, env secrets) in text.

const MASK: &str = "[REDACTED]";

fn is_tok(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+' | '=')
}

fn secret_token(w: &str) -> bool {
    if w.strip_prefix("sk-").is_some_and(|r| r.len() >= 8) {
        return true;
    }
    if w.strip_prefix("ghp_").is_some_and(|r| r.len() >= 20) {
        return true;
    }
    let b = w.as_bytes();
    if b.len() >= 10 && w.starts_with("xox") && b[3].is_ascii_alphabetic() && b[4] == b'-' {
        return true;
    }
    if let Some(r) = w.strip_prefix("AKIA") {
        return r.len() >= 16
            && r.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    }
    false
}

fn env_line(line: &str) -> Option<String> {
    let body = line.trim_end_matches(['\r', '\n']);
    let tail = &line[body.len()..];
    let lead = body.len() - body.trim_start().len();
    let rest = &body[lead..];
    let rest_noexp = rest.strip_prefix("export ").unwrap_or(rest);
    let skipped = rest.len() - rest_noexp.len();
    let (key, val) = rest_noexp.split_once('=')?;
    let valid = !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !key.starts_with(|c: char| c.is_ascii_digit());
    let up = key.to_ascii_uppercase();
    let secret = ["KEY", "TOKEN", "SECRET", "PASSWORD"]
        .iter()
        .any(|k| up.contains(k));
    if !(valid && secret) || val.trim().is_empty() {
        return None;
    }
    Some(format!(
        "{}{}{}={}{}",
        &body[..lead],
        &rest[..skipped],
        key,
        MASK,
        tail
    ))
}

fn redact_line(line: &str) -> String {
    if let Some(s) = env_line(line) {
        return s;
    }
    let mut out = String::with_capacity(line.len());
    let mut bearer = false; // previous word was "Bearer"
    let mut it = line.char_indices().peekable();
    while let Some(&(i, c)) = it.peek() {
        if is_tok(c) {
            let mut end = i;
            while let Some(&(j, d)) = it.peek() {
                if !is_tok(d) {
                    break;
                }
                end = j + d.len_utf8();
                it.next();
            }
            let w = &line[i..end];
            let core = w.trim_end_matches('.');
            if secret_token(core) || (bearer && core.len() >= 8) {
                out.push_str(MASK);
                out.push_str(&w[core.len()..]);
            } else {
                out.push_str(w);
            }
            bearer = w.eq_ignore_ascii_case("bearer");
        } else {
            if !c.is_whitespace() {
                bearer = false;
            }
            out.push(c);
            it.next();
        }
    }
    out
}

/// Replace secrets in `text` with `[REDACTED]`.
pub fn redact(text: &str) -> String {
    text.split_inclusive('\n').map(redact_line).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sk_key() {
        assert_eq!(redact("key sk-abcdef1234567890 end"), "key [REDACTED] end");
    }

    #[test]
    fn github_token() {
        let t = format!("ghp_{}", "a1B2".repeat(9));
        assert_eq!(redact(&format!("use {t}")), "use [REDACTED]");
        assert_eq!(redact(&format!("({t})")), "([REDACTED])");
    }

    #[test]
    fn slack_token() {
        assert_eq!(redact("xoxb-1234-abcd-efgh"), "[REDACTED]");
    }

    #[test]
    fn aws_key() {
        assert_eq!(redact("id AKIAIOSFODNN7EXAMPLE."), "id [REDACTED].");
    }

    #[test]
    fn bearer() {
        assert_eq!(
            redact("Authorization: Bearer abc.def.ghi123"),
            "Authorization: Bearer [REDACTED]"
        );
    }

    #[test]
    fn env_lines() {
        assert_eq!(
            redact("API_KEY=hunter2\nname=bob\n"),
            "API_KEY=[REDACTED]\nname=bob\n"
        );
        assert_eq!(redact("export GH_TOKEN=abc"), "export GH_TOKEN=[REDACTED]");
        assert_eq!(redact("DB_PASSWORD=x\r\n"), "DB_PASSWORD=[REDACTED]\r\n");
    }

    #[test]
    fn plain_unchanged() {
        for s in [
            "hello world",
            "sk-short",
            "task-list ghp_ AKIA",
            "Bearer",
            "a bearer of news",
            "path=/usr/bin\nline two\n",
            "unicode caf\u{e9} ok",
            "",
        ] {
            assert_eq!(redact(s), s);
        }
    }
}
