use std::{any::Any, cell::RefCell, collections::HashMap, marker::PhantomData, ops::Deref, rc::Rc};

use ui::{
    graphics::TargetId,
    render::texture::{Atlas, TextureHandle},
};

use crate::Engine;

/// Modules can name the type but cannot construct one or key the store with it;
/// all scoped access goes through `OrbitCtl`, which holds the tag for this call.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct OutputTag(u32);

#[derive(Clone, Debug, Default)]
pub struct OutputInfo {
    pub name: Option<String>,
    pub logical_size: Option<(i32, i32)>,
    pub logical_position: Option<(i32, i32)>,
    pub scale: i32,
    tag: OutputTag,
}

impl OutputInfo {
    pub fn new(
        wl_global: u32,
        name: Option<String>,
        logical_size: Option<(i32, i32)>,
        logical_position: Option<(i32, i32)>,
        scale: i32,
    ) -> Self {
        Self {
            name,
            logical_size,
            logical_position,
            scale,
            tag: OutputTag(wl_global),
        }
    }
    pub fn tag(&self) -> OutputTag {
        self.tag
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Global,
    PerOutput,
}

/// A typed, scoped resource key. Both the value type and the output-scoping are
/// fixed at the definition site, so a producer and consumer of the same key
/// physically cannot disagree on either.
pub struct Key<T> {
    pub id: &'static str,
    scope: Scope,
    _ty: PhantomData<fn() -> T>,
}

impl<T> Key<T> {
    /// One slot shared across every output.
    pub const fn global(id: &'static str) -> Self {
        Self {
            id,
            scope: Scope::Global,
            _ty: PhantomData,
        }
    }
    /// One slot per output, resolved against the calling surface's output.
    pub const fn per_output(id: &'static str) -> Self {
        Self {
            id,
            scope: Scope::PerOutput,
            _ty: PhantomData,
        }
    }
}
impl<T> Clone for Key<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Key<T> {}

/// Deferred cleanup for resources that own engine/GPU state. Run by orbitd
/// (which holds the `Engine`) after the call - never inside `Drop`.
pub trait Reclaim: 'static {
    fn reclaim(self: Box<Self>, engine: &mut Engine<'_>);
}
impl Reclaim for TextureHandle {
    fn reclaim(self: Box<Self>, engine: &mut Engine<'_>) {
        engine.unload_texture(*self);
    }
}
impl Reclaim for Atlas {
    fn reclaim(self: Box<Self>, engine: &mut Engine<'_>) {
        let mut atlas = *self;
        engine.destroy_atlas(&mut atlas);
    }
}

type Sink = Rc<RefCell<Vec<Box<dyn Reclaim>>>>;

struct ResCell<T: Reclaim> {
    value: Option<T>,
    sink: Sink,
}
impl<T: Reclaim> Drop for ResCell<T> {
    fn drop(&mut self) {
        if let Some(v) = self.value.take() {
            self.sink.borrow_mut().push(Box::new(v));
        }
    }
}

/// Pinned handle to a published resource. While ANY lease is alive the resource
/// is not reclaimed. Read through `Deref`; never copy the inner value out and
/// cache it - the pin is the `Rc`, not the (possibly `Copy`) value.
pub struct Lease<T: Reclaim> {
    cell: Rc<ResCell<T>>,
}
impl<T: Reclaim> Clone for Lease<T> {
    fn clone(&self) -> Self {
        Self {
            cell: Rc::clone(&self.cell),
        }
    }
}
impl<T: Reclaim> Deref for Lease<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.cell.value.as_ref().expect("present until last drop")
    }
}

type Slot = (&'static str, Option<OutputTag>);

/// Output-scoped, type-erased resource store with refcounted leases. Knows
/// nothing about modules; the only GPU knowledge is the `Reclaim` impl, invoked
/// by the Engine owner. Scoped access is private - only `OrbitCtl` (same module)
/// resolves a key's scope into a slot, so modules can never key it by hand.
#[derive(Default)]
pub struct ResourceManager {
    leased: HashMap<Slot, Box<dyn Any>>, // each box is an `Rc<ResCell<T>>`
    sink: Sink,
}
impl ResourceManager {
    fn publish_at<T: Reclaim>(&mut self, id: &'static str, tag: Option<OutputTag>, val: T) {
        let cell = Rc::new(ResCell {
            value: Some(val),
            sink: Rc::clone(&self.sink),
        });
        self.leased.insert((id, tag), Box::new(cell));
    }
    fn lease_at<T: Reclaim>(&self, id: &'static str, tag: Option<OutputTag>) -> Option<Lease<T>> {
        let cell = self
            .leased
            .get(&(id, tag))?
            .downcast_ref::<Rc<ResCell<T>>>()?;
        Some(Lease {
            cell: Rc::clone(cell),
        })
    }
    fn revoke_at(&mut self, id: &'static str, tag: Option<OutputTag>) {
        self.leased.remove(&(id, tag));
    }

    /// Drop every per-output entry for a tag (call on output-destroyed). Global
    /// (`None`) entries are untouched. Reclaimed once outstanding leases clear.
    pub fn clear_output(&mut self, tag: OutputTag) {
        self.leased.retain(|(_, t), _| *t != Some(tag));
    }

    /// Resources whose last owner dropped. Drain with the Engine in hand:
    /// `for r in rm.take_reclaimable() { r.reclaim(engine); }`
    pub fn take_reclaimable(&mut self) -> Vec<Box<dyn Reclaim>> {
        std::mem::take(&mut *self.sink.borrow_mut())
    }
}

/// Per-call context: the shared store plus this call's surface/output identity.
/// `publish`/`revoke` record a dirty entry; the daemon harvests it after the
/// call and emits the change broadcast. Modules never emit one by hand.
pub struct OrbitCtl<'a> {
    resources: &'a mut ResourceManager,
    tid: Option<TargetId>,
    output: Option<OutputInfo>,
    dirty: Vec<Slot>,
}
impl<'a> OrbitCtl<'a> {
    pub fn new(
        resources: &'a mut ResourceManager,
        tid: Option<TargetId>,
        output: Option<OutputInfo>,
    ) -> Self {
        Self {
            resources,
            tid,
            output,
            dirty: Vec::new(),
        }
    }

    pub fn target(&self) -> Option<TargetId> {
        self.tid
    }
    pub fn output_info(&self) -> Option<&OutputInfo> {
        self.output.as_ref()
    }

    /// Resolve a key's scope against this call's output.
    /// `None` => unresolvable (a per-output key, but this call has no output).
    fn slot<T>(&self, key: Key<T>) -> Option<Slot> {
        let tag = match key.scope {
            Scope::Global => None,
            Scope::PerOutput => Some(self.output.as_ref()?.tag),
        };
        Some((key.id, tag))
    }

    /// Publish for this call's output (or globally, per the key). Returns false
    /// only if a per-output key has no output to resolve against.
    pub fn publish<T: Reclaim>(&mut self, key: Key<T>, val: T) -> bool {
        let Some((id, tag)) = self.slot(key) else {
            return false;
        };
        self.resources.publish_at(id, tag, val);
        self.dirty.push((id, tag));
        true
    }
    pub fn lease<T: Reclaim>(&self, key: Key<T>) -> Option<Lease<T>> {
        let (id, tag) = self.slot(key)?;
        self.resources.lease_at(id, tag)
    }
    pub fn revoke<T>(&mut self, key: Key<T>) {
        if let Some((id, tag)) = self.slot(key) {
            self.resources.revoke_at(id, tag);
            self.dirty.push((id, tag));
        }
    }

    /// Act on an explicit output rather than this call's - for the case where a
    /// module touches a target other than the one being updated (e.g. the
    /// wallpaper Cycle tick arrives with no target).
    pub fn publish_on<T: Reclaim>(&mut self, out: &OutputInfo, key: Key<T>, val: T) {
        let tag = match key.scope {
            Scope::Global => None,
            Scope::PerOutput => Some(out.tag),
        };
        self.resources.publish_at(key.id, tag, val);
        self.dirty.push((key.id, tag));
    }
    pub fn lease_on<T: Reclaim>(&self, out: &OutputInfo, key: Key<T>) -> Option<Lease<T>> {
        let tag = match key.scope {
            Scope::Global => None,
            Scope::PerOutput => Some(out.tag),
        };
        self.resources.lease_at(key.id, tag)
    }

    #[doc(hidden)]
    /// Daemon-only: drain recorded changes to emit as broadcasts.
    pub fn take_dirty(&mut self) -> Vec<(&'static str, Option<OutputTag>)> {
        std::mem::take(&mut self.dirty)
    }
}
