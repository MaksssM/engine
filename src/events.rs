use bevy_ecs::resource::Resource;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventCursor {
    sequence: u64,
}

impl Default for EventCursor {
    fn default() -> Self {
        Self { sequence: 0 }
    }
}

#[derive(Debug, Resource)]
pub struct Events<T> {
    queue: VecDeque<(u64, T)>,
    next_sequence: u64,
}

impl<T> Default for Events<T> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            next_sequence: 0,
        }
    }
}

impl EventCursor {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl<T> Events<T> {
    pub fn send(&mut self, event: T) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.queue.push_back((sequence, event));
    }
    pub fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.queue.drain(..).map(|(_, event)| event)
    }
    pub fn read<'a>(&'a self, cursor: &'a mut EventCursor) -> impl Iterator<Item = &'a T> + 'a {
        let current = cursor.sequence;
        let newest = self.next_sequence;
        cursor.sequence = newest;
        self.queue
            .iter()
            .filter(move |(sequence, _)| *sequence >= current)
            .map(|(_, event)| event)
    }
    /// Discard events that every registered reader has already passed.
    /// Callers retain cursors for as long as they need delivery guarantees.
    pub fn retain_from(&mut self, oldest_sequence: u64) {
        while self
            .queue
            .front()
            .is_some_and(|(sequence, _)| *sequence < oldest_sequence)
        {
            self.queue.pop_front();
        }
    }

    /// Advance a cursor and return its events while allowing the queue to be
    /// compacted once all readers have acknowledged the same boundary.
    pub fn read_and_retain<'a>(
        &'a mut self,
        cursor: &'a mut EventCursor,
        oldest_sequence: u64,
    ) -> impl Iterator<Item = &'a T> + 'a {
        let current = cursor.sequence;
        let newest = self.next_sequence;
        cursor.sequence = newest;
        self.retain_from(oldest_sequence);
        self.queue
            .iter()
            .filter(move |(sequence, _)| *sequence >= current)
            .map(|(_, event)| event)
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

    #[test]
    fn readers_receive_events_independently_and_once() {
        let mut events = Events::default();
        let mut first = super::EventCursor::default();
        let mut second = super::EventCursor::default();
        events.send(1);
        events.send(2);
        assert_eq!(
            events.read(&mut first).copied().collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(events.read(&mut first).count(), 0);
        assert_eq!(
            events.read(&mut second).copied().collect::<Vec<_>>(),
            vec![1, 2]
        );
        events.send(3);
        assert_eq!(
            events.read(&mut first).copied().collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(
            events.read(&mut second).copied().collect::<Vec<_>>(),
            vec![3]
        );
        events.retain_from(first.sequence());
        assert_eq!(events.len(), 0);
    }
}
