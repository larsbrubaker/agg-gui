//! How a browser `paste` event becomes a paste the app sees: the decisions
//! behind `web_paste`'s DOM listener, kept free of `web_sys` so they are
//! unit-tested on every target.
//!
//! A paste event carries plain text (`clipboardData.getData("text/plain")`),
//! often an HTML flavour (`getData("text/html")`: rich text, or an app's own
//! payload riding next to its text) and a list of items (`clipboardData.items`), each with a `kind`
//! (`"string"` or `"file"`) and a MIME `type`. A copied picture is a
//! `"file"` item of an `image/*` type. Text can be handed to the app at
//! once; a picture must first be decoded, which the browser does
//! asynchronously so no frame waits on it. The paste is then delivered as
//! one unit (text, HTML and picture together, one synthesized `Ctrl+V`) when the
//! decode finishes, unless a newer paste came in meanwhile: only the newest
//! paste is ever delivered, so a slow decode can't land after, and
//! overwrite, the paste that followed it.

use crate::clipboard::ClipboardImage;

/// The index of the first item that is a picture file, given each item's
/// `(kind, type)`.
pub(crate) fn image_item_index<'a>(
    items: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<usize> {
    items
        .into_iter()
        .position(|(kind, mime)| kind.eq_ignore_ascii_case("file") && is_image_mime(mime))
}

/// `true` for an `image/<subtype>` MIME type, in any letter case.
fn is_image_mime(mime: &str) -> bool {
    mime.get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
        && mime.len() > 6
}

/// What to do with one paste event.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PastePlan {
    /// Nothing the app can paste: the event is left to the browser.
    Ignore,
    /// Text (and maybe HTML), no picture: deliver it now. `html` is empty
    /// when the paste carried none.
    Text { text: String, html: String },
    /// Decode the picture in item `index`, then hand the outcome, `text` and
    /// `html` to [`PasteSequencer::finish`] with `ticket`.
    DecodeImage {
        index: usize,
        ticket: u64,
        text: String,
        html: String,
    },
}

/// A paste ready for the app: the buffers to fill before the synthesized
/// `Ctrl+V`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PasteDelivery {
    /// The pasted plain text, empty when there was none.
    pub text: String,
    /// The pasted HTML flavour, empty when there was none.
    pub html: String,
    pub image: Option<ClipboardImage>,
}

/// Orders paste events so only the newest one is delivered.
#[derive(Debug, Default)]
pub(crate) struct PasteSequencer {
    latest: u64,
}

impl PasteSequencer {
    /// Plan a new paste event carrying `text`, `html` (each empty when the
    /// paste has none) and, when one of its items is a picture, that item's
    /// index. Every paste supersedes any picture still decoding for an
    /// earlier one, even one with nothing to paste ([`PastePlan::Ignore`]):
    /// the user's latest paste wins, so an earlier picture never lands after
    /// it. A paste with HTML but no plain text is still delivered: the HTML
    /// alone can be what the app pastes.
    pub(crate) fn plan(
        &mut self,
        text: String,
        html: String,
        image_index: Option<usize>,
    ) -> PastePlan {
        self.latest += 1;
        match image_index {
            Some(index) => PastePlan::DecodeImage {
                index,
                ticket: self.latest,
                text,
                html,
            },
            None if text.is_empty() && html.is_empty() => PastePlan::Ignore,
            None => PastePlan::Text { text, html },
        }
    }

    /// The decode for `ticket` has finished with `image` (`None` when the
    /// browser couldn't decode it). Returns the paste to deliver, or `None`
    /// when a newer paste superseded this one or there is nothing to paste.
    pub(crate) fn finish(
        &self,
        ticket: u64,
        text: String,
        html: String,
        image: Option<ClipboardImage>,
    ) -> Option<PasteDelivery> {
        if ticket != self.latest || (image.is_none() && text.is_empty() && html.is_empty()) {
            return None;
        }
        Some(PasteDelivery { text, html, image })
    }
}

#[cfg(test)]
mod tests {
    use super::{image_item_index, PasteDelivery, PastePlan, PasteSequencer};
    use crate::clipboard::ClipboardImage;

    fn pixel() -> ClipboardImage {
        ClipboardImage::new(1, 1, vec![10, 20, 30, 255]).expect("1x1 RGBA")
    }

    /// Plan a paste that is expected to wait for a picture decode, returning
    /// its ticket, text and HTML.
    fn decoding(
        sequencer: &mut PasteSequencer,
        text: &str,
        html: &str,
        index: usize,
    ) -> (u64, String, String) {
        match sequencer.plan(text.to_string(), html.to_string(), Some(index)) {
            PastePlan::DecodeImage {
                index: planned,
                ticket,
                text,
                html,
            } => {
                assert_eq!(planned, index, "the picture item is the one decoded");
                (ticket, text, html)
            }
            other => panic!("a picture item is decoded first, planned {other:?}"),
        }
    }

    #[test]
    fn picture_file_item_is_found() {
        let items = [("string", "text/plain"), ("file", "image/png")];
        assert_eq!(image_item_index(items), Some(1));
    }

    #[test]
    fn first_picture_wins_and_case_is_ignored() {
        let items = [
            ("file", "application/pdf"),
            ("FILE", "Image/JPEG"),
            ("file", "image/png"),
        ];
        assert_eq!(image_item_index(items), Some(1));
    }

    #[test]
    fn non_picture_items_are_not_pictures() {
        // An image MIME type on a string item is markup about a picture (a
        // browser's copied `<img>` tag), not the picture itself.
        let items = [
            ("string", "image/png"),
            ("file", "text/plain"),
            ("file", "image/"),
            ("file", "imagery/x"),
            ("file", ""),
        ];
        assert_eq!(image_item_index(items), None);
        assert_eq!(image_item_index([]), None);
    }

    #[test]
    fn text_paste_is_delivered_at_once() {
        let mut sequencer = PasteSequencer::default();
        assert_eq!(
            sequencer.plan("hello".to_string(), String::new(), None),
            PastePlan::Text {
                text: "hello".to_string(),
                html: String::new(),
            }
        );
    }

    #[test]
    fn text_paste_keeps_its_html_flavour() {
        let mut sequencer = PasteSequencer::default();
        assert_eq!(
            sequencer.plan(
                "part".to_string(),
                "<span data-part=\"AAEC\">part</span>".to_string(),
                None
            ),
            PastePlan::Text {
                text: "part".to_string(),
                html: "<span data-part=\"AAEC\">part</span>".to_string(),
            }
        );
    }

    #[test]
    fn html_only_paste_is_delivered() {
        let mut sequencer = PasteSequencer::default();
        assert_eq!(
            sequencer.plan(String::new(), "<b>rich</b>".to_string(), None),
            PastePlan::Text {
                text: String::new(),
                html: "<b>rich</b>".to_string(),
            }
        );
    }

    #[test]
    fn empty_paste_is_left_to_the_browser() {
        let mut sequencer = PasteSequencer::default();
        assert_eq!(
            sequencer.plan(String::new(), String::new(), None),
            PastePlan::Ignore
        );
    }

    #[test]
    fn picture_paste_waits_for_its_decode_and_keeps_its_text_and_html() {
        let mut sequencer = PasteSequencer::default();
        let (ticket, text, html) = decoding(&mut sequencer, "caption", "<img alt=caption>", 2);
        assert_eq!(
            sequencer.finish(ticket, text, html, Some(pixel())),
            Some(PasteDelivery {
                text: "caption".to_string(),
                html: "<img alt=caption>".to_string(),
                image: Some(pixel()),
            })
        );
    }

    #[test]
    fn picture_alone_is_delivered_with_empty_text() {
        let mut sequencer = PasteSequencer::default();
        let (ticket, text, html) = decoding(&mut sequencer, "", "", 0);
        assert_eq!(
            sequencer.finish(ticket, text, html, Some(pixel())),
            Some(PasteDelivery {
                text: String::new(),
                html: String::new(),
                image: Some(pixel()),
            })
        );
    }

    #[test]
    fn failed_decode_still_pastes_the_text() {
        let mut sequencer = PasteSequencer::default();
        let (ticket, text, html) = decoding(&mut sequencer, "words", "", 0);
        assert_eq!(
            sequencer.finish(ticket, text, html, None),
            Some(PasteDelivery {
                text: "words".to_string(),
                html: String::new(),
                image: None,
            })
        );
    }

    #[test]
    fn failed_decode_still_pastes_the_html() {
        let mut sequencer = PasteSequencer::default();
        let (ticket, text, html) = decoding(&mut sequencer, "", "<i>only</i>", 0);
        assert_eq!(
            sequencer.finish(ticket, text, html, None),
            Some(PasteDelivery {
                text: String::new(),
                html: "<i>only</i>".to_string(),
                image: None,
            })
        );
    }

    #[test]
    fn failed_decode_without_text_pastes_nothing() {
        let mut sequencer = PasteSequencer::default();
        let (ticket, text, html) = decoding(&mut sequencer, "", "", 0);
        assert_eq!(sequencer.finish(ticket, text, html, None), None);
    }

    #[test]
    fn newer_paste_supersedes_a_picture_still_decoding() {
        let mut sequencer = PasteSequencer::default();
        let (ticket, text, html) = decoding(&mut sequencer, "", "<p>old</p>", 0);
        assert_eq!(
            sequencer.plan("later".to_string(), String::new(), None),
            PastePlan::Text {
                text: "later".to_string(),
                html: String::new(),
            }
        );
        assert_eq!(sequencer.finish(ticket, text, html, Some(pixel())), None);
    }

    #[test]
    fn empty_paste_also_supersedes_a_picture_still_decoding() {
        let mut sequencer = PasteSequencer::default();
        let (ticket, text, html) = decoding(&mut sequencer, "", "", 0);
        assert_eq!(
            sequencer.plan(String::new(), String::new(), None),
            PastePlan::Ignore
        );
        assert_eq!(sequencer.finish(ticket, text, html, Some(pixel())), None);
    }

    #[test]
    fn only_the_newest_of_two_picture_pastes_is_delivered() {
        let mut sequencer = PasteSequencer::default();
        let (first, first_text, first_html) = decoding(&mut sequencer, "", "", 0);
        let (second, second_text, second_html) = decoding(&mut sequencer, "", "", 0);
        // The second decode may finish first; the first must not land after it.
        assert!(sequencer
            .finish(second, second_text, second_html, Some(pixel()))
            .is_some());
        assert_eq!(
            sequencer.finish(first, first_text, first_html, Some(pixel())),
            None
        );
    }
}
