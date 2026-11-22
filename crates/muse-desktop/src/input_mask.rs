pub fn display(text: &str, secure: bool) -> String {
    if secure {
        "•".repeat(text.chars().count())
    } else {
        text.into()
    }
}
pub fn display_index(text: &str, byte: usize, secure: bool) -> usize {
    if secure {
        text.char_indices()
            .take_while(|(offset, _)| *offset < byte)
            .count()
            * 3
    } else {
        byte
    }
}
pub fn source_index(text: &str, byte: usize, secure: bool) -> usize {
    if secure {
        text.char_indices()
            .nth(byte / 3)
            .map(|(offset, _)| offset)
            .unwrap_or(text.len())
    } else {
        byte
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secure_text_hides_content_and_maps_unicode_caret_and_selection_positions() {
        let text = "a密🔒z";
        assert_eq!(display(text, true), "••••");
        assert_eq!(display_index(text, 4, true), 6);
        assert_eq!(source_index(text, 9, true), 8);
        assert_eq!(source_index(text, 12, true), 9);
        assert_eq!(display(text, false), text);
        assert_eq!(display_index(text, 4, false), 4);
    }
}
