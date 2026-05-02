use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Expr, Ident, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

struct PluginInput {
    module_ty: syn::Type,
    name: Expr,
    options: Expr,
    commands: Vec<(Expr, Expr)>,
    show_on_startup: Expr,
    persistent_state: Expr,
    pipelines: Expr,
}

enum Field {
    Module(syn::Type),
    Name(Expr),
    Options(Expr),
    Commands(Vec<(Expr, Expr)>),
    ShowOnStartup(Expr),
    PersistentState(Expr),
    Pipelines(Expr),
}

impl Parse for Field {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key: Ident = input.parse()?;
        input.parse::<Token![:]>()?;

        match key.to_string().as_str() {
            "module" => Ok(Field::Module(input.parse()?)),
            "name" => Ok(Field::Name(input.parse()?)),
            "options" => Ok(Field::Options(input.parse()?)),
            "show_on_startup" => Ok(Field::ShowOnStartup(input.parse()?)),
            "persistent_state" => Ok(Field::PersistentState(input.parse()?)),
            "pipelines" => Ok(Field::Pipelines(input.parse()?)),
            "commands" => {
                let content;
                syn::bracketed!(content in input);
                let pairs: Punctuated<CommandPair, Token![,]> =
                    content.parse_terminated(CommandPair::parse, Token![,])?;
                Ok(Field::Commands(
                    pairs.into_iter().map(|p| (p.name, p.msg)).collect(),
                ))
            }
            other => Err(syn::Error::new(
                key.span(),
                format!("unknown field `{other}` in orbit_plugin!"),
            )),
        }
    }
}

struct CommandPair {
    name: Expr,
    msg: Expr,
}

impl Parse for CommandPair {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        syn::parenthesized!(content in input);
        let name: Expr = content.parse()?;
        content.parse::<Token![,]>()?;
        let msg: Expr = content.parse()?;
        Ok(CommandPair { name, msg })
    }
}

impl Parse for PluginInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let fields: Punctuated<Field, Token![,]> =
            input.parse_terminated(Field::parse, Token![,])?;

        let mut module_ty: Option<syn::Type> = None;
        let mut name: Option<Expr> = None;
        let mut options: Option<Expr> = None;
        let mut commands: Vec<(Expr, Expr)> = Vec::new();
        let mut show_on_startup: Option<Expr> = None;
        let mut persistent_state: Option<Expr> = None;
        let mut pipelines: Option<Expr> = None;

        for field in fields {
            match field {
                Field::Module(v) => module_ty = Some(v),
                Field::Name(v) => name = Some(v),
                Field::Options(v) => options = Some(v),
                Field::Commands(v) => commands = v,
                Field::ShowOnStartup(v) => show_on_startup = Some(v),
                Field::PersistentState(v) => persistent_state = Some(v),
                Field::Pipelines(v) => pipelines = Some(v),
            }
        }

        let module_ty =
            module_ty.ok_or_else(|| input.error("orbit_plugin! requires `module: Type`"))?;
        let name = name.ok_or_else(|| input.error("orbit_plugin! requires `name: \"...\"`"))?;
        let options =
            options.ok_or_else(|| input.error("orbit_plugin! requires `options: ...`"))?;

        Ok(PluginInput {
            module_ty,
            name,
            options,
            commands,
            show_on_startup: show_on_startup.unwrap_or_else(|| syn::parse_quote!(false)),
            persistent_state: persistent_state.unwrap_or_else(|| syn::parse_quote!(false)),
            pipelines: pipelines.unwrap_or_else(|| syn::parse_quote!(vec![])),
        })
    }
}

pub fn orbit_plugin_impl(input: TokenStream) -> TokenStream {
    let PluginInput {
        module_ty,
        name,
        options,
        commands,
        show_on_startup,
        persistent_state,
        pipelines,
    } = syn::parse_macro_input!(input as PluginInput);

    let cmd_names: Vec<&Expr> = commands.iter().map(|(n, _)| n).collect();
    let cmd_names2 = cmd_names.clone();
    let cmd_msgs: Vec<&Expr> = commands.iter().map(|(_, m)| m).collect();

    let output = quote! {
        #[doc(hidden)]
        use orbit_api::{
            Subscription as __Sub,
            Task as __Task,
            ErasedMsg as __ErasedMsg,
            Event as __Event,
            ui::{
                graphics::{Engine as __Engine, TargetId as __TargetId},
                render::pipeline::Pipeline as __Pipeline,
            },
        };

        #[doc(hidden)]
        struct __Wrapper {
            manifest: orbit_api::runtime::Manifest,
            pipelines: ::std::vec::Vec<(&'static str, orbit_api::ui::render::PipelineFactoryFn)>,
            inner: ::std::sync::OnceLock<#module_ty>,
        }

        impl __Wrapper {
            #[inline]
            fn inner_mut(&mut self) -> &mut #module_ty {
                if self.inner.get().is_none() {
                    let _ = self.inner.set(<#module_ty as ::std::default::Default>::default());
                }
                self.inner.get_mut().expect("OnceLock just initialized")
            }

            #[inline]
            fn inner_ref(&self) -> &#module_ty {
                self.inner.get_or_init(<#module_ty as ::std::default::Default>::default)
            }

            fn merged_config_value(raw: &orbit_api::serde_yml::Value) -> orbit_api::serde_yml::Value {
                fn merge(
                    base: orbit_api::serde_yml::Value,
                    overlay: &orbit_api::serde_yml::Value,
                ) -> orbit_api::serde_yml::Value {
                    match (base, overlay) {
                        (
                            orbit_api::serde_yml::Value::Mapping(mut b),
                            orbit_api::serde_yml::Value::Mapping(o),
                        ) => {
                            for (k, ov) in o {
                                match b.remove(k) {
                                    Some(bv) => {
                                        b.insert(k.clone(), merge(bv, ov));
                                    }
                                    None => {
                                        b.insert(k.clone(), ov.clone());
                                    }
                                }
                            }
                            orbit_api::serde_yml::Value::Mapping(b)
                        }
                        (b, orbit_api::serde_yml::Value::Null) => b,
                        (_, o) => o.clone(),
                    }
                }

                let defaults = orbit_api::serde_yml::to_value(
                    <<#module_ty as orbit_api::OrbitModule>::Config as ::std::default::Default>::default()
                )
                .expect("serialize default config");

                merge(defaults, raw)
            }

            fn map_event<M: Send + Clone + 'static>(
                event: &__Event<__ErasedMsg>,
            ) -> Option<__Event<M>> {
                match event {
                    __Event::RedrawRequested => Some(__Event::RedrawRequested),
                    __Event::Resized { size } => Some(__Event::Resized { size: *size }),
                    __Event::CursorMoved { position } => {
                        Some(__Event::CursorMoved { position: *position })
                    }
                    __Event::MouseInput { button, state } => Some(__Event::MouseInput {
                        button: *button,
                        state: *state,
                    }),
                    __Event::MouseWheel(d) => Some(__Event::MouseWheel(*d)),
                    __Event::Key(k) => Some(__Event::Key(k.clone())),
                    __Event::Text(t) => Some(__Event::Text(t.clone())),
                    __Event::ModifiersChanged(m) => Some(__Event::ModifiersChanged(*m)),
                    __Event::Platform(e) => Some(__Event::Platform(e.clone())),
                    __Event::Message(erased_msg) => erased_msg.message::<M>().map(__Event::Message),
                }
            }

            fn map_sub<M: Send + Clone + 'static>(sub: __Sub<M>) -> __Sub<orbit_api::ErasedMsg> {
                use __Sub::*;
                match sub {
                    None => None,
                    Interval { every, message } => Interval {
                        every,
                        message: orbit_api::ErasedMsg::new(message),
                    },
                    Timeout { after, message } => Timeout {
                        after,
                        message: orbit_api::ErasedMsg::new(message),
                    },
                    SyncedInterval { every, message } => SyncedInterval {
                        every,
                        message: orbit_api::ErasedMsg::new(message),
                    },
                    SyncedTimeout { after, message } => SyncedTimeout {
                        after,
                        message: orbit_api::ErasedMsg::new(message),
                    },
                    Batch(v) => Batch(v.into_iter().map(Self::map_sub).collect()),
                    Stream(typed_factory) => {
                        Stream(::std::boxed::Box::new(
                            move |erased_tx: orbit_api::SubscriptionSender<orbit_api::ErasedMsg>| {
                                let typed_tx = orbit_api::SubscriptionSender::new(
                                    ::std::sync::Arc::new(move |msg: M| {
                                        erased_tx.send(orbit_api::ErasedMsg::new(msg))
                                    }),
                                );
                                typed_factory(typed_tx)
                            },
                        ))
                    }
                }
            }

            fn map_task<M: Send + Clone + 'static>(
                task: __Task<M>,
            ) -> __Task<orbit_api::ErasedMsg> {
                use __Task::*;
                match task {
                    None => None,
                    Batch(v) => Batch(v.into_iter().map(Self::map_task).collect()),
                    Spawn(fut) => {
                        let fut = async move {
                            let msg = fut.await;
                            orbit_api::ErasedMsg::new(msg)
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

        impl orbit_api::runtime::OrbitModuleDyn for __Wrapper {
            fn manifest(&self) -> &orbit_api::runtime::Manifest {
                &self.manifest
            }

            fn cleanup<'a>(&mut self, engine: &mut __Engine<'a, __ErasedMsg>) {
                <#module_ty as orbit_api::OrbitModule>::cleanup(self.inner_mut(), engine);
            }

            fn validate_config_raw(
                &self,
                cfg: &orbit_api::serde_yml::Value,
            ) -> Result<(), String> {
                <#module_ty as orbit_api::OrbitModule>::validate_config_raw(cfg)
            }

            fn validate_config(
                &self,
                cfg: &orbit_api::serde_yml::Value,
            ) -> Result<(), String> {
                let merged = Self::merged_config_value(cfg);
                let parsed: <#module_ty as orbit_api::OrbitModule>::Config =
                    orbit_api::serde_yml::from_value(merged)
                        .map_err(|e| format!("config parse failed: {e}"))?;
                <#module_ty as orbit_api::OrbitModule>::validate_config(parsed)
            }

            fn apply_config<'a>(
                &mut self,
                engine: &mut __Engine<'a, __ErasedMsg>,
                config: &orbit_api::serde_yml::Value,
                options: &mut orbit_api::ui::sctk::Options,
            ) -> bool {
                let merged = Self::merged_config_value(config);
                let parsed: <#module_ty as orbit_api::OrbitModule>::Config =
                    match orbit_api::serde_yml::from_value(merged) {
                        Ok(v) => v,
                        Err(e) => {
                            orbit_api::tracing::warn!(
                                module = %self.manifest.name,
                                "config parse failed: {e}"
                            );
                            return false;
                        }
                    };
                <#module_ty as orbit_api::OrbitModule>::apply_config(
                    self.inner_mut(),
                    engine,
                    parsed,
                    options,
                )
            }

            fn pipelines(
                &self,
            ) -> ::std::vec::Vec<(&'static str, orbit_api::ui::render::PipelineFactoryFn)> {
                self.pipelines.clone()
            }

            fn update<'a>(
                &mut self,
                tid: Option<__TargetId>,
                engine: &mut __Engine<'a, __ErasedMsg>,
                event: &__Event<__ErasedMsg>,
            ) -> __Task<__ErasedMsg> {
                type __Msg = <#module_ty as orbit_api::OrbitModule>::Message;
                match Self::map_event::<__Msg>(event) {
                    Some(e) => Self::map_task(
                        <#module_ty as orbit_api::OrbitModule>::update(
                            self.inner_mut(),
                            tid,
                            engine,
                            &e,
                        ),
                    ),
                    _ => __Task::None,
                }
            }

            fn view(
                &self,
                tid: &orbit_api::ui::graphics::TargetId,
            ) -> orbit_api::ui::widget::Element<orbit_api::ErasedMsg> {
                let typed = <#module_ty as orbit_api::OrbitModule>::view(self.inner_ref(), tid);
                orbit_api::runtime::erased::erase_element(typed)
            }

            fn command_message(
                &self,
                command: &str,
            ) -> ::std::option::Option<orbit_api::ErasedMsg> {
                match command {
                    #(
                        #cmd_names => {
                            ::std::option::Option::Some(orbit_api::ErasedMsg::new(#cmd_msgs))
                        }
                    )*
                    _ => ::std::option::Option::None,
                }
            }

            fn subscriptions(&self) -> __Sub<orbit_api::ErasedMsg> {
                Self::map_sub::<<#module_ty as orbit_api::OrbitModule>::Message>(
                    <#module_ty as orbit_api::OrbitModule>::subscriptions(self.inner_ref()),
                )
            }
        }

        #[doc(hidden)]
        #[unsafe(no_mangle)]
        pub extern "C" fn orbit_module_create() -> *mut dyn orbit_api::runtime::OrbitModuleDyn {
            let wrapper = __Wrapper {
                manifest: orbit_api::runtime::Manifest {
                    name: #name,
                    commands: &[#(#cmd_names2),*],
                    options: #options,
                    show_on_startup: #show_on_startup,
                    persistent_state: #persistent_state,
                },
                pipelines: #pipelines,
                inner: ::std::sync::OnceLock::new(),
            };
            let obj: ::std::boxed::Box<dyn orbit_api::runtime::OrbitModuleDyn> =
                ::std::boxed::Box::new(wrapper);
            ::std::boxed::Box::into_raw(obj)
        }

        #[doc(hidden)]
        #[unsafe(no_mangle)]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn orbit_module_destroy(
            ptr: *mut dyn orbit_api::runtime::OrbitModuleDyn,
        ) {
            if !ptr.is_null() {
                unsafe {
                    drop(
                        ::std::boxed::Box::<dyn orbit_api::runtime::OrbitModuleDyn>::from_raw(ptr),
                    )
                }
            }
        }

        #[doc(hidden)]
        #[unsafe(no_mangle)]
        pub extern "C" fn orbit_schema() -> *const std::ffi::c_char {
            use orbit_api::schemars;
            static SCHEMA: std::sync::OnceLock<std::ffi::CString> = std::sync::OnceLock::new();
            SCHEMA
                .get_or_init(|| {
                    let root =
                        schemars::schema_for!(<#module_ty as orbit_api::OrbitModule>::Config);
                    let json = orbit_api::serde_json::to_string(&root)
                        .expect("schema serialization");
                    std::ffi::CString::new(json).expect("schema contains null byte")
                })
                .as_ptr()
        }
    };

    TokenStream::from(output)
}
