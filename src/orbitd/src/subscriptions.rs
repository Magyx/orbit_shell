use std::{
    sync::{Arc, mpsc},
    time::{Duration, SystemTime},
};

use calloop::{
    LoopHandle, RegistrationToken,
    timer::{TimeoutAction, Timer},
};
use orbit_api::{BoxStreamFactory, ErasedMsg, SendError, Subscription, SubscriptionSender};
use ui::sctk::{SctkEvent, state::SctkState};

use crate::{
    event::{self, Event},
    module::ModuleId,
};

fn handle_timer(
    tx: &mpsc::Sender<Event>,
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
                let _ = ui_tx.send(Event::Ui(event::Ui::Module(
                    mid,
                    SctkEvent::message(message.clone_for_send()),
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
    tx: &mpsc::Sender<Event>,
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
                let _ = ui_tx.send(Event::Ui(event::Ui::Module(
                    mid,
                    SctkEvent::message(message.clone_for_send()),
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
    tx: &mpsc::Sender<Event>,
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
    tx: &mpsc::Sender<Event>,
    loop_handle: &mut LoopHandle<SctkState>,
    mid: &ModuleId,
    tokens: &mut Vec<(RegistrationToken, RegistrationToken)>,
) {
    for factory in streams {
        let ui_tx = tx.clone();
        let mid = *mid;

        let (stream_tx, stream_rx) = calloop::channel::channel::<ErasedMsg>();
        let rx_token = loop_handle
            .insert_source(stream_rx, move |evt, _, _| {
                if let calloop::channel::Event::Msg(msg) = evt {
                    let _ = ui_tx.send(Event::Ui(event::Ui::Module(mid, SctkEvent::message(msg))));
                }
            })
            .expect("insert stream channel");

        let sender = SubscriptionSender::new(Arc::new(move |msg: ErasedMsg| {
            stream_tx.send(msg).map_err(|_| SendError::Disconnected)
        }));

        let (executor, scheduler) =
            calloop::futures::executor::<()>().expect("create stream executor");

        let factory_future = factory(sender);
        scheduler
            .schedule(factory_future)
            .expect("schedule stream factory");

        let exec_token = loop_handle
            .insert_source(executor, |_ret, _, _| {
                // Future finished.
            })
            .expect("insert stream executor");
        tokens.push((exec_token, rx_token));
    }
}
