use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use orbit_api::{
    Engine, Event, Lease, OrbitCtl, OrbitModule, Subscription, Task, orbit_config, orbit_plugin,
    ui::{
        el,
        event::{KeyEvent, KeyState, LogicalKey},
        graphics::TargetId,
        model::{Color, Size},
        render::texture::TextureHandle,
        sctk::{LockOptions, Options, OutputSet},
        widget::{Column, ContentFit, Element, Length, Overlay, Rectangle, Row, Spacer, Text},
    },
};
use orbit_keys::WALLPAPER_TEX;
use pam::Client;

mod pipeline;
mod widgets;

use widgets::*;

fn default_message() -> String {
    "Welcome {username}!".into()
}
fn default_idle_duration() -> String {
    "5m".into()
}

#[orbit_config]
pub struct Config {
    #[serde(default = "default_message")]
    pub message: String,
    #[serde(default = "default_idle_duration")]
    pub idle: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            message: default_message(),
            idle: default_idle_duration(),
        }
    }
}

fn current_username() -> String {
    if let Ok(u) = std::env::var("USER")
        && !u.is_empty()
    {
        return u;
    }
    // Fall back to a POSIX uid → pw_name lookup.
    // SAFETY: getuid() is always safe; getpwuid() returns a pointer to static
    // storage or NULL; CStr::from_ptr is valid for as long as we hold no other
    // pw calls on this thread.
    unsafe {
        let uid = libc::getuid();
        let pw = libc::getpwuid(uid);
        if !pw.is_null()
            && let Ok(s) = std::ffi::CStr::from_ptr((*pw).pw_name).to_str()
        {
            return s.to_owned();
        }
    }
    "user".to_owned()
}

fn authenticate(username: &str, password: String) -> bool {
    let mut auth = match Client::with_password("login") {
        Ok(a) => a,
        Err(e) => {
            println!("lockscreen: failed to create PAM authenticator: {e}");
            return false;
        }
    };
    auth.conversation_mut().set_credentials(username, &password);
    match auth.authenticate() {
        Ok(()) => true,
        Err(e) => {
            println!("lockscreen: PAM authentication failed: {e}");
            false
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    AuthResult(bool),
    CheckAfk,
}

#[derive(Debug, Default, PartialEq)]
enum AuthState {
    #[default]
    Idle,
    Checking,
    Failed,
}

pub struct LockScreen {
    cfg: Config,
    username: String,
    password: String,
    state: AuthState,

    bg: HashMap<TargetId, Lease<TextureHandle>>,

    last_event: Instant,
    idle_time: Option<Duration>,
    idle: bool,
}

impl Default for LockScreen {
    fn default() -> Self {
        Self {
            cfg: Default::default(),
            username: current_username(),
            password: Default::default(),
            state: Default::default(),
            bg: HashMap::new(),
            last_event: Instant::now(),
            idle_time: None,
            idle: false,
        }
    }
}

impl LockScreen {
    fn ensure_bg(&mut self, ctl: &OrbitCtl<'_>) {
        if let Some(tid) = ctl.target()
            && !self.bg.contains_key(&tid)
            && let Some(lease) = ctl.lease(WALLPAPER_TEX)
        {
            self.bg.insert(tid, lease);
        }
    }
}

impl OrbitModule for LockScreen {
    type Config = Config;
    type Message = Msg;

    fn cleanup<'a>(&mut self, _engine: &mut Engine<'a>) {
        self.password.clear();
        self.state = AuthState::Idle;
    }

    fn apply_config<'a>(
        &mut self,
        _engine: &mut Engine<'a>,
        config: Self::Config,
        _options: &mut Options,
    ) -> bool {
        self.cfg = config;
        self.idle_time = duration_str::parse(&self.cfg.idle).ok();
        false
    }

    fn update<'a>(
        &mut self,
        ctl: &mut orbit_api::OrbitCtl,
        _tid: Option<TargetId>,
        _engine: &mut Engine<'a>,
        event: &Event<Self::Message>,
    ) -> Task<Msg> {
        self.ensure_bg(ctl);

        let mut task = match event {
            Event::Key(KeyEvent {
                state: KeyState::Pressed,
                logical_key: key,
                ..
            }) => {
                // Block input while PAM is running.
                if self.state == AuthState::Checking {
                    return Task::None;
                }

                let mut is_enter = false;
                let task = match key {
                    LogicalKey::Enter => {
                        is_enter = true;

                        Task::None
                    }

                    LogicalKey::Backspace => {
                        self.password.pop();
                        Task::RedrawTarget
                    }

                    // Escape clears the attempt but keeps the screen locked.
                    LogicalKey::Escape => {
                        self.password.clear();
                        self.state = AuthState::Idle;
                        Task::RedrawTarget
                    }

                    LogicalKey::Character(c) => {
                        if c == "\r" {
                            is_enter = true;
                            Task::None
                        } else {
                            self.password.push_str(c);
                            self.state = AuthState::Idle;
                            Task::RedrawTarget
                        }
                    }

                    LogicalKey::Space => {
                        self.password.push(' ');
                        self.state = AuthState::Idle;
                        Task::RedrawTarget
                    }

                    _ => Task::None,
                };

                if is_enter {
                    if self.password.is_empty() {
                        return Task::None;
                    }
                    self.state = AuthState::Checking;
                    let username = self.username.clone();
                    let password = std::mem::take(&mut self.password);
                    Task::batch([
                        Task::RedrawTarget,
                        Task::spawn(async move {
                            let ok = authenticate(&username, password);
                            Msg::AuthResult(ok)
                        }),
                    ])
                } else {
                    task
                }
            }

            Event::Message(msg) => match msg {
                Msg::AuthResult(true) => {
                    self.password.clear();
                    self.state = AuthState::Idle;
                    Task::ExitModule
                }
                Msg::AuthResult(false) => {
                    self.password.clear();
                    self.state = AuthState::Failed;
                    Task::RedrawTarget
                }

                Msg::CheckAfk => {
                    if let Some(idle_time) = self.idle_time
                        && self.last_event.elapsed() >= idle_time
                    {
                        if !self.idle {
                            self.idle = true;
                            Task::RedrawModule
                        } else {
                            Task::None
                        }
                    } else {
                        Task::None
                    }
                }
            },

            _ => Task::None,
        };

        if !matches!(
            event,
            Event::Message(_) | Event::Platform(_) | Event::RedrawRequested
        ) {
            if self.idle {
                task = Task::batch([task, Task::RedrawModule])
            }
            self.idle = false;
            self.last_event = Instant::now();
        }

        task
    }

    fn on_broadcast(
        &mut self,
        ctl: &mut OrbitCtl<'_>,
        _tid: Option<orbit_api::ui::graphics::TargetId>,
        key: &'static str,
    ) -> Task<Self::Message> {
        if key == WALLPAPER_TEX.id
            && let Some(tid) = ctl.target()
            && let Some(lease) = ctl.lease(WALLPAPER_TEX)
        {
            self.bg.insert(tid, lease);
            return Task::RedrawTarget;
        }
        Task::None
    }

    fn view(&self, tid: &TargetId, _theme: &orbit_api::ui::theme::Theme) -> Element<Self::Message> {
        if self.idle {
            return Rectangle::new(Size::splat(Length::Grow), Color::BLACK).into();
        }

        let lock_message = self.cfg.message.replace("{username}", &self.username);
        let dots: String = "●".repeat(self.password.len());
        let status_color = match self.state {
            AuthState::Checking => Color::rgb(180, 180, 180),
            AuthState::Failed => Color::rgb(220, 80, 80),
            AuthState::Idle => Color::WHITE,
        };

        let column = Column::new(el![
            Spacer::new(Size::splat(Length::Grow)),
            Spacer::new(Size::splat(Length::Grow)),
            // Lock message
            Row::new(el![
                Spacer::new(Size::splat(Length::Grow)),
                Text::h2(lock_message)
                    .size(Size::splat(Length::Fit))
                    .wrap(orbit_api::ui::model::Wrap::None),
                Spacer::new(Size::splat(Length::Grow)),
            ])
            .size(Size::new(Length::Grow, Length::Fit)),
            Spacer::new(Size::splat(Length::Grow)),
            // Password dots
            Row::new(el![
                Spacer::new(Size::splat(Length::Grow)),
                Text::h3(dots)
                    .size(Size::splat(Length::Fit))
                    .color(status_color)
                    .family(orbit_api::ui::model::Family::Name("Noto Sans Mono".into())),
                Spacer::new(Size::splat(Length::Grow)),
            ])
            .size(Size::new(Length::Grow, Length::Fit)),
            Spacer::new(Size::splat(Length::Grow)),
            Spacer::new(Size::splat(Length::Grow)),
        ]);

        match self.bg.get(tid) {
            Some(lease) => {
                let tex = **lease;
                Overlay::new(el![
                    BlurImage::new(Size::splat(Length::Grow), tex)
                        .fit(ContentFit::Cover)
                        .tint(Color::from_hex(0x5e5e5e))
                        .strength(5),
                    column.size(Size::splat(Length::Grow))
                ])
                .size(Size::splat(Length::Grow))
                .into()
            }
            None => column.color(Color::BLACK).into(),
        }
    }

    fn subscriptions(&self) -> Subscription<Self::Message> {
        Subscription::SyncedInterval {
            every: Duration::from_secs(1),
            message: Msg::CheckAfk,
        }
    }
}

orbit_plugin! {
    module: LockScreen,
    name: "lockscreen",
    options: Options::Lock(LockOptions {
        size: Size::new(0, 0),
        output: Some(OutputSet::All),
    }),
    pipelines: orbit_api::ui::pipeline_factories!["blur" => pipeline::BlurPipeline],
}
