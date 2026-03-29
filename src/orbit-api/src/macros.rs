// TODO: look into making the commands be (name, Msg::Variant) => makes it easy to push into
// existing loop
#[macro_export]
macro_rules! orbit_plugin {
    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            options: $options:expr,
        },
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [],
            options = $options,
            show_on_startup = true,
            pipelines = vec![]
        );
    };

    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            commands: [$(($cmd_name:expr, $cmd_msg:expr)),* $(,)?],
            options: $options:expr,
        },
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [$(($cmd_name, $cmd_msg)),*],
            options = $options,
            show_on_startup = true,
            pipelines = vec![]
        );
    };

    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            commands: [$(($cmd_name:expr, $cmd_msg:expr)),* $(,)?],
            options: $options:expr,
        },
        pipelines = $pipelines:expr,
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [$(($cmd_name, $cmd_msg)),*],
            options = $options,
            show_on_startup = true,
            pipelines = $pipelines
        );
    };

    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            options: $options:expr,
            show_on_startup: $show:expr,
        },
        pipelines = $pipelines:expr,
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [],
            options = $options,
            show_on_startup = $show,
            pipelines = $pipelines
        );
    };

    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            options: $options:expr,
            show_on_startup: $show:expr,
        },
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [],
            options = $options,
            show_on_startup = $show,
            pipelines = vec![]
        );
    };

    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            commands: [$(($cmd_name:expr, $cmd_msg:expr)),* $(,)?],
            options: $options:expr,
            show_on_startup: $show:expr,
        },
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [$(($cmd_name, $cmd_msg)),*],
            options = $options,
            show_on_startup = $show,
            pipelines = vec![]
        );
    };

    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            commands: [$(($cmd_name:expr, $cmd_msg:expr)),* $(,)?],
            options: $options:expr,
            show_on_startup: $show:expr,
        },
        pipelines = $pipelines:expr,
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [$(($cmd_name, $cmd_msg)),*],
            options = $options,
            show_on_startup = $show,
            pipelines = $pipelines
        );
    };

    (@impl
        module = $ty:ty,
        name = $name:expr,
        commands = [$(($cmd_name:expr, $cmd_msg:expr)),*],
        options = $options:expr,
        show_on_startup = $show:expr,
        pipelines = $pipelines:expr
    ) => {
        use $crate::{serde, serde_yml::{self, Value}};
        use orbit_api::{Subscription as __Sub, Task as __Task, ErasedMsg as __ErasedMsg, Event as __Event, ui::{graphics::{Engine as __Engine, TargetId as __TargetId}, render::pipeline::Pipeline as __Pipeline}};

        struct __Wrapper {
            manifest: $crate::runtime::Manifest,
            pipelines: ::std::vec::Vec<(&'static str, $crate::ui::render::PipelineFactoryFn)>,
            inner: ::std::sync::OnceLock<$ty>,
        }

        impl __Wrapper {
            #[inline]
            fn inner_mut(&mut self) -> &mut $ty {
                if self.inner.get().is_none() {
                    let _ = self.inner.set(<$ty as ::std::default::Default>::default());
                }
                self.inner.get_mut().expect("OnceLock just initialized")
            }

            #[inline]
            fn inner_ref(&self) -> &$ty {
                self.inner.get_or_init(<$ty as ::std::default::Default>::default)
            }

            fn merged_config_value(raw: &serde_yml::Value) -> serde_yml::Value {

                fn merge(base: Value, overlay: &Value) -> Value {
                    match (base, overlay) {
                        (Value::Mapping(mut b), Value::Mapping(o)) => {
                            for (k, ov) in o {
                                match b.remove(k) {
                                    Some(bv) => { b.insert(k.clone(), merge(bv, ov)); }
                                    None => { b.insert(k.clone(), ov.clone()); }
                                }
                            }
                            Value::Mapping(b)
                        }
                        // Treat `null` as "leave default"
                        (b, Value::Null) => b,
                        // Scalars/sequences: overlay wins
                        (_, o) => o.clone(),
                    }
                }

                let defaults = serde_yml::to_value(
                    <<$ty as $crate::OrbitModule>::Config as ::std::default::Default>::default()
                ).expect("serialize default config");

                merge(defaults, raw)
            }
            fn map_event<M: Send + Clone + 'static>(event: &__Event<__ErasedMsg>) -> Option<__Event<M>> {
                match event {
                    __Event::RedrawRequested => Some(__Event::RedrawRequested),
                    __Event::Resized { size } => Some(__Event::Resized { size: *size }),

                    __Event::CursorMoved { position } => Some(__Event::CursorMoved { position: *position }),
                    __Event::MouseInput { button, state } => Some(__Event::MouseInput {
                        button: *button,
                        state: *state
                    }),
                    __Event::MouseWheel(d) => Some(__Event::MouseWheel(*d)),

                    __Event::Key(k) => Some(__Event::Key(k.clone())),
                    __Event::Text(t) => Some(__Event::Text(t.clone())),
                    __Event::ModifiersChanged(m) => Some(__Event::ModifiersChanged(*m)),
                    __Event::Platform(e) => Some(__Event::Platform(e.clone())),

                    __Event::Message(erased_msg) => {
                        erased_msg.message::<M>().map(__Event::Message)
                    }
                }
            }
            fn map_sub<M: Send + Clone + 'static>(sub: __Sub<M>) -> __Sub<$crate::ErasedMsg> {
                use __Sub::*;
                match sub {
                    None => None,
                    Interval { every, message } => Interval { every, message: $crate::ErasedMsg::new(message) },
                    Timeout  { after, message } => Timeout  { after, message: $crate::ErasedMsg::new(message) },
                    SyncedInterval { every, message } => SyncedInterval { every, message: $crate::ErasedMsg::new(message) },
                    SyncedTimeout { after, message } => SyncedTimeout { after, message: $crate::ErasedMsg::new(message) },
                    Batch(v) => Batch(v.into_iter().map(Self::map_sub).collect()),
                    Stream(typed_factory) => {
                        Stream(::std::boxed::Box::new(
                            move |erased_tx: $crate::SubscriptionSender<$crate::ErasedMsg>| {
                                let typed_tx = $crate::SubscriptionSender::new(
                                    ::std::sync::Arc::new(move |msg: M| {
                                        erased_tx.send($crate::ErasedMsg::new(msg))
                                    }),
                                );
                                typed_factory(typed_tx)
                            },
                        ))
                    }
                }
            }
            fn map_task<M: Send + Clone + 'static>(task: __Task<M>) -> __Task<$crate::ErasedMsg> {
                use __Task::*;
                match task {
                    None => None,
                    Batch(v) => {
                        Batch(v.into_iter().map(Self::map_task).collect())
                    }
                    Spawn(fut) => {
                        let fut = async move {
                            let msg = fut.await;
                            $crate::ErasedMsg::new(msg)
                        };
                        __Task::spawn(fut)
                    }
                    RedrawTarget => RedrawTarget,
                    RedrawModule => RedrawModule,
                    ExitModule => ExitModule,
                    ExitOrbit => ExitOrbit,
                }
            }
        }

        impl $crate::runtime::OrbitModuleDyn for __Wrapper {
            fn manifest(&self) -> &$crate::runtime::Manifest { &self.manifest }

            fn cleanup<'a>(&mut self, engine: &mut __Engine<'a, __ErasedMsg>) {
                < $ty as $crate::OrbitModule >::cleanup(self.inner_mut(), engine);
            }

            fn validate_config_raw(&self, cfg: &serde_yml::Value) -> Result<(), String> {
                < $ty as $crate::OrbitModule >::validate_config_raw(cfg)
            }
            fn validate_config(&self, cfg: &serde_yml::Value) -> Result<(), String> {
                let merged = Self::merged_config_value(cfg);

                let parsed: < $ty as $crate::OrbitModule >::Config =
                    serde_yml::from_value(merged).map_err(|e| format!("config parse failed: {e}"))?;
                < $ty as $crate::OrbitModule >::validate_config(parsed)
            }
            fn apply_config<'a>(
                &mut self,
                engine: &mut __Engine<'a, __ErasedMsg>,
                config: &serde_yml::Value,
                options: &mut $crate::ui::sctk::Options,
            ) -> bool {
                let merged = Self::merged_config_value(config);

                let parsed: < $ty as $crate::OrbitModule >::Config =
                    match serde_yml::from_value(merged) {
                        Ok(v) => v,
                        Err(e) => {
                            $crate::tracing::warn!(
                                module = %self.manifest.name,
                                "config parse failed: {e}"
                            );
                            return false;
                        }
                    };
                < $ty as $crate::OrbitModule >::apply_config(self.inner_mut(), engine, parsed, options)
            }

            fn pipelines(&self) -> ::std::vec::Vec<(&'static str, $crate::ui::render::PipelineFactoryFn)> {
                self.pipelines.clone()
            }
            fn update<'a>(
                &mut self,
                tid: __TargetId,
                engine: &mut __Engine<'a, __ErasedMsg>,
                event: &__Event<__ErasedMsg>,
            ) -> __Task<__ErasedMsg> {
                type __Msg = < $ty as $crate::OrbitModule >::Message;

                match Self::map_event(event) {
                    Some(e) => Self::map_task(< $ty as $crate::OrbitModule >::update(self.inner_mut(), tid, engine, &e)),
                    _ => __Task::None
                }
            }
            fn view(&self, tid: &$crate::ui::graphics::TargetId) -> $crate::ui::widget::Element<$crate::ErasedMsg> {
                let typed = < $ty as $crate::OrbitModule >::view(self.inner_ref(), tid);
                $crate::runtime::erased::erase_element(typed)
            }
            fn command_message(&self, command: &str) -> ::std::option::Option<$crate::ErasedMsg> {
                match command {
                    $(
                        $cmd_name => ::std::option::Option::Some($crate::ErasedMsg::new($cmd_msg)),
                    )*
                    _ => ::std::option::Option::None,
                }
            }
            fn subscriptions(&self) -> __Sub<$crate::ErasedMsg> {
                Self::map_sub::<<$ty as $crate::OrbitModule>::Message>(<$ty as $crate::OrbitModule>::subscriptions(self.inner_ref()))
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn orbit_module_create() -> *mut dyn $crate::runtime::OrbitModuleDyn {
            let wrapper = __Wrapper {
                manifest: $crate::runtime::Manifest {
                    name: $name,
                    commands: &[$($cmd_name),*],
                    options: $options,
                    show_on_startup: $show,
                },
                pipelines: $pipelines,
                inner: ::std::sync::OnceLock::new(),
            };
            let obj: ::std::boxed::Box<dyn $crate::runtime::OrbitModuleDyn> = ::std::boxed::Box::new(wrapper);
            ::std::boxed::Box::into_raw(obj)
        }

        #[unsafe(no_mangle)]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn orbit_module_destroy(ptr: *mut dyn $crate::runtime::OrbitModuleDyn) {
            if !ptr.is_null() {
                unsafe { drop(::std::boxed::Box::<dyn $crate::runtime::OrbitModuleDyn>::from_raw(ptr)) }
            }
        }
    };
}
