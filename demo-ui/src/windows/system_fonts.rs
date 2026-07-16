//! App-owned font catalog and lazy-load cache for the demo System window.
//!
//! The `agg-gui` library accepts caller-provided font bytes; this module keeps
//! the demo's particular font choices out of the library and out of the initial
//! WASM binary. Platform shells load these assets only when selected.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use agg_gui::{font_settings, Font, SvgParseOptions};

use super::system::{try_cells, SystemCells};

pub const FONT_AWESOME_PATH: &str = "assets/fa.ttf";
pub const EMOJI_FONT_PATH: &str = "assets/NotoEmoji-Regular.ttf";

#[derive(Clone, Copy)]
pub struct FontAsset {
    pub name: &'static str,
    pub path: &'static str,
}

struct FontLoadRequest {
    name: String,
    path: String,
}

static FONT_OPTIONS: &[FontAsset] = &[
    FontAsset {
        name: "Alfa Slab",
        path: "assets/Alfa_Slab.ttf",
    },
    FontAsset {
        name: "Arial",
        path: "assets/Arial-Regular.ttf",
    },
    FontAsset {
        name: "Arial Italic",
        path: "assets/Arial-Italic.ttf",
    },
    FontAsset {
        name: "Audiowide",
        path: "assets/Audiowide.ttf",
    },
    FontAsset {
        name: "Bangers",
        path: "assets/Bangers.ttf",
    },
    FontAsset {
        name: "Cascadia Code",
        path: "assets/CascadiaCode.ttf",
    },
    FontAsset {
        name: "Courgette",
        path: "assets/Courgette.ttf",
    },
    FontAsset {
        name: "Damion",
        path: "assets/Damion.ttf",
    },
    FontAsset {
        name: "Fredoka",
        path: "assets/Fredoka.ttf",
    },
    FontAsset {
        name: "Georgia",
        path: "assets/Georgia-Regular.ttf",
    },
    FontAsset {
        name: "Georgia Italic",
        path: "assets/Georgia-Italic.ttf",
    },
    FontAsset {
        name: "Great Vibes",
        path: "assets/Great_Vibes.ttf",
    },
    FontAsset {
        name: "Liberation Sans",
        path: "assets/LiberationSans-Regular.ttf",
    },
    FontAsset {
        name: "Liberation Sans Italic",
        path: "assets/LiberationSans-Italic.ttf",
    },
    FontAsset {
        name: "Liberation Serif",
        path: "assets/LiberationSerif-Regular.ttf",
    },
    FontAsset {
        name: "Liberation Serif Italic",
        path: "assets/LiberationSerif-Italic.ttf",
    },
    FontAsset {
        name: "Lobster",
        path: "assets/Lobster.ttf",
    },
    FontAsset {
        name: "Nunito",
        path: "assets/Nunito_Regular.ttf",
    },
    FontAsset {
        name: "Nunito Italic",
        path: "assets/Nunito_Italic.ttf",
    },
    FontAsset {
        name: "Nunito SemiBold",
        path: "assets/Nunito_SemiBold.ttf",
    },
    FontAsset {
        name: "Nunito Bold",
        path: "assets/Nunito_Bold.ttf",
    },
    FontAsset {
        name: "Nunito Bold Italic",
        path: "assets/Nunito_Bold_Italic.ttf",
    },
    FontAsset {
        name: "Pacifico",
        path: "assets/Pacifico.ttf",
    },
    FontAsset {
        name: "Poppins",
        path: "assets/Poppins.ttf",
    },
    FontAsset {
        name: "Questrial",
        path: "assets/Questrial.ttf",
    },
    FontAsset {
        name: "Righteous",
        path: "assets/Righteous.ttf",
    },
    FontAsset {
        name: "Russo",
        path: "assets/Russo.ttf",
    },
    FontAsset {
        name: "Tahoma",
        path: "assets/Tahoma-Regular.ttf",
    },
    FontAsset {
        name: "Times New Roman",
        path: "assets/TimesNewRoman-Regular.ttf",
    },
    FontAsset {
        name: "Times New Roman Italic",
        path: "assets/TimesNewRoman-Italic.ttf",
    },
    FontAsset {
        name: "Titan",
        path: "assets/Titan.ttf",
    },
    FontAsset {
        name: "Verdana",
        path: "assets/Verdana-Regular.ttf",
    },
    FontAsset {
        name: "Verdana Italic",
        path: "assets/Verdana-Italic.ttf",
    },
];

thread_local! {
    static FONT_CACHE: RefCell<HashMap<String, Arc<Font>>> = RefCell::new(HashMap::new());
    static FONT_BYTES_CACHE: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
    static PENDING_FONT_REQUESTS: RefCell<VecDeque<FontLoadRequest>> =
        RefCell::new(VecDeque::new());
    static FONT_CACHE_EPOCH: RefCell<u64> = const { RefCell::new(0) };
}

pub const DEFAULT_FONT_NAME: &str = "Nunito";

pub fn font_option_names() -> Vec<&'static str> {
    FONT_OPTIONS.iter().map(|o| o.name).collect()
}

pub fn font_option_index(name: &str) -> Option<usize> {
    FONT_OPTIONS.iter().position(|o| o.name == name)
}

pub fn default_font_index() -> usize {
    font_option_index(DEFAULT_FONT_NAME).unwrap_or(0)
}

pub fn font_asset_by_index(idx: usize) -> Option<&'static FontAsset> {
    FONT_OPTIONS.get(idx)
}

/// Whether the catalog ships a bold face for `family` — i.e. a `"{family} Bold"`,
/// `"{family} SemiBold"`, or `"{family} Bold Italic"` entry. Mirrors the bold
/// candidates the resolver tries (see `rich_text_demo::variant_candidates`), so
/// the toolbar can disable the Bold toggle for families with no real bold face
/// rather than silently rendering the regular weight.
pub fn family_has_bold(family: &str) -> bool {
    let bold = format!("{family} Bold");
    let semibold = format!("{family} SemiBold");
    let bold_italic = format!("{family} Bold Italic");
    FONT_OPTIONS
        .iter()
        .any(|o| o.name == bold || o.name == semibold || o.name == bold_italic)
}

/// Whether the catalog ships an italic face for `family` — i.e. a
/// `"{family} Italic"` or `"{family} Bold Italic"` entry. Companion to
/// [`family_has_bold`] for the toolbar's Italic toggle.
pub fn family_has_italic(family: &str) -> bool {
    let italic = format!("{family} Italic");
    let bold_italic = format!("{family} Bold Italic");
    FONT_OPTIONS
        .iter()
        .any(|o| o.name == italic || o.name == bold_italic)
}

pub fn font_asset_by_name(name: &str) -> Option<&'static FontAsset> {
    FONT_OPTIONS.iter().find(|o| o.name == name)
}

pub fn load_font_by_name(name: &str) -> Option<Arc<Font>> {
    FONT_CACHE.with(|cache| cache.borrow().get(name).cloned())
}

pub fn font_cache_epoch() -> u64 {
    FONT_CACHE_EPOCH.with(|epoch| *epoch.borrow())
}

pub fn loaded_item_fonts(label_font: &Arc<Font>) -> Vec<Arc<Font>> {
    FONT_OPTIONS
        .iter()
        .map(|asset| load_font_by_name(asset.name).unwrap_or_else(|| Arc::clone(label_font)))
        .collect()
}

pub fn apply_font_by_index(cells: &SystemCells, idx: usize) {
    if let Some(asset) = font_asset_by_index(idx) {
        *cells.font_name.borrow_mut() = Some(asset.name.to_string());
        cells.font_index.set(idx);

        if let Some(font) = load_font_by_name(asset.name) {
            font_settings::set_system_font(Some(font));
        } else {
            request_font(cells, asset);
        }
    }
}

pub fn request_font_by_index(cells: &SystemCells, idx: usize) {
    if let Some(asset) = font_asset_by_index(idx) {
        request_font(cells, asset);
    }
}

pub fn request_all_font_previews(cells: &SystemCells) {
    for asset in FONT_OPTIONS {
        request_font(cells, asset);
    }
}

fn request_font(cells: &SystemCells, asset: &FontAsset) {
    if load_font_by_name(asset.name).is_some() {
        return;
    }

    PENDING_FONT_REQUESTS.with(|requests| {
        let mut requests = requests.borrow_mut();
        if !requests.iter().any(|r| r.name == asset.name) {
            requests.push_back(FontLoadRequest {
                name: asset.name.to_string(),
                path: asset.path.to_string(),
            });
        }
    });
    (cells.platform.on_font_request)(asset.name, asset.path);
}

pub fn take_pending_font_request() -> Option<(String, String)> {
    PENDING_FONT_REQUESTS.with(|requests| {
        requests
            .borrow_mut()
            .pop_front()
            .map(|request| (request.name, request.path))
    })
}

pub fn install_font_bytes(
    name: &str,
    primary_bytes: Vec<u8>,
    icon_bytes: Option<Vec<u8>>,
    emoji_bytes: Option<Vec<u8>>,
) -> Result<Arc<Font>, &'static str> {
    FONT_BYTES_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .insert(name.to_string(), primary_bytes.clone());
    });
    let mut font = Font::from_bytes(primary_bytes)?;
    if let Some(icon_bytes) = icon_bytes {
        let mut icon_font = Font::from_bytes(icon_bytes)?;
        if let Some(emoji_bytes) = emoji_bytes {
            icon_font = icon_font.with_fallback(Arc::new(Font::from_bytes(emoji_bytes)?));
        }
        font = font.with_fallback(Arc::new(icon_font));
    } else if let Some(emoji_bytes) = emoji_bytes {
        font = font.with_fallback(Arc::new(Font::from_bytes(emoji_bytes)?));
    }

    let font = Arc::new(font);
    FONT_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .insert(name.to_string(), Arc::clone(&font));
    });
    FONT_CACHE_EPOCH.with(|epoch| *epoch.borrow_mut() += 1);
    agg_gui::animation::request_draw();
    refresh_default_svg_fonts();

    if let Some(cells) = try_cells() {
        if cells.font_name.borrow().as_deref() == Some(name) {
            font_settings::set_system_font(Some(Arc::clone(&font)));
            if let Some(idx) = font_option_index(name) {
                cells.font_index.set(idx);
            }
        }
    }

    Ok(font)
}

fn refresh_default_svg_fonts() {
    FONT_BYTES_CACHE.with(|cache| {
        let fonts = cache.borrow().values().cloned().collect::<Vec<_>>();
        if fonts.is_empty() {
            return;
        }
        let fontdb = agg_gui::svg_fontdb_from_font_data(fonts, Some(DEFAULT_FONT_NAME));
        agg_gui::set_default_svg_parse_options(SvgParseOptions::new().with_fontdb(fontdb));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nunito is the only family that ships a full Bold + Bold-Italic set, so it
    /// reports both capabilities. This is the default family, so a default-styled
    /// selection keeps B and I enabled.
    #[test]
    fn nunito_has_bold_and_italic() {
        assert!(family_has_bold("Nunito"));
        assert!(family_has_italic("Nunito"));
        assert!(family_has_bold(DEFAULT_FONT_NAME));
        assert!(family_has_italic(DEFAULT_FONT_NAME));
    }

    /// Arial / Georgia ship a real Italic but no Bold face, so the Italic toggle
    /// stays enabled while Bold is disabled for them.
    #[test]
    fn italic_only_families_report_no_bold() {
        for family in ["Arial", "Georgia", "Verdana", "Times New Roman"] {
            assert!(family_has_italic(family), "{family} should have italic");
            assert!(!family_has_bold(family), "{family} should not have bold");
        }
    }

    /// A display family with only a single regular face reports neither.
    #[test]
    fn single_face_family_has_no_variants() {
        assert!(!family_has_bold("Pacifico"));
        assert!(!family_has_italic("Pacifico"));
    }
}
