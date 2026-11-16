/// True when the text holds at least one CJK ideograph.
pub fn has_chinese(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_chinese() {
        assert!(has_chinese("需要奶妈"));
        assert!(!has_chinese("lf tank"));
        assert!(!has_chinese("こんにちは"));
    }
}
