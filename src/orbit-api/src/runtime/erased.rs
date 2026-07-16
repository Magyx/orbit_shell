use std::any::Any;

use crate::ErasedMsg;

pub trait DynMsg: Send + 'static {
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn DynMsg>;
}

impl<T> DynMsg for T
where
    T: Any + Send + Clone + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_box(&self) -> Box<dyn DynMsg> {
        Box::new(self.clone())
    }
}

impl ErasedMsg {
    pub fn new<M: 'static + Clone + Send>(m: M) -> Self {
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
