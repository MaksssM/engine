use bevy_ecs::resource::Resource;
use std::collections::VecDeque;

#[derive(Debug, Resource)]
pub struct Events<T> {
    queue: VecDeque<T>,
}

impl<T> Default for Events<T> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }
}

impl<T> Events<T> {
    pub fn send(&mut self, event: T) {
        self.queue.push_back(event);
    }
    pub fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.queue.drain(..)
    }
    pub fn len(&self) -> usize {
        self.queue.len()
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Events;
    #[test]
    fn events_are_fifo_and_drained() {
        let mut events = Events::default();
        events.send(1);
        events.send(2);
        assert_eq!(events.drain().collect::<Vec<_>>(), vec![1, 2]);
        assert!(events.is_empty());
    }
}
