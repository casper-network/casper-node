//! Operations occuring during execution.
use std::{
    default::Default,
    fmt::{self, Display, Formatter},
    ops::{Add, AddAssign},
};

/// Representation of a single operation during execution.
#[derive(PartialEq, Eq, Debug, Clone, Copy, Default)]
pub enum Op {
    /// Read value from a `Key`.
    Read,
    /// Write value under a `Key`.
    Write,
    /// Add a value into a `Key`.
    Add,
    /// No operation.
    #[default]
    NoOp,
}

impl Add for Op {
    type Output = Op;

    fn add(self, other: Op) -> Op {
        match (self, other) {
            (a, Op::NoOp) => a,
            (Op::NoOp, b) => b,
            (Op::Read, Op::Read) => Op::Read,
            (Op::Add, Op::Add) => Op::Add,
            _ => Op::Write,
        }
    }
}

impl AddAssign for Op {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl Display for Op {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<&Op> for casper_types::OpKind {
    fn from(op: &Op) -> Self {
        match op {
            Op::Read => casper_types::OpKind::Read,
            Op::Write => casper_types::OpKind::Write,
            Op::Add => casper_types::OpKind::Add,
            Op::NoOp => casper_types::OpKind::NoOp,
        }
    }
}
