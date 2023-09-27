//! Runtime stacks.

use casper_types::{account::AccountHash, system::CallStackElement, PublicKey};

/// A runtime stack frame.
///
/// Currently it aliases to a [`CallStackElement`].
///
/// NOTE: Once we need to add more data to a stack frame we should make this a newtype, rather than
/// change [`CallStackElement`].
pub type RuntimeStackFrame = CallStackElement;

/// The runtime stack.
#[derive(Clone)]
pub struct RuntimeStack {
    frames: Vec<RuntimeStackFrame>,
    max_height: usize,
}

/// Error returned on an attempt to pop off an empty stack.
#[cfg(test)]
#[derive(Debug)]
struct RuntimeStackUnderflow;

/// Error returned on an attempt to push to a stack already at the maximum height.
#[derive(Debug)]
pub struct RuntimeStackOverflow;

impl RuntimeStack {
    /// Creates an empty stack.
    pub fn new(max_height: usize) -> Self {
        Self {
            frames: Vec::with_capacity(max_height),
            max_height,
        }
    }

    /// Creates a stack with one entry.
    pub fn new_with_frame(max_height: usize, frame: RuntimeStackFrame) -> Self {
        let mut frames = Vec::with_capacity(max_height);
        frames.push(frame);
        Self { frames, max_height }
    }

    /// Creates a new call instance that starts with a system account.
    pub(crate) fn new_system_call_stack(max_height: usize) -> Self {
        RuntimeStack::new_with_frame(
            max_height,
            CallStackElement::session(PublicKey::System.to_account_hash()),
        )
    }

    /// Is the stack empty?
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The height of the stack.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// The current stack frame.
    pub fn current_frame(&self) -> Option<&RuntimeStackFrame> {
        self.frames.last()
    }

    /// The previous stack frame.
    pub fn previous_frame(&self) -> Option<&RuntimeStackFrame> {
        self.frames.iter().nth_back(1)
    }

    /// The first stack frame.
    pub fn first_frame(&self) -> Option<&RuntimeStackFrame> {
        self.frames.first()
    }

    /// Pops the current frame from the stack.
    #[cfg(test)]
    fn pop(&mut self) -> Result<(), RuntimeStackUnderflow> {
        self.frames.pop().ok_or(RuntimeStackUnderflow)?;
        Ok(())
    }

    /// Pushes a frame onto the stack.
    pub fn push(&mut self, frame: RuntimeStackFrame) -> Result<(), RuntimeStackOverflow> {
        if self.len() < self.max_height {
            self.frames.push(frame);
            Ok(())
        } else {
            Err(RuntimeStackOverflow)
        }
    }

    // It is here for backwards compatibility only.
    /// A view of the stack in the previous stack format.
    pub fn call_stack_elements(&self) -> &Vec<CallStackElement> {
        &self.frames
    }

    /// Returns a stack with exactly one session element with the associated account hash.
    pub fn from_account_hash(account_hash: AccountHash, max_height: usize) -> Self {
        RuntimeStack {
            frames: vec![CallStackElement::session(account_hash)],
            max_height,
        }
    }
}

#[cfg(test)]
mod test {
    use core::convert::TryInto;

    use casper_types::account::{AccountHash, ACCOUNT_HASH_LENGTH};

    use super::*;

    fn nth_frame(n: usize) -> CallStackElement {
        let mut bytes = [0_u8; ACCOUNT_HASH_LENGTH];
        let n: u32 = n.try_into().unwrap();
        bytes[0..4].copy_from_slice(&n.to_le_bytes());
        CallStackElement::session(AccountHash::new(bytes))
    }

    #[allow(clippy::redundant_clone)]
    #[test]
    fn stack_should_respect_max_height_after_clone() {
        const MAX_HEIGHT: usize = 3;
        let mut stack = RuntimeStack::new(MAX_HEIGHT);
        stack.push(nth_frame(1)).unwrap();

        let mut stack2 = stack.clone();
        stack2.push(nth_frame(2)).unwrap();
        stack2.push(nth_frame(3)).unwrap();
        stack2.push(nth_frame(4)).unwrap_err();
        assert_eq!(stack2.len(), MAX_HEIGHT);
    }

    #[test]
    fn stack_should_work_as_expected() {
        const MAX_HEIGHT: usize = 6;

        let mut stack = RuntimeStack::new(MAX_HEIGHT);
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);
        assert_eq!(stack.current_frame(), None);
        assert_eq!(stack.previous_frame(), None);
        assert_eq!(stack.first_frame(), None);

        stack.push(nth_frame(0)).unwrap();
        assert!(!stack.is_empty());
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.current_frame(), Some(&nth_frame(0)));
        assert_eq!(stack.previous_frame(), None);
        assert_eq!(stack.first_frame(), Some(&nth_frame(0)));

        let mut n: usize = 1;
        while stack.push(nth_frame(n)).is_ok() {
            n += 1;
            assert!(!stack.is_empty());
            assert_eq!(stack.len(), n);
            assert_eq!(stack.current_frame(), Some(&nth_frame(n - 1)));
            assert_eq!(stack.previous_frame(), Some(&nth_frame(n - 2)));
            assert_eq!(stack.first_frame(), Some(&nth_frame(0)));
        }
        assert!(!stack.is_empty());
        assert_eq!(stack.len(), MAX_HEIGHT);
        assert_eq!(stack.current_frame(), Some(&nth_frame(MAX_HEIGHT - 1)));
        assert_eq!(stack.previous_frame(), Some(&nth_frame(MAX_HEIGHT - 2)));
        assert_eq!(stack.first_frame(), Some(&nth_frame(0)));

        while stack.len() >= 3 {
            stack.pop().unwrap();
            n = n.checked_sub(1).unwrap();
            assert!(!stack.is_empty());
            assert_eq!(stack.len(), n);
            assert_eq!(stack.current_frame(), Some(&nth_frame(n - 1)));
            assert_eq!(stack.previous_frame(), Some(&nth_frame(n - 2)));
            assert_eq!(stack.first_frame(), Some(&nth_frame(0)));
        }

        stack.pop().unwrap();
        assert!(!stack.is_empty());
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.current_frame(), Some(&nth_frame(0)));
        assert_eq!(stack.previous_frame(), None);
        assert_eq!(stack.first_frame(), Some(&nth_frame(0)));

        stack.pop().unwrap();
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);
        assert_eq!(stack.current_frame(), None);
        assert_eq!(stack.previous_frame(), None);
        assert_eq!(stack.first_frame(), None);

        assert!(stack.pop().is_err());
    }
}
