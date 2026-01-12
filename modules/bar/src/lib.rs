use std::time::Duration;

use orbit_api::{
    Engine, Event, OrbitCtl, OrbitModule, Subscription, orbit_plugin,
    ui::{
        el,
        graphics::TargetId,
        model::{Color, Size},
        sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
        widget::{Column, Element, Length, Row, Spacer, Text},
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

#[derive(Debug)]
pub struct Bar {
    now: chrono::DateTime<chrono::Local>,
    cfg: Config,
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            now: chrono::Local::now(),
            cfg: Default::default(),
        }
    }
}

impl OrbitModule for Bar {
    type Config = Config;
    type Message = Msg;

    fn cleanup<'a>(&mut self, _engine: &mut Engine<'a>) {}

    fn validate_config(cfg: Self::Config) -> Result<(), String> {
        if cfg.height < 1 {
            Err("Height must be at least 1".into())
        } else {
            Ok(())
        }
    }

    fn apply_config<'a>(
        &mut self,
        _engine: &mut Engine<'a>,
        config: Self::Config,
        options: &mut orbit_api::ui::sctk::Options,
    ) -> bool {
        self.cfg = config;
        let Options::Layer(layer) = options else {
            return false;
        };

        if layer.size.height != self.cfg.height {
            layer.size.height = self.cfg.height;
            layer.exclusive_zone = self.cfg.height as i32;
            true
        } else {
            false
        }
    }

    fn update<'a>(
        &mut self,
        _tid: TargetId,
        _engine: &mut Engine<'a>,
        event: &Event<Self::Message>,
        _orbit: &OrbitCtl,
    ) -> bool {
        if let Event::Message(Msg::Tick) = event {
            self.now = chrono::Local::now();
            return true;
        }
        false
    }

    fn view(&self, _tid: &TargetId) -> Element<Self::Message> {
        Row::new(el![
            Spacer::new(Size::splat(Length::Grow)),
            Column::new(el![
                Spacer::new(Size::splat(Length::Grow)),
                Text::new(self.now.format(&self.cfg.time_format).to_string(), 18.0),
                Spacer::new(Size::splat(Length::Grow)),
            ])
            .size(Size::new(Length::Fit, Length::Grow)),
            Spacer::new(Size::splat(Length::Grow)),
        ])
        .color(Color::BLACK)
        .size(Size::splat(Length::Grow))
        .into()
    }

    fn subscriptions(&self) -> Subscription<Self::Message> {
        fn interval_for_format(fmt: &str) -> Duration {
            if fmt.contains("%f") {
                return Duration::from_millis(100);
            }
            if fmt.contains("%S") {
                return Duration::from_millis(500);
            }
            if fmt.contains("%M") {
                return Duration::from_secs(1);
            }
            if fmt.contains("%H") {
                return Duration::from_secs(60);
            }

            Duration::from_secs(30 * 60)
        }
        Subscription::Interval {
            every: interval_for_format(&self.cfg.time_format),
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
