//! ExtraContext is used for passing extra data to Vector's components when Vector is used as a library.
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::{Send, Sync},
    sync::Arc,
};

/// Structure containing any extra data.
/// The data is held in an [`Arc`] so is cheap to clone.
#[derive(Clone, Default)]
pub struct ExtraContext(Arc<HashMap<TypeId, Box<dyn Any + Send + Sync>>>);

impl ExtraContext {
    /// Create a new `ExtraContext` with the provided [`HashMap`].
    pub fn new(context: HashMap<TypeId, Box<dyn Any + Send + Sync>>) -> Self {
        Self(Arc::new(context))
    }

    /// Create a new `ExtraContext` that contains the single passed in value.
    pub fn single_value<T: Any + Send + Sync>(value: T) -> Self {
        let mut map = HashMap::new();
        map.insert(
            value.type_id(),
            Box::new(value) as Box<dyn Any + Send + Sync>,
        );
        Self(Arc::new(map))
    }

    #[cfg(test)]
    /// This is only available for tests due to panic implications of making an Arc
    /// mutable when there may be multiple references to it.
    fn insert<T: Any + Send + Sync>(&mut self, value: T) {
        Arc::get_mut(&mut self.0)
            .expect("only insert into extra context when there is a single reference to it")
            .insert(value.type_id(), Box::new(value));
    }

    /// Get an object from the context.
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.0
            .get(&TypeId::of::<T>())
            .and_then(|t| t.downcast_ref())
    }

    /// Get an object from the context, if it doesn't exist return the default.
    pub fn get_or_default<T: 'static>(&self) -> T
    where
        T: Clone + Default,
    {
        self.get().cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Eq, PartialEq, Debug)]
    struct Peas {
        beans: usize,
    }

    #[derive(Clone, Eq, PartialEq, Debug)]
    struct Potatoes(usize);

    #[test]
    fn get_fetches_item() {
        let peas = Peas { beans: 42 };
        let potatoes = Potatoes(8);

        let mut context = ExtraContext::default();
        context.insert(peas);
        context.insert(potatoes);

        assert_eq!(&Peas { beans: 42 }, context.get::<Peas>().unwrap());
        assert_eq!(&Potatoes(8), context.get::<Potatoes>().unwrap());
    }

    #[test]
    fn duplicate_types() {
        let potatoes = Potatoes(8);
        let potatoes99 = Potatoes(99);

        let mut context = ExtraContext::default();
        context.insert(potatoes);
        context.insert(potatoes99);

        assert_eq!(&Potatoes(99), context.get::<Potatoes>().unwrap());
    }
}
