pub(super) fn clipboard_get() -> Option<String> {
    crate::clipboard::get_text()
}

pub(super) fn clipboard_set(text: &str) {
    crate::clipboard::set_text(text);
}
