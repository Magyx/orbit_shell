use std::path::Path;

use libloading::{Library, Symbol};

/// A loaded dynamic library with a safe symbol-resolution helper.
///
/// The library remains open (and its code mapped) for the lifetime of this
/// struct.  Dropping it unmaps the library — callers must ensure no function
/// pointers obtained from it are still in use.
pub struct LibraryHandle {
    lib: Library,
}

impl LibraryHandle {
    // FIX: TOCTOU, use file handle instead
    /// Open `path` with `dlopen`.
    pub fn open(path: &Path) -> Result<Self, String> {
        let lib = unsafe { Library::new(path) }
            .map_err(|e| format!("could not load {}: {e}", path.display()))?;
        Ok(Self { lib })
    }

    /// Resolve a symbol by its null-terminated C name and copy the function
    /// pointer out so it can be called without the `Symbol` borrow.
    ///
    /// # Safety
    /// The caller must ensure `T` matches the actual type of the exported
    /// symbol.  The returned value must not outlive this `LibraryHandle`.
    pub unsafe fn get_fn<T: Copy>(&self, name: &[u8]) -> Result<T, String> {
        unsafe {
            let sym: Symbol<T> = self
                .lib
                .get(name)
                .map_err(|e| format!("symbol {} not found: {e}", String::from_utf8_lossy(name)))?;
            Ok(*sym)
        }
    }
}
