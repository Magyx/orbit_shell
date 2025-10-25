use std::time::Duration;

use orbit_api::{
    ErasedMsg, Event, OrbitLoop, OrbitModule, Subscription, orbit_plugin,
    ui::{
        graphics::{Engine, TargetId},
        model::Size,
        sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
        widget::{Column, Element, Length, Row, Spacer, Text, Widget as _},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub enum Msg {
    Tick,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Bar height in pixels
    pub height: u32,
    /// strftime-style format (chrono)
    pub time_format: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            height: 32,
            time_format: "%H:%M:%S".into(),
        }
    }
}

#[derive(Default, Debug)]
pub struct Bar {
    now: chrono::DateTime<chrono::Local>,
    cfg: Config,
}

impl OrbitModule for Bar {
    type Config = Config;
    type Message = Msg;

    fn cleanup<'a>(&mut self, _engine: &mut Engine<'a, ErasedMsg>) {}

    fn config_updated<'a>(&mut self, _engine: &mut Engine<'a, ErasedMsg>, cfg: &serde_yml::Value) {
        if let Ok(parsed) = serde_yml::from_value::<Config>(cfg.clone()) {
            self.cfg = parsed;
        }
    }

    fn update<'a>(
        &mut self,
        _tid: TargetId,
        _engine: &mut Engine<'a, ErasedMsg>,
        event: &Event<Self::Message>,
        _orbit: &OrbitLoop,
    ) -> bool {
        if let Event::Message(Msg::Tick) = event {
            self.now = chrono::Local::now();
            return true;
        }
        false
    }

    fn view(&self, _tid: &TargetId) -> Element<Self::Message> {
        Row::new(vec![
            Spacer::new(Size::splat(Length::Grow)).einto(),
            Column::new(vec![
                Spacer::new(Size::splat(Length::Grow)).einto(),
                Text::new(self.now.format(&self.cfg.time_format).to_string(), 18.0).einto(),
                Spacer::new(Size::splat(Length::Grow)).einto(),
            ])
            .size(Size::new(Length::Fit, Length::Grow))
            .einto(),
            Spacer::new(Size::splat(Length::Grow)).einto(),
        ])
        .size(Size::splat(Length::Grow))
        .einto()
    }

    fn subscriptions(&self) -> Subscription<Self::Message> {
        Subscription::Interval {
            every: Duration::from_secs(1),
            message: Msg::Tick,
        }
    }
}

orbit_plugin! {
    module = Bar,
    manifest = {
        name: "bar",
        commands: [],
        options: Options::Layer(LayerOptions {
            layer: Layer::Top,
            size: Size::new(0, 32),
            anchors: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: 32,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            namespace: Some("bar".to_string()),
            output: Some(OutputSet::All),
        }),
        show_on_startup: true,
    },
}
