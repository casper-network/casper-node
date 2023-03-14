#![warn(missing_docs)]

use core::{cell::RefCell, slice};
use std::rc::Rc;

#[derive(Debug)]
enum EntryOrigin {
    Index(usize),
    Detached,
}

impl From<usize> for EntryOrigin {
    fn from(v: usize) -> Self {
        EntryOrigin::Index(v)
    }
}

/// Reference counting, link-handling object.
#[derive(Debug)]
pub struct Entry<T> {
    val: T,
    index: EntryOrigin,
}

impl<T> Entry<T> {
    /// New entity.
    pub fn new(val: T, index: usize) -> Entry<T> {
        Entry {
            val,
            index: EntryOrigin::Index(index),
        }
    }

    /// New detached entry.
    pub fn new_detached(val: T) -> Entry<T> {
        Entry {
            val,
            index: EntryOrigin::Detached,
        }
    }

    /// Index of the element within the reference list.
    pub fn order(&self) -> Option<usize> {
        match self.index {
            EntryOrigin::Detached => None,
            EntryOrigin::Index(idx) => Some(idx),
        }
    }
}

impl<T> core::ops::Deref for Entry<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.val
    }
}

impl<T> core::ops::DerefMut for Entry<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.val
    }
}

/// Reference to the entry in the rerence list.
#[derive(Debug)]
pub struct EntryRef<T>(Rc<RefCell<Entry<T>>>);

impl<T> Clone for EntryRef<T> {
    fn clone(&self) -> Self {
        EntryRef(self.0.clone())
    }
}

impl<T> From<Entry<T>> for EntryRef<T> {
    fn from(v: Entry<T>) -> Self {
        EntryRef(Rc::new(RefCell::new(v)))
    }
}

impl<T> EntryRef<T> {
    /// Read the reference data.
    pub fn read(&self) -> core::cell::Ref<Entry<T>> {
        self.0.borrow()
    }

    /// Try to modify internal content of the referenced object.
    ///
    /// May panic if it is already borrowed.
    pub fn write(&self) -> core::cell::RefMut<Entry<T>> {
        self.0.borrow_mut()
    }

    /// Index of the element within the reference list.
    pub fn order(&self) -> Option<usize> {
        self.0.borrow().order()
    }

    /// Number of active links to this entity.
    pub fn link_count(&self) -> usize {
        Rc::strong_count(&self.0) - 1
    }
}

/// List that tracks references and indices.
#[derive(Debug)]
pub struct RefList<T> {
    items: Vec<EntryRef<T>>,
}

impl<T> Default for RefList<T> {
    fn default() -> Self {
        RefList {
            items: Default::default(),
        }
    }
}

impl<T> RefList<T> {
    /// New empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Push new element in the list.
    ///
    /// Returns refernce tracking entry.
    pub fn push(&mut self, t: T) -> EntryRef<T> {
        let idx = self.items.len();
        let val: EntryRef<_> = Entry::new(t, idx).into();
        self.items.push(val.clone());
        val
    }

    /// Start deleting.
    ///
    /// Start deleting some entries in the list. Returns transaction
    /// that can be populated with number of removed entries.
    /// When transaction is finailized, all entries are deleted and
    /// internal indices of other entries are updated.
    pub fn begin_delete(&mut self) -> DeleteTransaction<T> {
        DeleteTransaction {
            list: self,
            deleted: Vec::new(),
        }
    }

    /// Start inserting.
    ///
    /// Start inserting some entries in the list at he designated position.
    /// Returns transaction that can be populated with some entries.
    /// When transaction is finailized, all entries are inserted and
    /// internal indices of other entries might be updated.
    pub fn begin_insert(&mut self, at: usize) -> InsertTransaction<T> {
        InsertTransaction {
            at,
            list: self,
            items: Vec::new(),
        }
    }

    /// Start inserting after the condition first matches (or at the end).
    ///
    /// Start inserting some entries in the list at he designated position.
    /// Returns transaction that can be populated with some entries.
    /// When transaction is finailized, all entries are inserted and
    /// internal indices of other entries might be updated.
    pub fn begin_insert_after<F>(&mut self, mut f: F) -> InsertTransaction<T>
    where
        F: FnMut(&T) -> bool,
    {
        let pos = self
            .items
            .iter()
            .position(|rf| f(&**rf.read()))
            .map(|x| x + 1)
            .unwrap_or(self.items.len());

        self.begin_insert(pos)
    }

    /// Start inserting after the condition first no longer true (or at the end).
    ///
    /// Start inserting some entries in the list at he designated position.
    /// Returns transaction that can be populated with some entries.
    /// When transaction is finailized, all entries are inserted and
    /// internal indices of other entries might be updated.
    pub fn begin_insert_not_until<F>(&mut self, mut f: F) -> InsertTransaction<T>
    where
        F: FnMut(&T) -> bool,
    {
        let pos = self.items.iter().take_while(|rf| f(&**rf.read())).count();
        self.begin_insert(pos)
    }

    /// Get entry with index (checked).
    ///
    /// Can return None when index out of bounts.
    pub fn get(&self, idx: usize) -> Option<EntryRef<T>> {
        self.items.get(idx).cloned()
    }

    fn done_delete(&mut self, indices: &[usize]) {
        for entry in self.items.iter_mut() {
            let mut entry = entry.write();
            let total_less = indices
                .iter()
                .take_while(|x| {
                    **x < entry
                        .order()
                        .expect("Items in the list always have order; qed")
                })
                .count();
            match &mut entry.index {
                EntryOrigin::Detached => unreachable!("Items in the list always have order!"),
                EntryOrigin::Index(idx) => {
                    *idx -= total_less;
                }
            };
        }

        for (total_removed, idx) in indices.iter().enumerate() {
            let detached = self.items.remove(*idx - total_removed);
            detached.write().index = EntryOrigin::Detached;
        }
    }

    fn done_insert(&mut self, index: usize, mut items: Vec<EntryRef<T>>) {
        let mut offset = 0;
        for item in items.drain(..) {
            item.write().index = EntryOrigin::Index(index + offset);
            self.items.insert(index + offset, item);
            offset += 1;
        }

        for idx in (index + offset)..self.items.len() {
            self.get_ref(idx).write().index = EntryOrigin::Index(idx);
        }
    }

    /// Delete several items.
    pub fn delete(&mut self, indices: &[usize]) {
        self.done_delete(indices)
    }

    /// Delete one item.
    pub fn delete_one(&mut self, index: usize) {
        self.done_delete(&[index])
    }

    /// Initialize from slice.
    ///
    /// Slice members are cloned.
    pub fn from_slice(list: &[T]) -> Self
    where
        T: Clone,
    {
        let mut res = Self::new();

        for t in list {
            res.push(t.clone());
        }

        res
    }

    /// Length of the list.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true iff len == 0.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clone entry (reference counting object to item) by index.
    ///
    /// Will panic if index out of bounds.
    pub fn clone_ref(&self, idx: usize) -> EntryRef<T> {
        self.items[idx].clone()
    }

    /// Get reference to entry by index.
    ///
    /// Will panic if index out of bounds.
    pub fn get_ref(&self, idx: usize) -> &EntryRef<T> {
        &self.items[idx]
    }

    /// Iterate through entries.
    pub fn iter(&self) -> slice::Iter<EntryRef<T>> {
        self.items.iter()
    }
}

/// Delete transaction.
#[must_use]
pub struct DeleteTransaction<'a, T> {
    list: &'a mut RefList<T>,
    deleted: Vec<usize>,
}

impl<'a, T> DeleteTransaction<'a, T> {
    /// Add new element to the delete list.
    pub fn push(self, idx: usize) -> Self {
        let mut tx = self;
        tx.deleted.push(idx);
        tx
    }

    /// Commit transaction.
    pub fn done(self) {
        let indices = self.deleted;
        let list = self.list;
        list.done_delete(&indices[..]);
    }
}

/// Insert transaction
#[must_use]
pub struct InsertTransaction<'a, T> {
    at: usize,
    list: &'a mut RefList<T>,
    items: Vec<EntryRef<T>>,
}

impl<'a, T> InsertTransaction<'a, T> {
    /// Add new element to the delete list.
    pub fn push(&mut self, val: T) -> EntryRef<T> {
        let val: EntryRef<_> = Entry::new_detached(val).into();
        self.items.push(val.clone());
        val
    }

    /// Commit transaction.
    pub fn done(self) {
        let items = self.items;
        let list = self.list;
        let at = self.at;
        list.done_insert(at, items);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order() {
        let mut list = RefList::<u32>::new();
        let item00 = list.push(0);
        let item10 = list.push(10);
        let item20 = list.push(20);
        let item30 = list.push(30);

        assert_eq!(item00.order(), Some(0));
        assert_eq!(item10.order(), Some(1));
        assert_eq!(item20.order(), Some(2));
        assert_eq!(item30.order(), Some(3));

        assert_eq!(**item00.read(), 0);
        assert_eq!(**item10.read(), 10);
        assert_eq!(**item20.read(), 20);
        assert_eq!(**item30.read(), 30);
    }

    #[test]
    fn delete() {
        let mut list = RefList::<u32>::new();
        let item00 = list.push(0);
        let item10 = list.push(10);
        let item20 = list.push(20);
        let item30 = list.push(30);

        list.begin_delete().push(2).done();

        assert_eq!(item00.order(), Some(0));
        assert_eq!(item10.order(), Some(1));
        assert_eq!(item30.order(), Some(2));

        // but this was detached
        assert_eq!(item20.order(), None);
    }

    #[test]
    fn complex_delete() {
        let mut list = RefList::<u32>::new();
        let item00 = list.push(0);
        let item10 = list.push(10);
        let item20 = list.push(20);
        let item30 = list.push(30);
        let item40 = list.push(40);
        let item50 = list.push(50);
        let item60 = list.push(60);
        let item70 = list.push(70);
        let item80 = list.push(80);
        let item90 = list.push(90);

        list.begin_delete().push(1).push(2).push(4).push(6).done();

        assert_eq!(item00.order(), Some(0));
        assert_eq!(item10.order(), None);
        assert_eq!(item20.order(), None);
        assert_eq!(item30.order(), Some(1));
        assert_eq!(item40.order(), None);
        assert_eq!(item50.order(), Some(2));
        assert_eq!(item60.order(), None);
        assert_eq!(item70.order(), Some(3));
        assert_eq!(item80.order(), Some(4));
        assert_eq!(item90.order(), Some(5));
    }

    #[test]
    fn insert() {
        let mut list = RefList::<u32>::new();
        let item00 = list.push(0);
        let item10 = list.push(10);
        let item20 = list.push(20);
        let item30 = list.push(30);

        let mut insert_tx = list.begin_insert(3);
        let item23 = insert_tx.push(23);
        let item27 = insert_tx.push(27);
        insert_tx.done();

        assert_eq!(item00.order(), Some(0));
        assert_eq!(item10.order(), Some(1));
        assert_eq!(item20.order(), Some(2));
        assert_eq!(item23.order(), Some(3));
        assert_eq!(item27.order(), Some(4));
        assert_eq!(item30.order(), Some(5));
    }

    #[test]
    fn insert_end() {
        let mut list = RefList::<u32>::new();

        let mut insert_tx = list.begin_insert(0);
        let item0 = insert_tx.push(0);
        insert_tx.done();

        assert_eq!(item0.order(), Some(0));
    }

    #[test]
    fn insert_end_more() {
        let mut list = RefList::<u32>::new();
        let item0 = list.push(0);

        let mut insert_tx = list.begin_insert(1);
        let item1 = insert_tx.push(1);
        insert_tx.done();

        assert_eq!(item0.order(), Some(0));
        assert_eq!(item1.order(), Some(1));
    }

    #[test]
    fn insert_after() {
        let mut list = RefList::<u32>::new();
        let item00 = list.push(0);
        let item10 = list.push(10);
        let item20 = list.push(20);
        let item30 = list.push(30);

        let mut insert_tx = list.begin_insert_after(|i| *i == 20);

        let item23 = insert_tx.push(23);
        let item27 = insert_tx.push(27);
        insert_tx.done();

        assert_eq!(item00.order(), Some(0));
        assert_eq!(item10.order(), Some(1));
        assert_eq!(item20.order(), Some(2));
        assert_eq!(item23.order(), Some(3));
        assert_eq!(item27.order(), Some(4));
        assert_eq!(item30.order(), Some(5));
    }

    #[test]
    fn insert_not_until() {
        let mut list = RefList::<u32>::new();
        let item10 = list.push(10);
        let item20 = list.push(20);
        let item30 = list.push(30);

        let mut insert_tx = list.begin_insert_not_until(|i| *i <= 20);

        let item23 = insert_tx.push(23);
        let item27 = insert_tx.push(27);
        insert_tx.done();

        assert_eq!(item10.order(), Some(0));
        assert_eq!(item20.order(), Some(1));
        assert_eq!(item23.order(), Some(2));
        assert_eq!(item27.order(), Some(3));
        assert_eq!(item30.order(), Some(4));
    }

    #[test]
    fn insert_after_none() {
        let mut list = RefList::<u32>::new();
        let item10 = list.push(10);
        let item20 = list.push(20);
        let item30 = list.push(30);

        let mut insert_tx = list.begin_insert_after(|i| *i == 50);

        let item55 = insert_tx.push(23);
        let item59 = insert_tx.push(27);
        insert_tx.done();

        assert_eq!(item10.order(), Some(0));
        assert_eq!(item20.order(), Some(1));
        assert_eq!(item30.order(), Some(2));
        assert_eq!(item55.order(), Some(3));
        assert_eq!(item59.order(), Some(4));
    }

    #[test]
    fn insert_not_until_none() {
        let mut list = RefList::<u32>::new();
        let item10 = list.push(10);
        let item20 = list.push(20);
        let item30 = list.push(30);

        let mut insert_tx = list.begin_insert_not_until(|i| *i < 50);

        let item55 = insert_tx.push(23);
        let item59 = insert_tx.push(27);
        insert_tx.done();

        assert_eq!(item10.order(), Some(0));
        assert_eq!(item20.order(), Some(1));
        assert_eq!(item30.order(), Some(2));
        assert_eq!(item55.order(), Some(3));
        assert_eq!(item59.order(), Some(4));
    }

    #[test]
    fn insert_after_empty() {
        let mut list = RefList::<u32>::new();

        let mut insert_tx = list.begin_insert_after(|x| *x == 100);
        let item0 = insert_tx.push(0);
        insert_tx.done();

        assert_eq!(item0.order(), Some(0));
    }

    #[test]
    fn insert_more() {
        let mut list = RefList::<u32>::new();
        let item10 = list.push(10);
        let item20 = list.push(20);
        let item30 = list.push(30);
        let item40 = list.push(10);
        let item50 = list.push(20);
        let item60 = list.push(30);

        let mut insert_tx = list.begin_insert(3);
        let item35 = insert_tx.push(23);
        let item37 = insert_tx.push(27);
        insert_tx.done();

        assert_eq!(item10.order(), Some(0));
        assert_eq!(item20.order(), Some(1));
        assert_eq!(item30.order(), Some(2));
        assert_eq!(item35.order(), Some(3));
        assert_eq!(item37.order(), Some(4));
        assert_eq!(item40.order(), Some(5));
        assert_eq!(item50.order(), Some(6));
        assert_eq!(item60.order(), Some(7));
    }
}
