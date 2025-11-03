use orbit_api::ErasedMsg;
use ui::{
    el,
    graphics::TargetId,
    model::{Color, Size, Vec4},
    sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
    widget::{Column, Element, Length, Scrollable, Text},
};

use crate::{Orbit, sctk::CreatedSurface};

pub fn error_view(_tid: &TargetId, errors: &Vec<String>) -> Element<ErasedMsg> {
    let mut col = Column::new::<Vec<_>, Element<ErasedMsg>>(el!())
        .padding(Vec4::splat(10))
        .size(Size::splat(Length::Grow))
        .color(Color::RED);
    for error in errors {
        col.push(Text::new(error.clone(), 14.0));
    }
    Scrollable::new(
        Column::new(el!(col))
            .padding(Vec4::splat(10))
            .size(Size::splat(Length::Grow)),
    )
    .size(Size::new(Length::Grow, Length::Grow))
    .into()
}

impl<'a> Orbit<'a> {
    pub fn show_error(&mut self, errors: Vec<String>) {
        if self.error_dialog.is_empty() {
            let opts = Options::Layer(LayerOptions {
                layer: Layer::Top,
                size: Size::new(0, 64),
                anchors: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
                exclusive_zone: 64,
                keyboard_interactivity: KeyboardInteractivity::None,
                namespace: Some("orbit_error".into()),
                output: Some(OutputSet::All),
            });

            let made = self.sctk.create_surfaces(opts);
            for CreatedSurface { sid, handles, size } in made {
                let tid = self
                    .engine
                    .attach_target(std::sync::Arc::new(handles), size);
                self.error_dialog.push((tid, sid));
            }
        }

        self.errors = errors;
    }

    pub fn hide_error(&mut self) {
        for (tid, sid) in self.error_dialog.drain(..) {
            self.engine.detach_target(&tid);
            self.sctk.destroy_surfaces(&[sid]);
        }
        self.errors.clear();
    }
}
