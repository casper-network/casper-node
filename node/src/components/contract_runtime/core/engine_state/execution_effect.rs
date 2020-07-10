use types::Key;

use super::op::Op;
use crate::components::contract_runtime::shared::{
    additive_map::AdditiveMap, transform::Transform,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecutionEffect {
    pub ops: AdditiveMap<Key, Op>,
    pub transforms: AdditiveMap<Key, Transform>,
}

impl ExecutionEffect {
    pub fn new(ops: AdditiveMap<Key, Op>, transforms: AdditiveMap<Key, Transform>) -> Self {
        ExecutionEffect { ops, transforms }
    }
}
