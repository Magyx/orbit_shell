use orbit_api::{Engine, ErasedMsg};
use ui::{
    el,
    graphics::TargetId,
    model::{Color, Size, Vec4},
    sctk::{
        Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet, RawWaylandHandles,
        SctkEvent, SurfaceId,
    },
    widget::{Column, Element, Length, Scrollable, Text},
};

use crate::sctk::SctkApp;

pub fn error_view(_: &TargetId, errors: &Vec<String>) -> Element<ErasedMsg> {
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

pub struct ErrorDialog {
    targets: Vec<(TargetId, SurfaceId)>,
    errors: Vec<String>,

    pending_sids: Vec<SurfaceId>,
}

impl ErrorDialog {
    pub fn new() -> Self {
        Self {
            targets: Vec::new(),
            errors: Vec::new(),
            pending_sids: Vec::new(),
        }
    }

    pub fn is_shown(&self) -> bool {
        !self.targets.is_empty()
    }
    pub fn show(&mut self, sctk: &mut SctkApp, errors: Vec<String>) {
        if self.targets.is_empty() {
            let opts = Options::Layer(LayerOptions {
                layer: Layer::Top,
                size: Size::new(0, 64),
                anchors: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
                exclusive_zone: 64,
                keyboard_interactivity: KeyboardInteractivity::None,
                namespace: Some("orbit_error".into()),
                output: Some(OutputSet::All),
            });

            let sids = sctk.create_surfaces(opts);
            for sid in sids {
                self.pending_sids.push(sid);
            }
        }

        self.errors = errors;
    }
    pub fn try_attach_pending(
        &mut self,
        engine: &mut Engine<'_>,
        sctk: &mut SctkApp,
        sid: SurfaceId,
    ) {
        let Some(index) = self.pending_sids.iter().position(|i| i == &sid) else {
            return;
        };
        self.pending_sids.remove(index);
        let Some(rec) = sctk.state.surfaces.get(&sid) else {
            return;
        };
        let handles = RawWaylandHandles::new(&sctk.conn, &rec.wl_surface);
        let tid = engine.attach_target(std::sync::Arc::new(handles), rec.size);
        self.targets.push((tid, sid));
    }
    pub fn hide(&mut self, engine: &mut Engine<'_>, sctk: &mut SctkApp) {
        let mut sids: Vec<SurfaceId> = self.targets.iter().map(|(_, sid)| *sid).collect();
        sids.append(&mut self.pending_sids);

        for (tid, _) in self.targets.drain(..) {
            engine.detach_target(&tid);
        }
        sctk.destroy_surfaces(&sids);
        self.errors.clear();
    }
    pub fn remove_sid(&mut self, sid: SurfaceId) {
        self.pending_sids.retain(|s| *s != sid);
    }

    pub fn handle_platform_event(&mut self, engine: &mut Engine<'_>, event: &SctkEvent) {
        if !self.targets.is_empty() {
            for (tid, _) in self
                .targets
                .iter()
                .filter(|(_, s)| Some(*s) == event.surface_id())
            {
                engine.handle_platform_event(tid, event, &mut |_, _, _, _| false, &mut (), &());
            }
        }
    }
    pub fn render(&mut self, engine: &mut Engine<'_>) {
        for (tid, _) in self.targets.iter() {
            let need = engine.poll(
                tid,
                &mut |_, _: &ui::event::Event<ErasedMsg, SctkEvent>, (), _| false,
                &mut (),
                &(),
            );
            if let Err(e) = engine.render_if_needed(tid, need, &error_view, &mut self.errors) {
                tracing::error!("error dialog render failed: {e:?}");
            }
        }
    }
}
