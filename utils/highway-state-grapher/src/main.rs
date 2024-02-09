mod renderer;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fmt::{self, Debug},
    fs::File,
    io::Read,
    ops::RangeBounds,
};

use casper_hashing::Digest;
use casper_node::consensus::{
    highway_core::{
        finality_detector::{assigned_weight_and_latest_unit, find_max_quora},
        Panorama, State,
    },
    utils::{ValidatorIndex, ValidatorMap},
    ClContext,
};
use casper_types::{EraId, PublicKey, Timestamp, U512};

use clap::Parser;
use flate2::read::GzDecoder;
use glium::{
    glutin::{
        event::{ElementState, Event, MouseButton, MouseScrollDelta, VirtualKeyCode, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::WindowBuilder,
        ContextBuilder,
    },
    Display,
};
use serde::{Deserialize, Serialize};

use crate::renderer::Renderer;

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

/// Helper struct for sorting the units with regards to the implicit partial ordering in the DAG.
struct Units {
    set: HashSet<Digest>,
    order: Vec<Digest>,
}

impl Units {
    /// Collects all the unit hashes and orders them roughly from the newest to the oldest.
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

    /// Reorders the units in self.order so that every unit comes after all its dependencies.
    fn reorder(&mut self, state: &State<ClContext>) {
        let mut new_order_set = HashSet::new();
        let mut new_order = vec![];
        let mut queue: VecDeque<_> = std::mem::take(&mut self.order).into_iter().rev().collect();
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

    /// Collects all the unit hashes and orders them so that every unit comes after all its
    /// dependencies.
    fn collect_ancestor_units(&mut self, state: &State<ClContext>) {
        self.do_collect_ancestor_units(state, state.panorama());
        self.reorder(state);
    }
}

/// A more readable unit ID: the validator index together with the height in that validator's
/// swimlane
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitId(ValidatorIndex, usize);

impl Debug for UnitId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "V{}_{}", self.0 .0, self.1)
    }
}

/// A more readable block id. The first field is the block height, the second is the number of the
/// block among all the blocks at that height (if there are no orphan blocks, all the block IDs will
/// have 0s in the second field).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(u64, u8);

impl Debug for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "B{}", self.0)?;
        for _ in 0..self.1 {
            write!(f, "'")?;
        }
        Ok(())
    }
}

/// A helper struct for coloring units based on the validator's max quorum.
/// `max_rank` is the number of distinct values of max quorum. `rank` is the index relative to the
/// maximum value (ie. the largest max quorum has rank 0, the second largest has rank 1 etc.)
#[derive(Clone, Copy)]
pub struct Quorum {
    pub rank: usize,
    pub max_rank: usize,
    pub weight_percent: f32,
}

impl Debug for Quorum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:3.1}", self.weight_percent)
    }
}

/// A representation of the protocol state unit for the purpose of drawing it on the screen.
/// `graph_height` is the maximum of graph heights of the cited units, plus 1 - drawing based on
/// graph height guarantees that every unit will appear higher than all its dependencies.
#[derive(Clone)]
pub struct GraphUnit {
    pub id: UnitId,
    pub creator: ValidatorIndex,
    pub vote: BlockId,
    pub is_proposal: bool,
    pub cited_units: Vec<UnitId>,
    pub height: usize,
    pub graph_height: usize,
    pub timestamp: u64,
    pub round_num: u64,
    pub round_id: Timestamp,
    pub round_exp: u8,
    pub max_quorum: Option<Quorum>,
}

impl Debug for GraphUnit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("GraphUnit")
            .field("id", &self.id)
            .field("creator", &format!("V{}", self.creator.0))
            .field("height", &self.height)
            .field("graph_height", &self.graph_height)
            .field("vote", &self.vote)
            .field("is_proposal", &self.is_proposal)
            .field("timestamp", &self.timestamp)
            .field("round_num", &self.round_num)
            .field("round_id", &self.round_id)
            .field("round_exp", &self.round_exp)
            .field("max_quorum", &self.max_quorum)
            .field("cited_units", &self.cited_units)
            .finish()
    }
}

/// A struct helping in assigning readable IDs to blocks.
#[derive(Clone, Debug)]
struct BlockMapper {
    hash_to_id: HashMap<Digest, BlockId>,
    id_to_hash: HashMap<BlockId, Digest>,
    last_id_by_height: HashMap<u64, u8>,
}

impl BlockMapper {
    fn new() -> Self {
        Self {
            hash_to_id: HashMap::new(),
            id_to_hash: HashMap::new(),
            last_id_by_height: HashMap::new(),
        }
    }

    /// Inserts the new ID, updating the mappings as necessary.
    fn insert(&mut self, hash: Digest, id: BlockId) {
        self.hash_to_id.insert(hash, id);
        self.id_to_hash.insert(id, hash);
        let entry = self.last_id_by_height.entry(id.0).or_insert(id.1);
        *entry = (*entry).max(id.1);
    }

    /// Returns the block ID for the next block at the given height.
    /// Usually, there will only be one block at a given height, but in some cases forks are
    /// possible - in those cases, blocks at the same height will get sequential IDs.
    fn next_id_for_height(&self, height: u64) -> BlockId {
        BlockId(
            height,
            self.last_id_by_height
                .get(&height)
                .map(|idx| *idx + 1)
                .unwrap_or(0),
        )
    }

    /// Gets the readable block ID corresponding to the given hash.
    fn get(&self, hash: &Digest) -> Option<BlockId> {
        self.hash_to_id.get(hash).copied()
    }

    /// Gets the block hash corresponding to the given ID.
    #[allow(unused)] // Will be useful if we add features related to the blocks
    fn get_by_id(&self, id: &BlockId) -> Option<Digest> {
        self.id_to_hash.get(id).copied()
    }
}

/// All the data needed for drawing the unit DAG.
#[derive(Clone, Debug)]
pub struct Graph {
    units: ValidatorMap<Vec<GraphUnit>>,
    reverse_edges: HashMap<UnitId, Vec<UnitId>>,
    #[allow(unused)] // Will be useful if we add features related to the blocks
    blocks: BlockMapper,
    weight_percentages: ValidatorMap<f32>,
}

impl Graph {
    /// Creates a `Graph` based on the `state`.
    fn new(state: &State<ClContext>, start_time: Timestamp) -> Self {
        let mut units: BTreeMap<ValidatorIndex, Vec<GraphUnit>> = state
            .weights()
            .iter()
            .enumerate()
            .map(|(idx, _)| (ValidatorIndex::from(idx as u32), vec![]))
            .collect();
        let mut reverse_edges: HashMap<UnitId, Vec<UnitId>> = HashMap::new();
        let mut unit_ids_by_hash: HashMap<Digest, UnitId> = HashMap::new();
        let mut blocks = BlockMapper::new();

        let mut units_set = Units {
            set: HashSet::new(),
            order: vec![],
        };

        units_set.collect_ancestor_units(state);

        eprintln!("num units: {}", units_set.order.len());

        let max_round_exp = (state.params().max_round_length().millis()
            / state.params().min_round_length().millis())
        .trailing_zeros();
        let max_round_length = state.params().min_round_length().millis() << max_round_exp;
        let rounded_era_start =
            Timestamp::from((start_time.millis() / max_round_length) * max_round_length);

        let mut highest_block: Option<(u64, Digest)> = None;

        for unit_hash in &units_set.order {
            let unit = state.unit(unit_hash);
            let block = state.block(&unit.block);
            if highest_block.map_or(true, |(height, _)| height < block.height) {
                highest_block = Some((block.height, unit.block));
            }
            let block_id = if let Some(b_id) = blocks.get(&unit.block) {
                b_id
            } else {
                let b_id = blocks.next_id_for_height(block.height);
                blocks.insert(unit.block, b_id);
                b_id
            };
            let is_proposal = unit
                .panorama
                .iter_correct_hashes()
                .all(|hash| state.unit(hash).block != unit.block);
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

            for cited_unit_id in &cited_units {
                reverse_edges
                    .entry(*cited_unit_id)
                    .or_default()
                    .push(unit_id);
            }

            let time_since_era_start = unit.timestamp.saturating_diff(rounded_era_start).millis();
            let round_num = time_since_era_start / state.params().min_round_length().millis();

            let graph_unit = GraphUnit {
                id: unit_id,
                creator: unit.creator,
                vote: block_id,
                is_proposal,
                cited_units,
                height: unit.seq_number as usize,
                graph_height,
                timestamp: time_since_era_start,
                round_num,
                round_id: unit.round_id(),
                round_exp: (unit.round_len().millis() / state.params().min_round_length().millis())
                    .trailing_zeros() as u8,
                max_quorum: None,
            };
            unit_ids_by_hash.insert(*unit_hash, unit_id);
            units.get_mut(&unit.creator).unwrap().push(graph_unit);
        }

        // fill in max quora
        if let Some((_hb_height, hb_hash)) = highest_block {
            let hb_unit = state.unit(&hb_hash);
            for bhash in state.ancestor_hashes(&hb_hash) {
                let proposal_unit = state.unit(bhash);
                let r_id = proposal_unit.round_id();

                let (assigned_weight, latest) =
                    assigned_weight_and_latest_unit(state, &hb_unit.panorama, r_id);

                let max_quora = find_max_quora(state, bhash, &latest);
                // deduplicate and sort max quora
                let max_quora_set: BTreeSet<_> = max_quora.iter().copied().collect();
                let max_quora_rank_map: BTreeMap<_, _> = max_quora_set
                    .into_iter()
                    .rev()
                    .enumerate()
                    .map(|(rank, quorum)| (quorum, rank))
                    .collect();

                for unit in latest.iter().flatten() {
                    let gunit_id = unit_ids_by_hash.get(*unit).unwrap();
                    let gunit = &mut units.get_mut(&gunit_id.0).unwrap()[gunit_id.1];
                    let quorum_w = max_quora[gunit.creator];
                    let rank = max_quora_rank_map[&quorum_w];
                    let weight_percent = quorum_w.0 as f32 / assigned_weight.0 as f32 * 100.0;
                    gunit.max_quorum = Some(Quorum {
                        rank,
                        max_rank: max_quora_rank_map.len(),
                        weight_percent,
                    });
                }
            }
        }

        let weight_percentages: ValidatorMap<f32> = state
            .weights()
            .iter()
            .map(|weight| weight.0 as f32 / state.total_weight().0 as f32 * 100.0)
            .collect();

        Self {
            units: units.into_values().collect(),
            reverse_edges,
            blocks,
            weight_percentages,
        }
    }

    /// Returns the unit under the given `unit_id`.
    pub fn get(&self, unit_id: &UnitId) -> Option<&GraphUnit> {
        self.units
            .get(unit_id.0)
            .and_then(|swimlane| swimlane.get(unit_id.1))
    }

    /// Returns the validator weights.
    pub fn validator_weights(&self) -> &ValidatorMap<f32> {
        &self.weight_percentages
    }

    /// Iterates over all the units created by validators within `range_vid` and with graph heights
    /// within `range_graph_height`.
    pub fn iter_range<R1, R2>(
        &self,
        range_vid: R1,
        range_graph_height: R2,
    ) -> impl Iterator<Item = &GraphUnit>
    where
        R1: RangeBounds<usize> + Clone,
        R2: RangeBounds<usize> + Clone,
    {
        let range_vid_clone = range_vid.clone();
        self.units
            .iter()
            .enumerate()
            .skip_while(move |(vid, _)| !range_vid.contains(vid))
            .take_while(move |(vid, _)| range_vid_clone.contains(vid))
            .flat_map(move |(_, swimlane)| {
                let range_graph_height_clone1 = range_graph_height.clone();
                let range_graph_height_clone2 = range_graph_height.clone();
                swimlane
                    .iter()
                    .skip_while(move |unit| !range_graph_height_clone1.contains(&unit.graph_height))
                    .take_while(move |unit| range_graph_height_clone2.contains(&unit.graph_height))
            })
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

    eprintln!("{}", dump.id);

    let graph = Graph::new(&dump.highway_state, dump.start_time);

    for (index, (pub_key, _)) in dump.validators.iter().enumerate() {
        eprintln!("{}: {}", index, pub_key);
    }

    start_rendering(graph);
}

/// Struct keeping the current state of some keys (the events only report the current state, so we
/// need to store the old state to know when it changes).
#[derive(Clone, Copy)]
struct KeyboardState {
    /// State of the 'E' key.
    e_state: bool,
}

impl KeyboardState {
    fn e_pressed(&mut self) -> bool {
        let was_pressed = self.e_state;
        self.e_state = true;
        !was_pressed
    }

    fn e_released(&mut self) {
        self.e_state = false;
    }
}

/// Enum keeping the state of mouse input.
#[derive(Clone, Copy)]
enum MouseState {
    /// Mouse is freely moving.
    Free { position: (f64, f64) },
    /// The user is dragging something.
    Dragging { last_position: (f64, f64) },
}

impl MouseState {
    /// Handles a mouse move event.
    /// Returns `Some(delta_x, delta_y)` if dragging is in progress.
    fn handle_move(&mut self, new_position: (f64, f64)) -> Option<(f32, f32)> {
        match self {
            Self::Free { position } => {
                *position = new_position;
                None
            }
            Self::Dragging { last_position } => {
                let delta_x = (new_position.0 - last_position.0) as f32;
                let delta_y = (new_position.1 - last_position.1) as f32;
                *last_position = new_position;
                Some((delta_x, delta_y))
            }
        }
    }

    /// Switches between `Free` and `Dragging` based on the button presses.
    fn handle_button(&mut self, button_down: bool) {
        match (*self, button_down) {
            (Self::Free { position }, true) => {
                *self = Self::Dragging {
                    last_position: position,
                };
            }
            (Self::Dragging { last_position }, false) => {
                *self = Self::Free {
                    position: last_position,
                };
            }
            _ => (),
        }
    }

    /// Returns the current position of the cursor.
    fn cursor(&self) -> (f32, f32) {
        match self {
            Self::Free { position } => (position.0 as f32, position.1 as f32),
            Self::Dragging { last_position } => (last_position.0 as f32, last_position.1 as f32),
        }
    }
}

/// The main loop of the program.
fn start_rendering(graph: Graph) {
    let event_loop = EventLoop::new();

    let wb = WindowBuilder::new()
        .with_title("Consensus Graph Visualization")
        .with_maximized(true)
        .with_resizable(true);
    let cb = ContextBuilder::new();
    let display = Display::new(wb, cb, &event_loop).unwrap();

    let mut renderer = Renderer::new(&display);
    let mut mouse_state = MouseState::Free {
        position: (0.0, 0.0),
    };
    let mut keyboard_state = KeyboardState { e_state: false };

    event_loop.run(move |ev, _, control_flow| {
        match ev {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                    return;
                }
                WindowEvent::MouseWheel { delta, .. } => match delta {
                    MouseScrollDelta::LineDelta(_, vertical) => {
                        renderer.mouse_scroll(vertical);
                    }
                    MouseScrollDelta::PixelDelta(pixels) => {
                        renderer.mouse_scroll(pixels.y as f32 / 30.0);
                    }
                },
                WindowEvent::KeyboardInput { input, .. } => {
                    match (input.virtual_keycode, input.state) {
                        (Some(VirtualKeyCode::E), ElementState::Pressed) => {
                            if keyboard_state.e_pressed() {
                                renderer.toggle_edges();
                            }
                        }
                        (Some(VirtualKeyCode::E), ElementState::Released) => {
                            keyboard_state.e_released();
                        }
                        _ => (),
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    if let (state, MouseButton::Left) = (state, button) {
                        mouse_state.handle_button(matches!(state, ElementState::Pressed));
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    if let Some(delta) = mouse_state.handle_move((position.x, position.y)) {
                        renderer.pan(delta.0, delta.1);
                    }
                }
                _ => (),
            },
            Event::MainEventsCleared => {
                let (cursor_x, cursor_y) = mouse_state.cursor();
                renderer.draw(&display, &graph, cursor_x, cursor_y);
            }
            _ => (),
        }
        *control_flow = ControlFlow::Poll;
    });
}
