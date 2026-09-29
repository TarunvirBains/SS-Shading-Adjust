//! Path globs over `/`-separated resource paths: `*` and `?` stay within a segment, `**` crosses
//! segments.

pub fn matches(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p {
            [] => s.is_empty(),
            [b'*', b'*', rest @ ..] => {
                let rest = rest.strip_prefix(b"/").unwrap_or(rest);
                (0..=s.len()).any(|i| go(rest, &s[i..]))
            }
            [b'*', rest @ ..] => (0..=s.len())
                .take_while(|&i| i == 0 || s[i - 1] != b'/')
                .any(|i| go(rest, &s[i..])),
            [b'?', rest @ ..] => !s.is_empty() && s[0] != b'/' && go(rest, &s[1..]),
            [c, rest @ ..] => !s.is_empty() && s[0] == *c && go(rest, &s[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

#[cfg(test)]
mod tests {
    #[test]
    fn globs() {
        assert!(super::matches(
            "scenes/*/spot04_scene/**",
            "scenes/shared/spot04_scene/a"
        ));
        assert!(!super::matches("scenes/*/x", "scenes/a/b/x"));
    }
}
