use ui::{
    graphics::{Engine, TargetId},
    render::PipelineFactoryFn,
    widget::Element,
};

use crate::{ErasedMsg, Event, OrbitCtl, Subscription};

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

    fn validate_config_raw(&self, cfg: &serde_yml::Value) -> Result<(), String>;
    fn validate_config(&self, cfg: &serde_yml::Value) -> Result<(), String>;
    fn apply_config<'a>(
        &mut self,
        engine: &mut Engine<'a, ErasedMsg>,
        config: &serde_yml::Value,
        options: &mut ui::sctk::Options,
    ) -> bool;
    fn pipelines(&self) -> Vec<(&'static str, PipelineFactoryFn)>;
    fn update<'a>(
        &mut self,
        tid: TargetId,
        engine: &mut Engine<'a, ErasedMsg>,
        event: &Event<ErasedMsg>,
        orbit: &OrbitCtl,
    ) -> bool;
    fn view(&self, tid: &TargetId) -> Element<ErasedMsg>;

    fn subscriptions(&self) -> Subscription<ErasedMsg>;
}
