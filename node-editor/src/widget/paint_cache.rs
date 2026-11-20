//! What the canvas paint reads from the model, kept while the canvas's
//! content stays the same.
//!
//! Painting the grid's noodles needs every card's layout (where each socket
//! is) and every noodle with its colour, dash and landing point: a read of
//! every node, socket and noodle from the model, which costs a host with a
//! large graph several milliseconds. All of it is in canvas space, so a
//! change of the view alone (`view_frames.rs`), which re-rasterises the
//! editor's layer every frame of a pan or zoom, leaves it as it was.
//!
//! [`NodeEditor::invalidate_canvas`] — what every change of the canvas's
//! content goes through, a full layout included — starts a new generation,
//! and the next paint reads the model again. A paint of the same generation
//! (a view change, or a card's own redraw such as a text cursor blinking)
//! reuses the last read.

use std::rc::Rc;

use agg_gui::Color;

use crate::draw::NodeLayoutInfo;
use crate::model::NoodleView;

use super::{hover, presentation, NodeEditor};

/// One noodle as the canvas draws it.
pub(super) struct NoodlePaint {
    pub noodle: NoodleView,
    /// The output socket's centre, canvas space.
    pub from: [f64; 2],
    /// Where the noodle lands on its input, canvas space.
    pub to: [f64; 2],
    pub color: Color,
    pub dashed: bool,
}

/// The model read one canvas paint makes.
pub(super) struct PaintRead {
    pub layouts: Vec<NodeLayoutInfo>,
    /// The noodles that resolve to two sockets, in the model's order.
    pub noodles: Vec<NoodlePaint>,
}

/// The last read and the generation it belongs to.
#[derive(Default)]
pub(crate) struct PaintCache {
    /// Bumped by every [`NodeEditor::invalidate_canvas`].
    pub generation: u64,
    read: Option<(u64, Rc<PaintRead>)>,
}

impl NodeEditor {
    /// The model read for this paint: the last one while the canvas's
    /// content is unchanged, else a fresh one.
    pub(super) fn paint_read(&mut self) -> Rc<PaintRead> {
        let generation = self.paint_cache.generation;
        if let Some((at, read)) = &self.paint_cache.read {
            if *at == generation {
                return Rc::clone(read);
            }
        }
        let layouts = self.snapshot_layouts();
        let model = self.model.lock().unwrap();
        let views = model.noodles();
        let noodles = views
            .iter()
            .filter_map(|noodle| {
                let (f, t) = hover::resolve_noodle_endpoints(&layouts, noodle)?;
                Some(NoodlePaint {
                    noodle: noodle.clone(),
                    from: f.center,
                    to: presentation::landing_point(&views, noodle, t),
                    color: model
                        .noodle_color(noodle)
                        .unwrap_or_else(|| model.socket_color(f.socket_type)),
                    dashed: model.noodle_dashed(noodle),
                })
            })
            .collect();
        drop(model);
        let read = Rc::new(PaintRead { layouts, noodles });
        self.paint_cache.read = Some((generation, Rc::clone(&read)));
        read
    }
}
