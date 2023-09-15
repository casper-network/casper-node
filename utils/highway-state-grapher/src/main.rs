use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fmt::{self, Debug},
    fs::File,
    io::Read,
};

use casper_hashing::Digest;
use casper_node::consensus::{
    highway_core::{Panorama, State},
    utils::{ValidatorIndex, ValidatorMap},
    ClContext,
};
use casper_types::{EraId, PublicKey, Timestamp, U512};

use clap::Parser;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    filename: String,
}

/// Debug dump of era used for serialization.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EraDump {
    /// The era that is being dumped.
    pub id: EraId,

    /// The scheduled starting time of this era.
    pub start_time: Timestamp,
    /// The height of this era's first block.
    pub start_height: u64,

    // omitted: pending blocks
    /// Validators that have been faulty in any of the recent BONDED_ERAS switch blocks. This
    /// includes `new_faulty`.
    pub faulty: HashSet<PublicKey>,
    /// Validators that are excluded from proposing new blocks.
    pub cannot_propose: HashSet<PublicKey>,
    /// Accusations collected in this era so far.
    pub accusations: HashSet<PublicKey>,
    /// The validator weights.
    pub validators: BTreeMap<PublicKey, U512>,

    /// The state of the highway instance associated with the era.
    pub highway_state: State<ClContext>,
}

struct Units {
    set: HashSet<Digest>,
    order: Vec<Digest>,
}

impl Units {
    fn do_collect_ancestor_units(
        &mut self,
        state: &State<ClContext>,
        panorama: &Panorama<ClContext>,
    ) {
        let hashes_to_add: Vec<_> = panorama.iter_correct_hashes().collect();
        let mut hashes_to_proceed_with = vec![];
        for hash in hashes_to_add {
            if self.set.insert(*hash) {
                self.order.push(*hash);
                hashes_to_proceed_with.push(*hash);
            }
        }
        for hash in hashes_to_proceed_with {
            let unit = state.unit(&hash);
            self.do_collect_ancestor_units(state, &unit.panorama);
        }
    }

    fn reorder(&mut self, state: &State<ClContext>) {
        let mut new_order_set = HashSet::new();
        let mut new_order = vec![];
        let mut queue: VecDeque<_> = std::mem::replace(&mut self.order, vec![])
            .into_iter()
            .rev()
            .collect();
        loop {
            if queue.is_empty() {
                break;
            }
            let unit = queue.pop_front().unwrap();
            if state
                .unit(&unit)
                .panorama
                .iter_correct_hashes()
                .all(|cited| new_order_set.contains(cited))
            {
                new_order_set.insert(unit);
                new_order.push(unit)
            } else {
                queue.push_back(unit);
            }
        }
        self.order = new_order;
    }

    fn collect_ancestor_units(&mut self, state: &State<ClContext>) {
        self.do_collect_ancestor_units(state, state.panorama());
        self.reorder(state);
    }
}

#[derive(Clone, Copy)]
struct UnitId(ValidatorIndex, usize);

impl Debug for UnitId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "V{}_{}", self.0 .0, self.1)
    }
}

#[derive(Clone, Copy)]
struct BlockId(u64, u8);

impl Debug for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "B{}", self.0)?;
        for _ in 0..self.1 {
            write!(f, "'")?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct GraphUnit {
    id: UnitId,
    creator: ValidatorIndex,
    vote: BlockId,
    cited_units: Vec<UnitId>,
    height: usize,
    graph_height: usize,
    round_exp: u8,
}

impl Debug for GraphUnit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("GraphUnit")
            .field("id", &self.id)
            .field("creator", &format!("V{}", self.creator.0))
            .field("height", &self.height)
            .field("graph_height", &self.graph_height)
            .field("vote", &self.vote)
            .field("round_exp", &self.round_exp)
            .field("cited_units", &self.cited_units)
            .finish()
    }
}

#[derive(Clone, Debug)]
struct BlockMapper {
    hash_to_id: HashMap<Digest, BlockId>,
    last_id_by_height: HashMap<u64, u8>,
}

impl BlockMapper {
    fn new() -> Self {
        Self {
            hash_to_id: HashMap::new(),
            last_id_by_height: HashMap::new(),
        }
    }

    fn insert(&mut self, hash: Digest, id: BlockId) {
        self.hash_to_id.insert(hash, id);
        let entry = self.last_id_by_height.entry(id.0).or_insert(id.1);
        *entry = (*entry).max(id.1);
    }

    fn next_id_for_height(&self, height: u64) -> BlockId {
        BlockId(height, *self.last_id_by_height.get(&height).unwrap_or(&0))
    }

    fn get(&self, hash: &Digest) -> Option<BlockId> {
        self.hash_to_id.get(hash).copied()
    }
}

#[derive(Clone, Debug)]
struct Graph {
    units: ValidatorMap<Vec<GraphUnit>>,
    blocks: BlockMapper,
}

impl Graph {
    fn new(state: &State<ClContext>) -> Self {
        let mut units: BTreeMap<ValidatorIndex, Vec<GraphUnit>> = state
            .weights()
            .iter()
            .enumerate()
            .map(|(idx, _)| (ValidatorIndex::from(idx as u32), vec![]))
            .collect();
        let mut unit_ids_by_hash: HashMap<Digest, UnitId> = HashMap::new();
        let mut blocks = BlockMapper::new();

        let mut units_set = Units {
            set: HashSet::new(),
            order: vec![],
        };

        units_set.collect_ancestor_units(state);

        println!("num units: {}", units_set.order.len());

        for unit_hash in &units_set.order {
            let unit = state.unit(unit_hash);
            let block = state.block(&unit.block);
            let block_id = if let Some(b_id) = blocks.get(&unit.block) {
                b_id
            } else {
                let b_id = blocks.next_id_for_height(block.height);
                blocks.insert(unit.block, b_id);
                b_id
            };
            let cited_units: Vec<UnitId> = unit
                .panorama
                .iter_correct_hashes()
                .map(|hash| *unit_ids_by_hash.get(hash).unwrap())
                .collect();
            let graph_height = cited_units
                .iter()
                .map(|unit_id| &units.get(&unit_id.0).unwrap()[unit_id.1])
                .map(|g_unit| g_unit.graph_height)
                .max()
                .map(|max_height| max_height + 1)
                .unwrap_or(0);
            let unit_id = UnitId(unit.creator, units.get(&unit.creator).unwrap().len());
            let graph_unit = GraphUnit {
                id: unit_id,
                creator: unit.creator,
                vote: block_id,
                cited_units,
                height: unit.seq_number as usize,
                graph_height,
                round_exp: (unit.round_len().millis() / state.params().min_round_length().millis())
                    .trailing_zeros() as u8,
            };
            unit_ids_by_hash.insert(*unit_hash, unit_id);
            units.get_mut(&unit.creator).unwrap().push(graph_unit);
        }

        Self {
            units: units.into_values().collect(),
            blocks,
        }
    }
}

fn main() {
    let args = Args::parse();

    let mut data = vec![];
    let mut file = File::open(&args.filename).unwrap();

    if args.filename.ends_with(".gz") {
        let mut gz = GzDecoder::new(file);
        gz.read_to_end(&mut data).unwrap();
    } else {
        file.read_to_end(&mut data).unwrap();
    }

    let dump: EraDump = bincode::deserialize(&data).unwrap();

    println!("{}", dump.id);

    let weight_percentages: ValidatorMap<f32> = dump
        .highway_state
        .weights()
        .iter()
        .map(|weight| weight.0 as f32 / dump.highway_state.total_weight().0 as f32 * 100.0)
        .collect();

    let graph = Graph::new(&dump.highway_state);

    println!("{:#?}", weight_percentages);
    println!("{:#?}", graph);
}
