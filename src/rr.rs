use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

struct LockInner<T> {
    wait_list: Vec<u8>,
    item: Option<Box<T>>,
}

struct FairLock<T> {
    tickets: Vec<AtomicBool>,
    inner: Mutex<LockInner<T>>,
}

impl<T> FairLock<T> {
    pub fn new(num_tickets: u8, item: T) -> Self {
        let mut tickets = Vec::new();
        tickets.resize_with(num_tickets as usize, || AtomicBool::new(false));

        FairLock {
            tickets,
            inner: Mutex::new(LockInner {
                wait_list: Vec::new(),
                item: Some(Box::new(item)),
            }),
        }
    }
}

struct Ticket<T> {
    id: u8,
    lock: Arc<FairLock<T>>,
}

impl<T> Drop for Ticket<T> {
    fn drop(&mut self) {
        let prev = self.lock.tickets[self.id as usize].fetch_and(false, Ordering::SeqCst);
        debug_assert!(
            !prev,
            "dropped ticket that does not exist, this should never happen",
        );
    }
}

struct Guard<T> {
    id: u8,
    item: Option<Box<T>>,
    lock: Arc<FairLock<T>>,
}

impl<T> Drop for Guard<T> {
    fn drop(&mut self) {
        let mut inner = self.lock.inner.lock().expect("HANDLE POISON");
        debug_assert!(inner.item.is_none());

        inner.item = Some(self.item.take().expect("violation, item disappread"));
        let first = inner.wait_list.pop();

        debug_assert_eq!(first, Some(self.id));
    }
}

impl<T> Deref for Guard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.item.as_ref().expect("ITEM DISAPPREAD")
    }
}

impl<T> DerefMut for Guard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.item.as_mut().expect("ITEM DISAPPREAD")
    }
}

impl<T> FairLock<T> {
    fn get_ticket(self: Arc<Self>, id: u8) -> Option<Ticket<T>> {
        if !self.tickets[id as usize].fetch_xor(true, Ordering::SeqCst) {
            self.inner.lock().expect("HANDLE POISON").wait_list.push(id);
            Some(Ticket {
                id,
                lock: self.clone(),
            })
        } else {
            None
        }
    }
}

impl<T> Ticket<T> {
    fn try_acquire(self) -> Result<Guard<T>, Self> {
        let mut inner = self.lock.inner.lock().expect("TODO: Handle poison");

        if inner.wait_list[0] != self.id {
            drop(inner);
            return Err(self);
        }

        let item = inner.item.take().expect("item disappeared?");
        Ok(Guard {
            id: self.id,
            item: Some(item),
            lock: self.lock.clone(),
        })

        // Now dropping ticket.
    }
}

#[cfg(test)]
mod tests {
    struct Dummy;

    #[test]
    fn basic_test() {
        let fair_lock = Arc::new(FairLock::new());
    }
}
