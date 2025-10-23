use ui::{
    context as ui_ctx,
    model::{Position, Size},
    primitive::Instance,
    widget::{Element, Layout, Widget},
};

use crate::ErasedMsg;

impl ErasedMsg {
    pub fn new<M: 'static + Send>(m: M) -> Self {
        Self { inner: Box::new(m) }
    }
    pub fn message<M: 'static + Clone>(&self) -> Option<M> {
        self.inner.downcast_ref::<M>().cloned()
    }
}

pub fn map_element<M, N, F>(elem: Element<M>, f: F) -> Element<N>
where
    M: 'static,
    N: 'static,
    F: Fn(M) -> N + 'static,
{
    struct Map<M, N> {
        inner: Element<M>,
        map: Box<dyn Fn(M) -> N>,
    }

    impl<M, N> Map<M, N> {
        fn new(inner: Element<M>, f: impl Fn(M) -> N + 'static) -> Self {
            Self {
                inner,
                map: Box::new(f),
            }
        }
    }

    impl<M, N> Widget<N> for Map<M, N>
    where
        M: 'static,
        N: 'static,
    {
        /* ----- identity ----- */
        fn id(&self) -> ui_ctx::Id {
            self.inner.id()
        }
        fn position(&self) -> &Position<i32> {
            self.inner.position()
        }
        fn layout(&self) -> &Layout {
            self.inner.layout()
        }

        /* ----- layout ----- */
        fn fit_width(&mut self, ctx: &mut ui_ctx::LayoutCtx<N>) -> Layout {
            let mut tmp_ui: ui_ctx::Context<M> = ui_ctx::Context::new();
            let mut tmp = ui_ctx::LayoutCtx {
                globals: ctx.globals,
                ui: &mut tmp_ui,
                text: ctx.text,
            };
            self.inner.fit_width(&mut tmp)
        }
        fn grow_width(&mut self, ctx: &mut ui_ctx::LayoutCtx<N>, parent_width: i32) {
            let mut tmp_ui: ui_ctx::Context<M> = ui_ctx::Context::new();
            let mut tmp = ui_ctx::LayoutCtx {
                globals: ctx.globals,
                ui: &mut tmp_ui,
                text: ctx.text,
            };
            self.inner.grow_width(&mut tmp, parent_width)
        }
        fn fit_height(&mut self, ctx: &mut ui_ctx::LayoutCtx<N>) -> Layout {
            let mut tmp_ui: ui_ctx::Context<M> = ui_ctx::Context::new();
            let mut tmp = ui_ctx::LayoutCtx {
                globals: ctx.globals,
                ui: &mut tmp_ui,
                text: ctx.text,
            };
            self.inner.fit_height(&mut tmp)
        }
        fn grow_height(&mut self, ctx: &mut ui_ctx::LayoutCtx<N>, parent_height: i32) {
            let mut tmp_ui: ui_ctx::Context<M> = ui_ctx::Context::new();
            let mut tmp = ui_ctx::LayoutCtx {
                globals: ctx.globals,
                ui: &mut tmp_ui,
                text: ctx.text,
            };
            self.inner.grow_height(&mut tmp, parent_height)
        }
        fn place(&mut self, ctx: &mut ui_ctx::LayoutCtx<N>, position: Position<i32>) -> Size<i32> {
            let mut tmp_ui: ui_ctx::Context<M> = ui_ctx::Context::new();
            let mut tmp = ui_ctx::LayoutCtx {
                globals: ctx.globals,
                ui: &mut tmp_ui,
                text: ctx.text,
            };
            self.inner.place(&mut tmp, position)
        }

        /* ----- paint ----- */
        fn draw_self(&self, ctx: &mut ui_ctx::PaintCtx, instances: &mut Vec<Instance>) {
            self.inner.draw_self(ctx, instances)
        }
        #[doc(hidden)]
        fn for_each_child(&self, f: &mut dyn for<'a> FnMut(&'a dyn Widget<N>)) {
            let mut wrap = |w: &dyn Widget<M>| {
                let _ = w;
            };
            self.inner.for_each_child(&mut wrap);
            let _ = f;
        }
        #[doc(hidden)]
        fn __paint(
            &self,
            ctx: &mut ui_ctx::PaintCtx,
            instances: &mut Vec<Instance>,
            t: &ui::widget::internal::PaintToken,
            debug_on: bool,
        ) {
            self.inner.__paint(ctx, instances, t, debug_on)
        }

        fn handle(&mut self, ctx: &mut ui_ctx::EventCtx<N>) {
            let mut tmp = ui_ctx::Context::<M>::new();
            tmp.mouse_pos = ctx.ui.mouse_pos;
            tmp.mouse_down = ctx.ui.mouse_down;
            tmp.mouse_pressed = ctx.ui.mouse_pressed;
            tmp.mouse_released = ctx.ui.mouse_released;
            tmp.hot_item = ctx.ui.hot_item;
            tmp.active_item = ctx.ui.active_item;
            tmp.kbd_focus_item = ctx.ui.kbd_focus_item;

            self.inner.handle(&mut ui_ctx::EventCtx {
                globals: ctx.globals,
                ui: &mut tmp,
            });

            if tmp.take_redraw() {
                ctx.ui.request_redraw();
            }

            for m in tmp.take() {
                ctx.ui.emit((self.map)(m));
            }

            ctx.ui.mouse_pos = tmp.mouse_pos;
            ctx.ui.mouse_down = tmp.mouse_down;
            ctx.ui.mouse_pressed = tmp.mouse_pressed;
            ctx.ui.mouse_released = tmp.mouse_released;
            ctx.ui.hot_item = tmp.hot_item;
            ctx.ui.active_item = tmp.active_item;
            ctx.ui.kbd_focus_item = tmp.kbd_focus_item;
        }
    }

    Map::new(elem, f).einto()
}

pub fn erase_element<M: Send + 'static>(elem: Element<M>) -> Element<ErasedMsg> {
    map_element(elem, ErasedMsg::new)
}
