//! Locale identifiers (17.5, R332): a Unicode BCP 47 tag, canonicalized on parse. Shared by the
//! elaborator, which rejects a malformed literal at stage 0 (E550), and the interpreter.

/// The canonical form of `tag` (`en_us` is `en-US`), or why it is malformed.
pub fn canonical(tag: &str) -> Result<String, String> {
    if tag == "und" || tag == "root" {
        return Ok("und".into());
    }
    let parts: Vec<&str> = tag.split(['-', '_']).collect();
    let alnum =
        |s: &str| !s.is_empty() && s.len() <= 8 && s.chars().all(|c| c.is_ascii_alphanumeric());
    let lang = parts[0];
    if !(matches!(lang.len(), 2 | 3 | 5..=8) && lang.chars().all(|c| c.is_ascii_alphabetic())) {
        return Err(format!("`{lang}` is not a language subtag"));
    }
    let mut out = vec![lang.to_ascii_lowercase()];
    let mut extension = false;
    for p in &parts[1..] {
        if !alnum(p) {
            return Err(format!("`{p}` is not a subtag"));
        }
        let lower = p.to_ascii_lowercase();
        if extension || p.len() == 1 {
            extension = true;
            out.push(lower);
        } else if p.len() == 4 && p.chars().all(|c| c.is_ascii_alphabetic()) {
            let mut s = lower;
            s[..1].make_ascii_uppercase();
            out.push(s);
        } else if (p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
            || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()))
        {
            out.push(p.to_ascii_uppercase());
        } else {
            out.push(lower);
        }
    }
    Ok(out.join("-"))
}

#[cfg(test)]
mod tests {
    use super::canonical;

    #[test]
    fn tags_canonicalize_and_bad_ones_fail() {
        assert_eq!(canonical("en_us").as_deref(), Ok("en-US"));
        assert_eq!(canonical("zh-hant-tw").as_deref(), Ok("zh-Hant-TW"));
        assert_eq!(
            canonical("de-DE-u-co-phonebk").as_deref(),
            Ok("de-DE-u-co-phonebk")
        );
        assert!(canonical("en_US!!").is_err());
        assert!(canonical("e").is_err());
    }
}
