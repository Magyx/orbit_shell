use orbit_api::ui::{
    context::{LayoutCtx, PaintCtx},
    layout::Node,
    model::*,
    primitive::Instance,
    render::{pipeline::PipelineKey, texture::TextureHandle},
    widget::{ContentFit, IntoElement, Length, Widget},
};

pub struct BlurImage {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    size: Size<Length>,
    min: Size<i32>,
    max: Size<i32>,
    handle: TextureHandle,
    fit: ContentFit,
    tint: Color,
    blur_strength: u32,
}

#[allow(dead_code)]
impl BlurImage {
    pub fn new(size: Size<Length>, handle: TextureHandle) -> Self {
        Self {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            size,
            min: Size::splat(0),
            max: Size::splat(i32::MAX),
            handle,
            fit: ContentFit::Fill,
            tint: Color::WHITE,
            blur_strength: 0,
        }
    }
    pub fn tint(mut self, tint: Color) -> Self {
        self.tint = tint;
        self
    }
    pub fn strength(mut self, strength: u32) -> Self {
        self.blur_strength = strength;
        self
    }
    pub fn fit(mut self, fit: ContentFit) -> Self {
        self.fit = fit;
        self
    }
}

impl IntoElement for BlurImage {}

impl<M> Widget<M> for BlurImage {
    fn layout<'a>(&mut self, _ctx: &mut LayoutCtx<'a, M>) -> Node {
        let mut node = Node {
            size: self.size,
            min: self.min,
            max: self.max,
            ..Default::default()
        };
        if matches!(self.fit, ContentFit::Cover) {
            node.clip_children = true;
        }
        node
    }

    fn set_layout(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.x = x;
        self.y = y;
        self.w = w;
        self.h = h;
    }

    fn child_count(&self) -> usize {
        0
    }
    fn child_mut(&mut self, _i: usize) -> &mut dyn Widget<M> {
        unreachable!()
    }

    fn paint(&mut self, _ctx: &mut PaintCtx, out: &mut Vec<Instance>) {
        if self.tint.a() == 0 {
            return;
        }

        let sw = self.handle.size_px.width as i32;
        let sh = self.handle.size_px.height as i32;
        if sw <= 0 || sh <= 0 || self.w <= 0 || self.h <= 0 {
            return;
        }

        let dst_w = self.w as f32;
        let dst_h = self.h as f32;
        let src_w = sw as f32;
        let src_h = sh as f32;

        let (draw_w, draw_h) = match self.fit {
            ContentFit::Fill => (dst_w, dst_h),
            ContentFit::Contain => {
                let s = (dst_w / src_w).min(dst_h / src_h);
                (src_w * s, src_h * s)
            }
            ContentFit::Cover => {
                let s = (dst_w / src_w).max(dst_h / src_h);
                (src_w * s, src_h * s)
            }
        };

        let px = self.x as f32 + (dst_w - draw_w) * 0.5;
        let py = self.y as f32 + (dst_h - draw_h) * 0.5;
        let dw = draw_w.max(1.0);
        let dh = draw_h.max(1.0);

        out.push(Instance::new(
            PipelineKey::Other("blur"),
            Position::new(px, py),
            Size::new(dw, dh),
            // data1 maps to `@location(2) style` in ui_shader.wgsl / blur_shader.wgsl
            // index 0: Tint Color
            // index 1: Blur Strength (read as in.style.y / blur_strength in shader)
            [self.tint.0, self.blur_strength, 0, 0],
            // data2 maps to `@location(3) tex`
            // Matches the standard ui_tex layout exactly so texture mapping works
            [
                self.handle.slot_gen,
                self.handle.scale_packed,
                self.handle.offset_packed,
                0,
            ],
        ));
    }
}
