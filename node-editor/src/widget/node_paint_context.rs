//! `NodePaintContext` — the per-frame paint context shared by every
//! widget under a [`crate::NodeEditor`].  Extracted from `nodes.rs` to
//! keep that file under the project's 800-line cap.

use std::sync::Arc;

use agg_gui::Color;

use crate::draw::CanvasPalette;
use crate::model::NodeGraphModel;

/// Shared per-frame context every node widget needs to render.  Cloning
/// the `Arc` is cheap — the inner data is rebuilt by `NodeEditor` each
/// paint frame.
#[derive(Clone)]
pub struct NodePaintContext {
    pub palette: Arc<CanvasPalette>,
    /// Socket colour lookup by type id.  Captured up-front so the row /
    /// socket widgets don't need to lock the host model during paint.
    pub socket_colors: Arc<dyn Fn(crate::model::SocketTypeId) -> Color + Send + Sync>,
    /// Title-bar colour lookup by category.
    pub title_colors: Arc<dyn Fn(&str, Color) -> Color + Send + Sync>,
    /// Active canvas zoom factor — multiplies every dimension (bounds,
    /// font sizes, radii, padding) so the widget tree paints at the
    /// right screen size.  We bake the scale into the widget tree
    /// rather than push `ctx.scale` because the framework's per-child
    /// translate composes additively in screen-space and doesn't
    /// respect a parent's scale (the existing nested-translate limit).
    pub scale: f64,
}

impl NodePaintContext {
    /// Build a fresh context from the live palette and model.  Resolves
    /// socket / title colours by snapshotting the model into owned
    /// closures so the widgets don't reach back into the model later.
    pub fn from_model<M: NodeGraphModel + ?Sized>(palette: CanvasPalette, model: &M) -> Self {
        Self::from_model_scaled(palette, model, 1.0)
    }

    /// Same as [`from_model`] with an explicit canvas zoom factor.
    pub fn from_model_scaled<M: NodeGraphModel + ?Sized>(
        palette: CanvasPalette,
        model: &M,
        scale: f64,
    ) -> Self {
        let mut ctx = Self::build_from_model(palette, model);
        ctx.scale = scale;
        ctx
    }

    fn build_from_model<M: NodeGraphModel + ?Sized>(palette: CanvasPalette, model: &M) -> Self {
        // Capture the model's colour data into a small owned table the
        // closures can read from without needing the borrow.  Sockets +
        // categories tend to be tiny (single digit count), so an alloc
        // per paint is fine.
        let mut socket_pairs: Vec<(crate::model::SocketTypeId, Color)> = Vec::new();
        for (ty, col) in collect_socket_colors(model) {
            socket_pairs.push((ty, col));
        }
        let socket_pairs = Arc::new(socket_pairs);
        let socket_pairs_clone = socket_pairs.clone();
        let socket_colors = Arc::new(move |ty: crate::model::SocketTypeId| -> Color {
            socket_pairs_clone
                .iter()
                .find(|(t, _)| *t == ty)
                .map(|(_, c)| *c)
                .unwrap_or_else(|| Color::rgba(0.55, 0.58, 0.66, 1.0))
        }) as Arc<dyn Fn(_) -> _ + Send + Sync>;

        let mut category_pairs: Vec<(String, Color)> = Vec::new();
        for (cat, col) in collect_category_colors(model, palette.node_title_fallback) {
            category_pairs.push((cat, col));
        }
        let category_pairs = Arc::new(category_pairs);
        let category_pairs_clone = category_pairs.clone();
        let title_colors: Arc<dyn Fn(&str, Color) -> Color + Send + Sync> =
            Arc::new(move |cat: &str, fallback: Color| -> Color {
                category_pairs_clone
                    .iter()
                    .find(|(c, _)| c == cat)
                    .map(|(_, col)| *col)
                    .unwrap_or(fallback)
            });

        Self {
            palette: Arc::new(palette),
            socket_colors,
            title_colors,
            scale: 1.0,
        }
    }
}

fn collect_socket_colors<M: NodeGraphModel + ?Sized>(
    model: &M,
) -> Vec<(crate::model::SocketTypeId, Color)> {
    let mut seen: Vec<crate::model::SocketTypeId> = Vec::new();
    for n in model.nodes() {
        for s in n.inputs.iter().chain(n.outputs.iter()) {
            if !seen.contains(&s.socket_type) {
                seen.push(s.socket_type);
            }
        }
    }
    seen.into_iter()
        .map(|ty| (ty, model.socket_color(ty)))
        .collect()
}

fn collect_category_colors<M: NodeGraphModel + ?Sized>(
    model: &M,
    fallback: Color,
) -> Vec<(String, Color)> {
    let mut seen: Vec<String> = Vec::new();
    for n in model.nodes() {
        if !seen.contains(&n.category) {
            seen.push(n.category.clone());
        }
    }
    seen.into_iter()
        .map(|c| {
            let col = model.category_color(&c, fallback);
            (c, col)
        })
        .collect()
}
