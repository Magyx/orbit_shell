use std::cell::RefCell;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::{any::Any, rc::Rc};

use ui::{
    context as ui_ctx,
    layout::Node,
    primitive::Instance,
    widget::{Element, IntoElement, Widget},
};

use crate::ErasedMsg;

pub trait DynMsg: Send + Debug + 'static {
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn DynMsg>;
}

impl<T> DynMsg for T
where
    T: Any + Debug + Send + Clone + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_box(&self) -> Box<dyn DynMsg> {
        Box::new(self.clone())
    }
}

impl ErasedMsg {
    pub fn new<M: 'static + Debug + Clone + Send>(m: M) -> Self {
        Self { inner: Box::new(m) }
    }
    pub fn message<M: 'static + Clone>(&self) -> Option<M> {
        self.inner.as_any().downcast_ref::<M>().cloned()
    }
    pub fn clone_for_send(&self) -> Self {
        Self {
            inner: self.inner.clone_box(),
        }
    }
}

fn set_ui_state_to<M2, N2>(from: &mut ui_ctx::Context<N2>) -> ui_ctx::Context<M2> {
    let mut tmp = ui_ctx::Context::<M2>::new();
    tmp.mouse_pos = from.mouse_pos;
    tmp.mouse_buttons_down = from.mouse_buttons_down;
    tmp.mouse_buttons_pressed = from.mouse_buttons_pressed;
    tmp.mouse_buttons_released = from.mouse_buttons_released;
    tmp.hot_item = from.hot_item;
    tmp.active_item = from.active_item;
    tmp.kbd_focus_item = from.kbd_focus_item;
    tmp.view_state = std::mem::take(&mut from.view_state);
    tmp
}

fn set_ui_state_back<M2, N2>(to: &mut ui_ctx::Context<N2>, mut from: ui_ctx::Context<M2>) {
    to.mouse_pos = from.mouse_pos;
    to.mouse_buttons_down = from.mouse_buttons_down;
    to.mouse_buttons_pressed = from.mouse_buttons_pressed;
    to.mouse_buttons_released = from.mouse_buttons_released;
    to.hot_item = from.hot_item;
    to.active_item = from.active_item;
    to.kbd_focus_item = from.kbd_focus_item;
    to.view_state = std::mem::take(&mut from.view_state);

    if from.take_redraw() {
        to.request_redraw();
    }
}

pub fn erase_element<M: Send + Debug + Clone + 'static>(elem: Element<M>) -> Element<ErasedMsg> {
    map_element(elem, ErasedMsg::new)
}

pub fn map_element<M, N, F>(elem: Element<M>, f: F) -> Element<N>
where
    M: 'static,
    N: 'static,
    F: Fn(M) -> N + 'static,
{
    Element::new(MappedNode {
        root: Rc::new(RefCell::new(elem)),
        f: Rc::new(f),
        path: Vec::new(),
        children: Vec::new(),
        phantom: PhantomData,
    })
}

struct MappedNode<M, N, F> {
    root: Rc<RefCell<Element<M>>>,
    f: Rc<F>,
    path: Vec<usize>,
    children: Vec<MappedNode<M, N, F>>,
    phantom: PhantomData<N>,
}

impl<M, N, F> IntoElement for MappedNode<M, N, F> {}

impl<M, N, F> MappedNode<M, N, F>
where
    F: Fn(M) -> N + 'static,
{
    fn with_target<R>(&self, mut with: impl FnMut(&mut dyn Widget<M>) -> R) -> R {
        let mut root = self.root.borrow_mut();
        let mut cur: &mut dyn Widget<M> = root.as_mut();
        for &idx in &self.path {
            cur = cur.child_mut(idx);
        }
        with(cur)
    }

    fn ensure_children_sized(&mut self, desired: usize) {
        if self.children.len() == desired {
            return;
        }
        self.children.clear();
        self.children.reserve(desired);
        for i in 0..desired {
            self.children.push(MappedNode {
                root: Rc::clone(&self.root),
                f: Rc::clone(&self.f),
                path: {
                    let mut p = self.path.clone();
                    p.push(i);
                    p
                },
                children: Vec::new(),
                phantom: PhantomData,
            });
        }
    }
}

impl<M, N, F> Widget<N> for MappedNode<M, N, F>
where
    M: 'static,
    N: 'static,
    F: Fn(M) -> N + 'static,
{
    fn layout<'a>(&mut self, ctx: &mut ui_ctx::LayoutCtx<'a, N>) -> Node {
        let mut tmp_ui = set_ui_state_to::<M, N>(ctx.ui);
        let mut m_ctx = ui_ctx::LayoutCtx {
            globals: ctx.globals,
            ui: &mut tmp_ui,
            text: ctx.text,
        };

        let out = self.with_target(|w| w.layout(&mut m_ctx));

        set_ui_state_back(ctx.ui, tmp_ui);
        out
    }

    fn set_layout(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.with_target(|widget| widget.set_layout(x, y, w, h));
    }

    fn supplied_id(&self) -> Option<ui_ctx::Id> {
        self.with_target(|w| w.supplied_id())
    }

    fn set_id(&mut self, id: ui_ctx::Id) {
        self.with_target(|w| w.set_id(id));
    }

    fn child_count(&self) -> usize {
        self.with_target(|w| w.child_count())
    }

    fn child_mut(&mut self, idx: usize) -> &mut dyn Widget<N> {
        let count = self.child_count();
        self.ensure_children_sized(count);
        &mut self.children[idx]
    }

    fn min_height_for_width<'a>(
        &mut self,
        ctx: &mut ui_ctx::LayoutCtx<'a, N>,
        width: i32,
    ) -> Option<i32> {
        let mut tmp_ui = set_ui_state_to::<M, N>(ctx.ui);
        let mut m_ctx = ui_ctx::LayoutCtx {
            globals: ctx.globals,
            ui: &mut tmp_ui,
            text: ctx.text,
        };
        let r = self.with_target(|w| w.min_height_for_width(&mut m_ctx, width));
        set_ui_state_back(ctx.ui, tmp_ui);
        r
    }

    fn children_offset(
        &self,
        view_state: &mut std::collections::HashMap<ui_ctx::Id, Box<dyn Any>>,
    ) -> (i32, i32) {
        self.with_target(|w| w.children_offset(view_state))
    }

    fn paint(&mut self, ctx: &mut ui_ctx::PaintCtx, out: &mut Vec<Instance>) {
        self.with_target(|w| w.paint(ctx, out));
    }

    fn paint_overlay(&mut self, ctx: &mut ui_ctx::PaintCtx, instancess: &mut Vec<Instance>) {
        self.with_target(|w| w.paint_overlay(ctx, instancess));
    }

    fn handle(&mut self, ctx: &mut ui_ctx::EventCtx<N>) {
        let mut tmp_ui = set_ui_state_to::<M, N>(ctx.ui);
        let mut m_ctx = ui_ctx::EventCtx {
            event: ctx.event,
            globals: ctx.globals,
            ui: &mut tmp_ui,
        };

        self.with_target(|w| w.handle(&mut m_ctx));

        let msgs = m_ctx.ui.take();
        for m in msgs {
            ctx.ui.emit((self.f)(m));
        }

        set_ui_state_back(ctx.ui, tmp_ui);
    }
}
