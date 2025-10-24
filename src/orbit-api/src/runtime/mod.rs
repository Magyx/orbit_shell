use ui::{
    graphics::{Engine, TargetId},
    render::PipelineFactoryFn,
    widget::Element,
};

use crate::{ErasedMsg, Event, OrbitLoop, Subscription};

pub mod erased;

#[derive(Clone)]
pub struct Manifest {
    pub name: &'static str,
    pub commands: &'static [&'static str],
    pub options: ui::sctk::Options,
    pub show_on_startup: bool,
}

pub trait OrbitModuleDyn: 'static {
    fn manifest(&self) -> &Manifest;
    fn cleanup<'a>(&mut self, engine: &mut Engine<'a, ErasedMsg>);

    fn init_config(&self, cfg: &mut serde_yml::Value);
    fn validate_config(&self, cfg: &serde_yml::Value) -> Result<(), String>;
    fn config_updated<'a>(&mut self, engine: &mut Engine<'a, ErasedMsg>, cfg: &serde_yml::Value);

    fn pipelines(&self) -> Vec<(&'static str, PipelineFactoryFn)>;
    fn update<'a>(
        &mut self,
        tid: TargetId,
        engine: &mut Engine<'a, ErasedMsg>,
        event: &Event<ErasedMsg>,
        orbit: &OrbitLoop,
    ) -> bool;
    fn view(&self, tid: &TargetId) -> Element<ErasedMsg>;

    fn subscriptions(&self) -> Subscription<ErasedMsg>;
}
