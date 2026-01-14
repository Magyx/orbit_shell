use std::{
    path::{Path, PathBuf},
    ptr::NonNull,
};

use libloading::{Library, Symbol};
use orbit_api::{Engine, runtime::OrbitModuleDyn};

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
    _library: Library,
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
        let library = unsafe { Library::new(path) }.map_err(|e| {
            tracing::error!(path = %path.display(), error = %e, "could not load library");
            format!("Could not load library {}: {}", path.display(), e)
        })?;

        let create: CreateFn = unsafe {
            let sym: Symbol<CreateFn> = library
                .get(b"orbit_module_create\0")
                .map_err(|e| format!("Could not find orbit_module_create symbol: {}", e))?;
            *sym
        };
        let destroy: DestroyFn = unsafe {
            let sym: Symbol<DestroyFn> = library
                .get(b"orbit_module_destroy\0")
                .map_err(|e| format!("Could not find orbit_module_destroy symbol: {}", e))?;
            *sym
        };

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
