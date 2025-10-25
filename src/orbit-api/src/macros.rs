// TODO: look into making the commands be (name, Msg::Variant) => makes it easy to push into
// existing loop
#[macro_export]
macro_rules! orbit_plugin {
    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            commands: [$($cmd:expr),* $(,)?],
            options: $options:expr,
        },
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [$($cmd),*],
            options = $options,
            show_on_startup = true,
            pipelines = vec![]
        );
    };

    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            commands: [$($cmd:expr),* $(,)?],
            options: $options:expr,
        },
        pipelines = $pipelines:expr,
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [$($cmd),*],
            options = $options,
            show_on_startup = true,
            pipelines = $pipelines
        );
    };

    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            commands: [$($cmd:expr),* $(,)?],
            options: $options:expr,
            show_on_startup: $show:expr,
        },
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [$($cmd),*],
            options = $options,
            show_on_startup = $show,
            pipelines = vec![]
        );
    };

    (
        module = $ty:ty,
        manifest = {
            name: $name:expr,
            commands: [$($cmd:expr),* $(,)?],
            options: $options:expr,
            show_on_startup: $show:expr,
        },
        pipelines = $pipelines:expr,
    ) => {
        orbit_plugin!(@impl
            module = $ty,
            name = $name,
            commands = [$($cmd),*],
            options = $options,
            show_on_startup = $show,
            pipelines = $pipelines
        );
    };

    (@impl
        module = $ty:ty,
        name = $name:expr,
        commands = [$($cmd:expr),*],
        options = $options:expr,
        show_on_startup = $show:expr,
        pipelines = $pipelines:expr
    ) => {
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
        }

        impl $crate::runtime::OrbitModuleDyn for __Wrapper {
            fn manifest(&self) -> &$crate::runtime::Manifest { &self.manifest }

            fn cleanup<'a>(&mut self, engine: &mut Engine<'a, ErasedMsg>) {
                < $ty as $crate::OrbitModule >::cleanup(self.inner_mut(), engine);
            }

            fn init_config(&self, cfg: &mut serde_yml::Value) {
                < $ty as $crate::OrbitModule >::init_config(cfg)
            }
            fn validate_config(&self, cfg: &serde_yml::Value) -> Result<(), String> {
                < $ty as $crate::OrbitModule >::validate_config(cfg)
            }
            fn config_updated<'a>(&mut self, engine: &mut Engine<'a, ErasedMsg>, cfg: &serde_yml::Value) {
                < $ty as $crate::OrbitModule >::config_updated(self.inner_mut(), engine, cfg)
            }

            fn pipelines(&self) -> ::std::vec::Vec<(&'static str, $crate::ui::render::PipelineFactoryFn)> {
                self.pipelines.clone()
            }
            fn update<'a>(
                &mut self,
                tid: TargetId,
                engine: &mut Engine<'a, ErasedMsg>,
                event: &Event<ErasedMsg>,
                orbit: &$crate::OrbitLoop,
            ) -> bool {
                type __Msg = < $ty as $crate::OrbitModule >::Message;

                let mapped: ::std::option::Option<$crate::Event<__Msg>> = match event {
                    $crate::Event::RedrawRequested => Some($crate::Event::RedrawRequested),
                    $crate::Event::Resized { size } => Some($crate::Event::Resized { size: *size }),
                    $crate::Event::CursorMoved { position } => {
                        Some($crate::Event::CursorMoved { position: *position })
                    }
                    $crate::Event::MouseInput { mouse_down } => {
                        Some($crate::Event::MouseInput { mouse_down: *mouse_down })
                    }

                    $crate::Event::Key(k) => Some($crate::Event::Key(k.clone())),
                    $crate::Event::Text(t) => Some($crate::Event::Text(t.clone())),
                    $crate::Event::ModifiersChanged(m) => Some($crate::Event::ModifiersChanged(*m)),

                    $crate::Event::Platform(e) => Some($crate::Event::Platform(e.clone())),
                    $crate::Event::Message(m) => m.message::<__Msg>().map($crate::Event::Message),
                };

                if let Some(evt) = mapped.as_ref() {
                    < $ty as $crate::OrbitModule >::update(self.inner_mut(), tid, engine, evt, orbit)
                } else {
                    false
                }
            }
            fn view(&self, tid: &$crate::ui::graphics::TargetId) -> $crate::ui::widget::Element<$crate::ErasedMsg> {
                let typed = < $ty as $crate::OrbitModule >::view(self.inner_ref(), tid);
                $crate::runtime::erased::erase_element(typed)
            }
            fn subscriptions(&self) -> $crate::Subscription<$crate::ErasedMsg> {
                fn map_sub<M: Send + Clone + ::std::fmt::Debug + 'static>(s: $crate::Subscription<M>) -> $crate::Subscription<$crate::ErasedMsg> {
                    use $crate::Subscription::*;
                    match s {
                        None => None,
                        Interval { every, message } => Interval { every, message: $crate::ErasedMsg::new(message) },
                        Timeout  { after, message } => Timeout  { after, message: $crate::ErasedMsg::new(message) },
                        Batch(v) => Batch(v.into_iter().map(map_sub).collect()),
                    }
                }
                map_sub::<<$ty as $crate::OrbitModule>::Message>(<$ty as $crate::OrbitModule>::subscriptions(self.inner_ref()))
            }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn orbit_module_create() -> *mut dyn $crate::runtime::OrbitModuleDyn {
            let wrapper = __Wrapper {
                manifest: $crate::runtime::Manifest {
                    name: $name,
                    commands: &[$($cmd),*],
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
