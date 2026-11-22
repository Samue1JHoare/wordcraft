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
    let mut scene = Scene::new(w as u16, h as u16);
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
