//! The rich editor's image cache — a port of agg-sharp's `RichImageCache.cs`.
//!
//! Loads the images a rich document's image atoms point at, one load per url,
//! through the markdown viewer's own loading: the host's image provider (the
//! viewer's `with_image_provider` contract: RGBA8, top row first) and, for an
//! http(s) url the provider does not supply, the viewer's remote fetcher
//! (`markdown::image_loader`).  The edit widget hands [`RichImageCache::loaded`]
//! to its painter and layout style.
//!
//! Differences from C#, where a loaded image raised a callback that the
//! document view turned into a relayout on the UI thread:
//! - A fetch completes on a worker, so the cache keeps each url's load state
//!   behind a mutex and the widget polls it: [`RichImageCache::take_arrived`]
//!   hands back each url whose image has arrived since the last call, once,
//!   and the widget re-lays out the blocks showing it (C#'s `ImageLoaded`).
//! - [`RichImageCache::loading`] tells the widget to keep drawing while a
//!   fetch is in flight, so the arrival is noticed (agg-gui's `needs_draw`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pulldown_cmark::{Event, Parser, Tag};

use super::block_painter::RichImage;
use super::inline::InlineAtom;
use crate::widgets::markdown::image_loader::load_remote_image;
use crate::widgets::markdown::{is_fetchable_url, ImagePixels, ImageState};

/// Supplies an image's RGBA8 pixels (top row first) and size, or `None`.
pub type RichImageProvider = Box<dyn Fn(&str) -> Option<(Vec<u8>, u32, u32)>>;

/// Maps an image's url as written in the markdown to what the loader fetches.
pub type RichResolveUrl = Box<dyn Fn(&str) -> String>;

struct Load {
    state: Arc<Mutex<ImageState>>,
    /// Whether the arrival has been handed to the widget.
    reported: bool,
}

/// The rich editor's image cache (C#'s `RichImageCache`).
#[derive(Default)]
pub struct RichImageCache {
    url_by_markdown: RefCell<HashMap<String, String>>,
    loads: RefCell<HashMap<String, Load>>,
    provider: Option<RichImageProvider>,
    resolve_url: Option<RichResolveUrl>,
}

impl RichImageCache {
    /// An empty cache with no provider: only http(s) images load.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets where images come from (the viewer's image provider contract).
    pub fn set_provider(&mut self, provider: Option<RichImageProvider>) {
        self.provider = provider;
    }

    /// Sets how an image's markdown url maps to what the loader fetches (a
    /// relative path to a file or an asset url); C#'s `ResolveUrl`.
    pub fn set_resolve_url(&mut self, resolve: Option<RichResolveUrl>) {
        self.resolve_url = resolve;
    }

    /// The image atom's url, read with the markdown parser so titles, angle
    /// brackets and escapes come out as the viewer reads them.
    pub fn url_of(&self, atom: &InlineAtom) -> String {
        if let Some(url) = self.url_by_markdown.borrow().get(&atom.raw_markdown) {
            return url.clone();
        }
        let url = Parser::new(&atom.raw_markdown)
            .find_map(|event| match event {
                Event::Start(Tag::Image { dest_url, .. }) => Some(dest_url.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        self.url_by_markdown
            .borrow_mut()
            .insert(atom.raw_markdown.clone(), url.clone());
        url
    }

    /// The atom's image once it has loaded, or `None` while it is loading (or
    /// has no loader or url).  The first call for a url starts its load.
    pub fn loaded(&self, atom: &InlineAtom) -> Option<RichImage> {
        let url = self.url_of(atom);
        if url.trim().is_empty() {
            return None;
        }

        let mut loads = self.loads.borrow_mut();
        let load = loads.entry(url.clone()).or_insert_with(|| self.start(&url));
        let state = load.state.lock().ok()?;
        match &*state {
            ImageState::Ready { image, .. } if image.width > 0 && image.height > 0 => {
                Some(RichImage {
                    data: Arc::clone(&image.data),
                    width: image.width,
                    height: image.height,
                })
            }
            _ => None,
        }
    }

    fn start(&self, url: &str) -> Load {
        let resolved = self
            .resolve_url
            .as_ref()
            .map_or_else(|| url.to_string(), |resolve| resolve(url));
        let provided = self.provider.as_ref().and_then(|p| p(&resolved));
        let (state, reported) = match provided {
            // In at once: the layout asking for it sizes it, so there is nothing to report.
            Some((data, width, height)) => (
                ImageState::Ready {
                    image: ImagePixels {
                        data: Arc::new(data),
                        width,
                        height,
                    },
                    seen: true,
                },
                true,
            ),
            None if is_fetchable_url(&resolved)
                && (resolved.starts_with("http://") || resolved.starts_with("https://")) =>
            {
                (ImageState::Loading, false)
            }
            None => (ImageState::Failed, true),
        };
        let state = Arc::new(Mutex::new(state));
        if !reported {
            load_remote_image(resolved, Arc::clone(&state));
        }
        Load { state, reported }
    }

    /// Whether a fetch is still in flight, or has arrived unreported.
    pub fn loading(&self) -> bool {
        self.loads.borrow().values().any(|load| {
            !load.reported
                && load
                    .state
                    .lock()
                    .is_ok_and(|s| !matches!(*s, ImageState::Failed))
        })
    }

    /// The urls whose images arrived since the last call, each once: the
    /// widget re-lays out the blocks showing them.
    pub fn take_arrived(&self) -> Vec<String> {
        let mut arrived = Vec::new();
        for (url, load) in self.loads.borrow_mut().iter_mut() {
            if load.reported {
                continue;
            }
            let Ok(state) = load.state.lock() else {
                continue;
            };
            match &*state {
                ImageState::Ready { .. } => {
                    load.reported = true;
                    arrived.push(url.clone());
                }
                ImageState::Failed => load.reported = true,
                _ => {}
            }
        }
        arrived.sort();
        arrived
    }
}
