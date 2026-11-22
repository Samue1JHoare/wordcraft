//! Record supported page display lists for a vello_hybrid renderer. A declined request
//! must be drawn in full with the CPU renderer.
use super::*;
use vello_hybrid::Scene;

/// The same page or area, raster size, display options and glyph outlines as the CPU renderer.
pub fn record(doc: &Document, page: &Page, area: Option<wordcraft_geom::Rect>, scale: f32, opts: &RenderOptions, max_side: u16) -> Option<Scene> {
    if doc.settings.watermark.is_some() || (opts.read_paper.is_none() && doc.settings.page_picture.is_some()) {
        return None;
    }
    let (w, h, view) = area.map_or_else(|| page_region(page, scale), |a| area_region(a, scale));
    if (w, h) != region_raster_size(w, h) || w.max(h) > u32::from(max_side) {
        return None;
    }
    let mut scene = Scene::new_with(w as u16, h as u16, vello_hybrid::RenderSettings {
        constraints: vello_hybrid::SceneConstraints::new().default_blending_only(), ..Default::default()
    });
    let bounds = kurbo::Rect::new(0.0, 0.0, f64::from(w), f64::from(h));
    scene.set_paint(color(opts.paper_of(doc), 1.0));
    scene.fill_rect(&bounds);
    let visible = view.inverse().transform_rect_bbox(bounds).inflate(40.0, 40.0);
    let fr = Frame { doc, opts, view, visible: &visible };
    for (alpha, items) in page_layers(doc, page, &opts.display) {
        if alpha < 1.0 && !items.is_empty() {
            scene.push_opacity_layer(alpha);
        }
        for it in &items {
            match it {
                Draw::Glyphs { face, .. } if !face.has_color_glyphs() => {
                    // Colour fonts were declined above; this callback is never reached.
                    draw_glyph_run(&mut scene, &fr, it, |_, _, _, _, _, _| {});
                }
                Draw::Fill { rect, color: c, alpha, text_bg } => {
                    let r = kurbo_rect(rect);
                    if r.overlaps(visible) {
                        scene.set_transform(view);
                        scene.set_paint(color(if *text_bg { opts.ink_bg(*c) } else { opts.ink(*c) }, *alpha));
                        scene.fill_rect(&r);
                    }
                }
                Draw::Line { .. } => draw_line(&mut scene, &fr, it),
                _ => return None,
            }
        }
        if alpha < 1.0 && !items.is_empty() {
            scene.pop_layer();
        }
    }
    Some(scene)
}

/// The font database already interns both hinted and unhinted outlines. Cache the other
/// expensive geometry, the stroke used to darken an outline, before positioning the glyph.
#[derive(Default)]
struct GlyphCache {
    entries: HashMap<(usize, u64, u64, u8), CachedGlyph>,
    bytes: usize,
}

struct CachedGlyph {
    // Retain the key's allocation so its address cannot be recycled.
    _outline: Arc<BezPath>,
    stroke: BezPath,
    bounds: kurbo::Rect,
}

impl GlyphTarget for Scene {
    fn glyph(&mut self, path: &Arc<BezPath>, at: Affine, stroked: bool) {
        thread_local! { static CACHE: std::cell::RefCell<GlyphCache> = Default::default(); }
        let viewport = kurbo::Rect::new(0., 0., f64::from(self.width()), f64::from(self.height())).inflate(1., 1.);
        CACHE.with_borrow_mut(|cache| {
            let style = self.stroke();
            // Same tolerance and expansion as vello_common 0.0.9 flatten::stroke.
            let m = at.as_coeffs();
            let tol = 0.25 / m[0].abs().max(m[3].abs()).max(1.);
            let key = (Arc::as_ptr(path) as usize, if stroked { style.width.to_bits() } else { 0 }, tol.to_bits(), style.join as u8);
            let build = || {
                let stroke = if stroked { kurbo::stroke(path.elements().iter().copied(), style, &Default::default(), tol) } else { BezPath::new() };
                let mut bounds = path.control_box();
                if stroked { bounds = bounds.union(stroke.control_box()); }
                CachedGlyph { _outline: path.clone(), stroke, bounds }
            };
            let transient;
            let glyph = if let Some(hit) = cache.entries.get(&key) {
                hit
            } else {
                let new = build();
                let bytes = (path.elements().len() + new.stroke.elements().len()) * std::mem::size_of::<kurbo::PathEl>() + 128;
                const BUDGET: usize = 2 << 20;
                if bytes > BUDGET {
                    transient = new;
                    &transient
                } else {
                    if cache.bytes + bytes > BUDGET { cache.entries.clear(); cache.bytes = 0; }
                    cache.bytes += bytes;
                    cache.entries.entry(key).or_insert(new)
                }
            };
            if at.transform_rect_bbox(glyph.bounds).overlaps(viewport) {
                self.set_transform(at);
                self.fill_path(path);
                if stroked { self.fill_path(&glyph.stroke); }
            }
        });
    }
}
