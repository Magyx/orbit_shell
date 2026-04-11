use std::{
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, SystemTime},
};

use calloop::{
    LoopHandle, RegistrationToken,
    timer::{TimeoutAction, Timer},
};
use orbit_api::{BoxStreamFactory, ErasedMsg, SendError, Subscription, SubscriptionSender};
use ui::sctk::state::SctkState;

use crate::{
    event::{self, RuntimeSender},
    module::ModuleId,
};

pub struct StreamHandle {
    pub rx_token: RegistrationToken,
    pub thread: JoinHandle<()>,
}

fn handle_timer(
    tx: &RuntimeSender,
    loop_handle: &mut LoopHandle<SctkState>,
    mid: ModuleId,
    message: ErasedMsg,
    duration: Duration,
    repeat: bool,
) -> RegistrationToken {
    let timer = Timer::from_duration(duration);
    loop_handle
        .insert_source(timer, {
            let ui_tx = tx.clone();
            move |_, _, _| {
                ui_tx.send(event::Event::Ui(event::Ui::Result(
                    event::FromDispatch::Subscription,
                    mid,
                    message.clone_for_send(),
                )));
                if repeat {
                    TimeoutAction::ToDuration(duration)
                } else {
                    TimeoutAction::Drop
                }
            }
        })
        .expect("insert Timer")
}

fn delay_to_next_tick(every: Duration) -> Duration {
    let elapsed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(every);
    let rem = elapsed.as_nanos() % every.as_nanos();
    if rem == 0 {
        every
    } else {
        Duration::from_nanos((every.as_nanos() - rem) as u64)
    }
}

fn handle_timer_synced(
    tx: &RuntimeSender,
    loop_handle: &mut LoopHandle<SctkState>,
    mid: ModuleId,
    message: ErasedMsg,
    duration: Duration,
    repeat: bool,
) -> RegistrationToken {
    let delay = delay_to_next_tick(duration);
    loop_handle
        .insert_source(Timer::from_duration(delay), {
            let ui_tx = tx.clone();
            move |deadline, _, _| {
                ui_tx.send(event::Event::Ui(event::Ui::Result(
                    event::FromDispatch::Subscription,
                    mid,
                    message.clone_for_send(),
                )));
                if repeat {
                    TimeoutAction::ToInstant(deadline + duration)
                } else {
                    TimeoutAction::Drop
                }
            }
        })
        .expect("insert Timer")
}

pub fn handle_subs(
    subs: Vec<Subscription<ErasedMsg>>,
    tx: &RuntimeSender,
    loop_handle: &mut LoopHandle<SctkState>,
    mid: &ModuleId,
    tokens: &mut Vec<RegistrationToken>,
) {
    for sub in subs {
        match sub {
            orbit_api::Subscription::Interval { message, every } => {
                tokens.push(handle_timer(tx, loop_handle, *mid, message, every, true))
            }
            orbit_api::Subscription::Timeout { message, after } => {
                tokens.push(handle_timer(tx, loop_handle, *mid, message, after, false))
            }
            orbit_api::Subscription::SyncedInterval { message, every } => tokens.push(
                handle_timer_synced(tx, loop_handle, *mid, message, every, true),
            ),
            orbit_api::Subscription::SyncedTimeout { message, after } => tokens.push(
                handle_timer_synced(tx, loop_handle, *mid, message, after, false),
            ),
            orbit_api::Subscription::Batch(_) | orbit_api::Subscription::Stream(_) => {
                unreachable!()
            }
            orbit_api::Subscription::None => (),
        }
    }
}

pub fn handle_streams(
    streams: Vec<BoxStreamFactory<ErasedMsg>>,
    tx: &RuntimeSender,
    loop_handle: &mut LoopHandle<SctkState>,
    mid: &ModuleId,
    handles: &mut Vec<StreamHandle>,
) {
    for factory in streams {
        let ui_tx = tx.clone();
        let mid = *mid;

        let (stream_tx, stream_rx) = calloop::channel::channel::<ErasedMsg>();

        let rx_token = loop_handle
            .insert_source(stream_rx, move |evt, _, _| {
                if let calloop::channel::Event::Msg(msg) = evt {
                    ui_tx.send(event::Event::Ui(event::Ui::Result(
                        event::FromDispatch::Subscription,
                        mid,
                        msg,
                    )));
                }
            })
            .expect("insert stream channel");

        let sender = SubscriptionSender::new(Arc::new(move |msg: ErasedMsg| {
            stream_tx.send(msg).map_err(|_| SendError::Disconnected)
        }));

        let future = factory(sender);

        let thread = std::thread::Builder::new()
            .name(format!("orbit-stream-{}", mid.0))
            .spawn(move || futures_lite::future::block_on(future))
            .expect("spawn stream thread");

        handles.push(StreamHandle { rx_token, thread });
    }
}
