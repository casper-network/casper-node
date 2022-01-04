//! Limit retrieval and set.
//!
//! Allows retrieval and setting of resource limits.
//!
//! This module wraps a fair number of libc types to make external use as pleasant as possible.

use std::{
    any,
    fmt::{self, Debug},
    io,
    marker::PhantomData,
    mem::MaybeUninit,
};

use fmt::Formatter;

/// A scalar limit.
pub type Limit = libc::rlim_t;

#[cfg(target_os = "linux")]
pub type LimitResourceId = libc::__rlimit_resource_t;

#[cfg(target_os = "macos")]
pub type LimitResourceId = libc::c_int;

/// A kind of limit that can be set/retrieved.
pub trait LimitKind {
    /// The `resource` id use for libc calls.
    const LIBC_RESOURCE: LimitResourceId;
}

/// Maximum number of open files (`ulimit -n`).
#[derive(Copy, Clone, Debug)]
pub struct OpenFiles;

impl LimitKind for OpenFiles {
    const LIBC_RESOURCE: LimitResourceId = libc::RLIMIT_NOFILE;
}

/// Infinite resource, i.e. no limit.
#[allow(dead_code)]
const INFINITE: Limit = libc::RLIM_INFINITY;

/// Wrapper around libc resource limit type.
#[derive(Copy, Clone)]
pub struct ResourceLimit<T> {
    limit: libc::rlimit,
    kind: PhantomData<T>,
}

impl<T> Debug for ResourceLimit<T>
where
    T: Copy,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let name = format!("ResourceLimit<{}>", any::type_name::<T>());
        f.debug_struct(&name)
            .field("cur", &self.current())
            .field("max", &self.max())
            .finish()
    }
}

impl<T> ResourceLimit<T> {
    /// Creates a new resource limit.
    #[inline]
    pub fn new(current: Limit, max: Limit) -> Self {
        ResourceLimit {
            limit: libc::rlimit {
                rlim_cur: current,
                rlim_max: max,
            },
            kind: PhantomData,
        }
    }

    /// Creates a new resource limit, setting hard and soft limit to the same value.
    #[inline]
    pub fn fixed(limit: Limit) -> Self {
        Self::new(limit, limit)
    }

    /// The current or "soft" limit.
    #[inline]
    pub fn current(self) -> Limit {
        self.limit.rlim_cur
    }

    /// The maximum allowed or "hard" limit .
    #[inline]
    pub fn max(self) -> Limit {
        self.limit.rlim_max
    }
}

impl<T> ResourceLimit<T>
where
    T: LimitKind,
{
    /// Retrieves the given resource limit from the operating system.
    #[inline]
    pub fn get() -> io::Result<Self> {
        let mut dest: MaybeUninit<libc::rlimit> = MaybeUninit::zeroed();

        let outcome = unsafe { libc::getrlimit(T::LIBC_RESOURCE, dest.as_mut_ptr()) };

        match outcome {
            -1 => Err(io::Error::last_os_error()),
            0 => Ok(ResourceLimit {
                limit: unsafe { dest.assume_init() },
                kind: PhantomData,
            }),
            // This should never happen, so we notify the user.
            _ => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("expected return value of -1 or 0, but got {}", outcome),
            )),
        }
    }

    /// Sets the specified limit via operation system.
    pub fn set(self) -> io::Result<()> {
        let outcome = unsafe { libc::setrlimit(T::LIBC_RESOURCE, &self.limit) };

        match outcome {
            -1 => Err(io::Error::last_os_error()),
            0 => Ok(()),
            // This should never happen, so we notify the user.
            _ => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("expected return value of -1 or 0, but got {}", outcome),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenFiles, ResourceLimit};

    #[test]
    fn get_and_reset_ulimit() {
        // Retrieve limit and set the exact same limit again.
        let limit = ResourceLimit::<OpenFiles>::get().expect("could not retrieve initial limit");
        println!("{:?}", limit);

        // Note: We could change it to something different, but we do not want to risk influencing
        // other tests, so this is safest.
        limit.set().expect("could not re-set limit");

        println!(
            "{:?}",
            ResourceLimit::<OpenFiles>::get().expect("could not retrieve limit a second time")
        );
    }
}
