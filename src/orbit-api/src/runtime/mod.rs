use ui::{
    graphics::{Engine, TargetId},
    render::PipelineFactoryFn,
    theme::Theme,
    widget::Element,
};

use crate::{ErasedMsg, Event, OrbitCtl, Subscription, Task};

pub mod erased;

#[derive(Clone)]
pub struct Manifest {
    pub name: &'static str,
    pub commands: &'static [&'static str],
    pub options: ui::sctk::Options,
    pub show_on_startup: bool,
    pub persistent_state: bool,
}

pub trait OrbitModuleDyn: 'static {
    fn manifest(&self) -> &Manifest;
    fn cleanup<'a>(&mut self, engine: &mut Engine<'a>);

    fn validate_config_raw(&self, cfg: &yaml_serde::Value) -> Result<(), String>;
    fn validate_config(&self, cfg: &yaml_serde::Value) -> Result<(), String>;
    fn apply_config<'a>(
        &mut self,
        engine: &mut Engine<'a>,
        config: &yaml_serde::Value,
        options: &mut ui::sctk::Options,
    ) -> bool;
    fn pipelines(&self) -> Vec<(&'static str, PipelineFactoryFn)>;
    fn update<'a>(
        &mut self,
        ctl: &mut OrbitCtl<'_>,
        tid: Option<TargetId>,
        engine: &mut Engine<'a>,
        event: &Event<ErasedMsg>,
    ) -> Task<ErasedMsg>;
    fn on_broadcast(
        &mut self,
        ctl: &mut OrbitCtl<'_>,
        tid: Option<TargetId>,
        key: &'static str,
    ) -> Task<ErasedMsg>;
    fn view(&self, tid: &TargetId, theme: &Theme) -> Element;
    fn command_message(&self, command: &str) -> Option<ErasedMsg>;

    fn subscriptions(&self) -> Subscription<ErasedMsg>;
}
