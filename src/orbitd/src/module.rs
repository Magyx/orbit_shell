use std::{
    path::{Path, PathBuf},
    ptr::NonNull,
};

use orbit_api::{Engine, runtime::OrbitModuleDyn};
use orbit_common::loader::LibraryHandle;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct ModuleId(pub u32);

pub struct ModuleInfo {
    pub name: String,
    pub path: PathBuf,
    pub inner: Option<Module>,
    pub toggled: bool,
}

impl ModuleInfo {
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            inner: None,
            toggled: false,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.inner.is_some()
    }

    pub fn ensure_loaded(&mut self) -> Result<(), String> {
        if self.inner.is_none() {
            self.inner = Some(Module::new(&self.path)?);
        }

        Ok(())
    }

    pub fn unload(&mut self, engine: &mut Engine<'_>) {
        if let Some(mut module) = self.inner.take() {
            module.as_mut().cleanup(engine);
        }
        self.toggled = false;
    }

    pub fn as_ref(&self) -> &dyn OrbitModuleDyn {
        self.inner.as_ref().expect("module not loaded").as_ref()
    }

    pub fn as_mut(&mut self) -> &mut dyn OrbitModuleDyn {
        self.inner.as_mut().expect("module not loaded").as_mut()
    }
}

type CreateFn = unsafe fn() -> *mut dyn OrbitModuleDyn;
type DestroyFn = unsafe fn(*mut dyn OrbitModuleDyn);

pub struct Module {
    _library: LibraryHandle,
    raw: NonNull<dyn OrbitModuleDyn>,
    destroy: DestroyFn,
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.raw.as_ptr()) }
    }
}

impl Module {
    pub fn new(path: &Path) -> Result<Self, String> {
        tracing::debug!(path = %path.display(), "loading");
        let library = LibraryHandle::open(path)?;

        let create: CreateFn = unsafe { library.get_fn(b"orbit_module_create\0")? };
        let destroy: DestroyFn = unsafe { library.get_fn(b"orbit_module_destroy\0")? };

        let raw = unsafe { create() };
        let raw =
            NonNull::new(raw).ok_or_else(|| "orbit_module_create returned null".to_string())?;

        tracing::info!(module = %unsafe { raw.as_ref() }.manifest().name, "loaded");

        Ok(Self {
            _library: library,
            raw,
            destroy,
        })
    }

    pub fn as_ref(&self) -> &dyn OrbitModuleDyn {
        unsafe { self.raw.as_ref() }
    }

    pub fn as_mut(&mut self) -> &mut dyn OrbitModuleDyn {
        unsafe { self.raw.as_mut() }
    }
}
