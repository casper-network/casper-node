//! Block validator
//!
//! The block validator checks whether all the deploys included in the proto block exist, either
//! locally or in the network

use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    mem,
};

use derive_more::From;
use rand::Rng;

use crate::{
    components::Component,
    effect::{requests::BlockValidatorRequest, EffectBuilder, EffectExt, Effects, Responder},
    types::{DeployHash, ProtoBlock},
};

/// Block executor component event.
#[derive(Debug, From)]
pub enum Event<I> {
    /// A request made of the Block executor component.
    #[from]
    Request(BlockValidatorRequest<I>),
    DeployFetcherResponse {
        deploy: DeployHash,
        validation_successful: bool,
    },
}

impl<I: Debug> Display for Event<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(req) => write!(f, "{}", req),
            Event::DeployFetcherResponse {
                deploy,
                validation_successful,
            } => write!(f, "deploy {} is valid: {}", deploy, validation_successful),
        }
    }
}

#[derive(Clone, Debug)]
struct MissingDeploys(HashSet<DeployHash>);

impl MissingDeploys {
    /// Removes the deploy from missing deploys, returns true if set is empty afterwards
    fn got_deploy(&mut self, hash: &DeployHash) -> bool {
        let _ = self.0.remove(hash);
        self.0.is_empty()
    }

    /// Returns true if `hash` is still missing
    fn has_deploy(&self, hash: &DeployHash) -> bool {
        self.0.contains(hash)
    }
}

#[derive(Debug)]
struct BlockData {
    missing_deploys: MissingDeploys,
    responder: Responder<bool>,
}

/// Block validator.
#[derive(Debug, Default)]
pub(crate) struct BlockValidator<I> {
    block_states: HashMap<ProtoBlock, BlockData>,
    _marker: std::marker::PhantomData<I>,
}

impl<I> BlockValidator<I> {
    pub(crate) fn new() -> Self {
        BlockValidator {
            block_states: Default::default(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<I, REv> Component<REv> for BlockValidator<I>
where
    REv: From<Event<I>> + From<BlockValidatorRequest<I>> + Send,
{
    type Event = Event<I>;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(BlockValidatorRequest {
                proto_block,
                sender: _sender,
                responder,
            }) => {
                let missing_deploys = MissingDeploys(proto_block.deploys.iter().cloned().collect());
                self.block_states.insert(
                    proto_block,
                    BlockData {
                        missing_deploys,
                        responder,
                    },
                );
                todo!("ask the deploy fetcher for all the deploy hashes in the block")
            }
            Event::DeployFetcherResponse {
                deploy,
                validation_successful,
            } => {
                let block_states = mem::take(&mut self.block_states);
                if validation_successful {
                    for (proto_block, mut block_data) in block_states {
                        if block_data.missing_deploys.got_deploy(&deploy) {
                            block_data.responder.respond(true).ignore::<Event<I>>();
                        } else {
                            self.block_states.insert(proto_block, block_data);
                        }
                    }
                } else {
                    for (proto_block, block_data) in block_states {
                        if block_data.missing_deploys.has_deploy(&deploy) {
                            block_data.responder.respond(false).ignore::<Event<I>>();
                        } else {
                            self.block_states.insert(proto_block, block_data);
                        }
                    }
                }
                Default::default()
            }
        }
    }
}
