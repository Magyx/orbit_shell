use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};

use orbit_api::{
    Engine, Event, OrbitModule, Subscription, Task, orbit_plugin,
    ui::{
        el,
        graphics::TargetId,
        model::{Color, Size, Vec4},
        sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
        widget::{Column, Element, Length, Row, Scrollable, Spacer, Text},
    },
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "orbit_api::serde")]
pub struct Config {
    /// Maximum number of entries to retain.
    pub max_entries: usize,
    /// Panel height in pixels.
    pub height: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_entries: 50,
            height: 300,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Copied(String),
    Clear,
}

#[derive(Default, Debug)]
pub struct ClipHistory {
    cfg: Config,
    history: Vec<String>,
}

impl OrbitModule for ClipHistory {
    type Config = Config;
    type Message = Msg;

    fn cleanup<'a>(&mut self, _engine: &mut Engine<'a>) {
        self.history.clear();
    }

    fn validate_config(cfg: Self::Config) -> Result<(), String> {
        if cfg.max_entries == 0 {
            return Err("max_entries must be at least 1".into());
        }
        if cfg.height < 64 {
            return Err("height must be at least 64 pixels".into());
        }
        Ok(())
    }

    fn apply_config<'a>(
        &mut self,
        _engine: &mut Engine<'a>,
        config: Self::Config,
        options: &mut Options,
    ) -> bool {
        let height_changed = self.cfg.height != config.height;
        self.cfg = config;
        if height_changed {
            if let Options::Layer(layer) = options {
                layer.size.height = self.cfg.height;
            }
            return true;
        }
        false
    }

    fn update<'a>(
        &mut self,
        _tid: TargetId,
        _engine: &mut Engine<'a>,
        event: &Event<Self::Message>,
    ) -> Task<Msg> {
        match event {
            Event::Message(Msg::Copied(text)) => {
                if self.history.first().map(String::as_str) != Some(text.as_str()) {
                    self.history.insert(0, text.clone());
                    self.history.truncate(self.cfg.max_entries);
                }
                Task::RedrawTarget
            }
            Event::Message(Msg::Clear) => {
                self.history.clear();
                Task::RedrawTarget
            }
            _ => Task::None,
        }
    }

    fn view(&self, _tid: &TargetId) -> Element<Self::Message> {
        let mut col = Column::new::<Vec<_>, Element<Msg>>(el!())
            .padding(Vec4::splat(8))
            .size(Size::splat(Length::Grow))
            .color(Color::rgba(20, 20, 20, 242));

        col.push(
            Row::new(el![
                Text::new("Clipboard History", 13.0),
                Spacer::new(Size::splat(Length::Grow)),
                Text::new(format!("{} entries", self.history.len()), 11.0),
            ])
            .size(Size::new(Length::Grow, Length::Fit))
            .padding(Vec4::new(0, 0, 6, 0)),
        );

        if self.history.is_empty() {
            col.push(Text::new("(no clipboard history yet)", 11.0));
        } else {
            let mut entries = Column::new::<Vec<_>, Element<Msg>>(el!())
                .size(Size::new(Length::Grow, Length::Fit));

            for (i, entry) in self.history.iter().enumerate() {
                let first_line = entry.lines().next().unwrap_or("").trim();
                let cutoff = first_line.char_indices().nth(100).map(|(idx, _)| idx);
                let preview = match cutoff {
                    Some(idx) => format!("{}…", &first_line[..idx]),
                    None => first_line.to_owned(),
                };
                let label = if entry.contains('\n') {
                    format!("{i}  {preview}  ↵")
                } else {
                    format!("{i}  {preview}")
                };

                entries.push(
                    Row::new(el![Text::new(label, 11.0)])
                        .size(Size::new(Length::Grow, Length::Fit))
                        .padding(Vec4::new(2, 3, 2, 3))
                        .color(if i % 2 == 0 {
                            Color::rgb(28, 28, 28)
                        } else {
                            Color::rgb(20, 20, 20)
                        }),
                );
            }

            col.push(Scrollable::new(entries).size(Size::new(Length::Grow, Length::Grow)));
        }

        col.into()
    }

    fn subscriptions(&self) -> Subscription<Self::Message> {
        // `wl-paste --watch sh -c 'cat; echo NUL'` re-runs the shell command on
        // every clipboard change, writing the full new content followed by a NUL
        // sentinel line. We accumulate lines between sentinels to reconstruct
        // multi-line clipboard content correctly.
        Subscription::stream(|tx| async move {
            let mut child = match Command::new("wl-paste")
                .args(["--watch", "sh", "-c", "cat; printf '\\x00\\n'"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("clipboard: failed to spawn wl-paste: {e}");
                    return;
                }
            };

            let stdout = child.stdout.take().expect("piped stdout");
            let reader = BufReader::new(stdout);

            let mut buf = String::new();
            for line in reader.lines() {
                match line {
                    Ok(l) if l == "\x00" => {
                        let text = std::mem::take(&mut buf);
                        if tx.send(Msg::Copied(text)).is_err() {
                            let _ = child.kill();
                            return;
                        }
                    }
                    Ok(l) => {
                        if !buf.is_empty() {
                            buf.push('\n');
                        }
                        buf.push_str(&l);
                    }
                    Err(_) => break,
                }
            }

            let _ = child.wait();
        })
    }
}

orbit_plugin! {
    module = ClipHistory,
    manifest = {
        name: "cliphistory",
        commands: [("clear", Msg::Clear)],
        options: Options::Layer(LayerOptions {
            layer: Layer::Top,
            size: Size::new(320, 300),
            anchors: Anchor::TOP | Anchor::RIGHT,
            exclusive_zone: 0,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            namespace: Some("orbit-cliphistory".to_string()),
            output: Some(OutputSet::Active),
        }),
        show_on_startup: false,
    },
}
