use orbit_api::{
    Engine, Event, OrbitModule, Task, orbit_plugin,
    ui::{
        el,
        event::{KeyEvent, KeyState, LogicalKey},
        graphics::TargetId,
        model::{Color, Size},
        sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
        widget::{Column, Element, Length, Row, Spacer, Text},
    },
};
use pam::Client;

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "orbit_api::serde")]
pub struct Config {
    /// Message shown at the top of the lock screen.
    pub message: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            message: "Welcome {username}!".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    AuthResult(bool),
}

#[derive(Debug, Default, PartialEq)]
enum AuthState {
    #[default]
    Idle,
    Checking,
    Failed,
}

#[derive(Debug, Default)]
pub struct LockScreen {
    cfg: Config,
    username: String,
    password: String,
    state: AuthState,
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
        false
    }

    fn update<'a>(
        &mut self,
        _tid: TargetId,
        _engine: &mut Engine<'a>,
        event: &Event<Self::Message>,
    ) -> Task<Msg> {
        match event {
            // Resolve username lazily on first draw so it is always fresh.
            Event::RedrawRequested if self.username.is_empty() => {
                self.username = current_username();
                Task::RedrawTarget
            }

            Event::Key(KeyEvent {
                state: KeyState::Pressed,
                logical_key: key,
                ..
            }) => {
                // Block input while PAM is running.
                if self.state == AuthState::Checking {
                    return Task::None;
                }

                match key {
                    LogicalKey::Enter => {
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
                        self.password.push_str(c);
                        self.state = AuthState::Idle;
                        Task::RedrawTarget
                    }

                    LogicalKey::Space => {
                        self.password.push(' ');
                        self.state = AuthState::Idle;
                        Task::RedrawTarget
                    }

                    _ => Task::None,
                }
            }

            Event::Message(Msg::AuthResult(true)) => {
                self.password.clear();
                self.state = AuthState::Idle;
                Task::ExitModule
            }

            Event::Message(Msg::AuthResult(false)) => {
                self.password.clear();
                self.state = AuthState::Failed;
                Task::RedrawTarget
            }

            _ => Task::None,
        }
    }

    fn view(&self, _tid: &TargetId) -> Element<Self::Message> {
        let lock_message = self.cfg.message.replace("{username}", &self.username);
        let dots: String = "●".repeat(self.password.len());
        let status_color = match self.state {
            AuthState::Checking => Color::rgb(180, 180, 180),
            AuthState::Failed => Color::rgb(220, 80, 80),
            AuthState::Idle => Color::WHITE,
        };

        Column::new(el![
            Spacer::new(Size::splat(Length::Grow)),
            Spacer::new(Size::splat(Length::Grow)),
            // Lock message
            Row::new(el![
                Spacer::new(Size::splat(Length::Grow)),
                Text::new(lock_message, 20.0)
                    .size(Size::splat(Length::Fit))
                    .wrap(orbit_api::ui::model::Wrap::None),
                Spacer::new(Size::splat(Length::Grow)),
            ])
            .size(Size::new(Length::Grow, Length::Fit)),
            Spacer::new(Size::splat(Length::Grow)),
            // Password dots
            Row::new(el![
                Spacer::new(Size::splat(Length::Grow)),
                Text::new(dots, 18.0)
                    .size(Size::splat(Length::Fit))
                    .color(status_color)
                    .family(orbit_api::ui::model::Family::Name("Noto Sans Mono".into())),
                Spacer::new(Size::splat(Length::Grow)),
            ])
            .size(Size::new(Length::Grow, Length::Fit)),
            Spacer::new(Size::splat(Length::Grow)),
            Spacer::new(Size::splat(Length::Grow)),
        ])
        .color(Color::rgba(0, 0, 0, 240))
        .size(Size::splat(Length::Grow))
        .into()
    }
}

orbit_plugin! {
    module = LockScreen,
    manifest = {
        name: "lockscreen",
        commands: [],
        options: Options::Layer(LayerOptions {
            layer: Layer::Overlay,
            size: Size::new(0, 0),
            anchors: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: -1,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            namespace: Some("orbit-lockscreen".to_string()),
            output: Some(OutputSet::All),
        }),
        show_on_startup: false,
    },
}
