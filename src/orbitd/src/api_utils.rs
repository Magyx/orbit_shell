use orbit_api::{BoxFuture, ErasedMsg, Task};

pub enum Action {
    ExitOrbit,
    ExitModule,
    RedrawModule,
    None,
}

#[derive(Default)]
pub struct UnraveledTask {
    exit_orbit: bool,
    exit_module: bool,
    redraw_module: bool,

    pub tasks: Option<Vec<BoxFuture<ErasedMsg>>>,
}

impl UnraveledTask {
    pub fn action(&mut self) -> Action {
        if self.exit_orbit {
            self.tasks.take();
            Action::ExitOrbit
        } else if self.exit_module {
            self.tasks.take();
            Action::ExitModule
        } else if self.redraw_module {
            Action::RedrawModule
        } else {
            Action::None
        }
    }
}

pub fn unravel_task(t: Task<ErasedMsg>) -> (UnraveledTask, bool) {
    fn unravel_task_internal(t: Task<ErasedMsg>, ut: &mut UnraveledTask, redraw: &mut bool) {
        match t {
            Task::None => (),
            Task::Batch(tasks) => {
                for task in tasks {
                    unravel_task_internal(task, ut, redraw);
                }
            }
            Task::RedrawTarget => *redraw = true,
            Task::RedrawModule => ut.redraw_module = true,
            Task::ExitModule => ut.exit_module = true,
            Task::ExitOrbit => ut.exit_orbit = true,
            Task::Spawn(pin) => ut.tasks.get_or_insert_default().push(pin),
        }
    }

    let mut utask = UnraveledTask::default();
    let mut redraw = false;
    unravel_task_internal(t, &mut utask, &mut redraw);
    (utask, redraw)
}
