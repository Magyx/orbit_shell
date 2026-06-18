use ui::{
    graphics::{Engine, TargetId},
    render::PipelineFactoryFn,
    theme::Theme,
    widget::Element,
};

use crate::{ErasedMsg, Event, SettingsOutcome, Subscription, Task};

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
    fn cleanup<'a>(&mut self, engine: &mut Engine<'a, ErasedMsg>);

    fn validate_config(&self, cfg: &yaml_serde::Value) -> Result<(), String>;
    fn apply_config<'a>(
        &mut self,
        engine: &mut Engine<'a, ErasedMsg>,
        cfg: &yaml_serde::Value,
        options: &mut ui::sctk::Options,
    ) -> bool;
    fn pipelines(&self) -> Vec<(&'static str, PipelineFactoryFn)>;
    fn update<'a>(
        &mut self,
        tid: Option<TargetId>,
        engine: &mut Engine<'a, ErasedMsg>,
        event: &Event<ErasedMsg>,
    ) -> Task<ErasedMsg>;
    fn view(&self, tid: &TargetId, theme: &Theme) -> Element<ErasedMsg>;
    fn command_message(&self, command: &str) -> Option<ErasedMsg>;

    fn subscriptions(&self) -> Subscription<ErasedMsg>;

    fn settings_view(&self, cfg: &yaml_serde::Value, theme: &Theme) -> Element<ErasedMsg> {
        let _ = (cfg, theme);
        let typed: Element<()> = ui::widget::Text::body("This module has no settings.").into();
        erased::erase_element(typed)
    }
    fn settings_update(
        &self,
        cfg: &mut yaml_serde::Value,
        event: &Event<ErasedMsg>,
    ) -> SettingsOutcome {
        let _ = (cfg, event);
        SettingsOutcome::Ignored
    }
}
