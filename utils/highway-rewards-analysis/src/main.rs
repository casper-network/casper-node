use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::Read,
};

use clap::Parser;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use casper_node::consensus::{
    highway_core::{
        finality_detector::{
            assigned_weight_and_latest_unit, find_max_quora, round_participation,
            RoundParticipation,
        },
        State,
    },
    protocols::common::validators,
    utils::Validators,
    ClContext,
};
use casper_types::{EraId, PublicKey, Timestamp, U512};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    filename: String,
    #[arg(short, long)]
    verbose: bool,
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

    let validators =
        validators::<ClContext>(&dump.faulty, &dump.cannot_propose, dump.validators.clone());

    print_faults(&validators, &dump.highway_state);

    print_skipped_rounds(&validators, &dump, args.verbose);

    print_lowest_quorum_participation(&validators, &dump);
}

fn print_faults(validators: &Validators<PublicKey>, state: &State<ClContext>) {
    if state.faulty_validators().count() == 0 {
        return;
    }

    println!("Faults:");
    for vid in state.faulty_validators() {
        let fault = state.maybe_fault(vid).unwrap();
        println!("{}: {:?}", validators.id(vid).unwrap(), fault);
    }
    println!();
}

const TOP_TO_PRINT: usize = 10;

fn round_num(dump: &EraDump, round_id: Timestamp) -> u64 {
    let min_round_length = dump.highway_state.params().min_round_length();
    (round_id.millis() - dump.start_time.millis()) / min_round_length.millis()
}

fn print_skipped_rounds(validators: &Validators<PublicKey>, dump: &EraDump, verbose: bool) {
    let state = &dump.highway_state;
    let highest_block = state.fork_choice(state.panorama()).unwrap();
    let all_blocks = std::iter::once(highest_block).chain(state.ancestor_hashes(highest_block));
    let mut skipped_rounds = vec![vec![]; validators.len()];

    for block_hash in all_blocks {
        let block_unit = state.unit(block_hash);
        let round_id = block_unit.round_id();
        for i in 0..validators.len() as u32 {
            let observation = state.panorama().get(i.into()).unwrap();
            let round_participation = round_participation(state, observation, round_id);
            if matches!(round_participation, RoundParticipation::No) {
                skipped_rounds[i as usize].push(round_id);
            }
        }
    }

    for rounds in skipped_rounds.iter_mut() {
        rounds.sort();
    }

    let mut num_skipped_rounds: Vec<_> = skipped_rounds
        .iter()
        .enumerate()
        .map(|(i, skipped)| (i as u32, skipped.len()))
        .collect();
    num_skipped_rounds.sort_by_key(|(_, len)| *len);

    println!("{} validators who skipped the most rounds:", TOP_TO_PRINT);
    for (vid, len) in num_skipped_rounds.iter().rev().take(TOP_TO_PRINT) {
        println!(
            "{}: skipped {} rounds",
            validators.id((*vid).into()).unwrap(),
            len
        );
    }

    if verbose {
        println!();
        for (vid, _) in num_skipped_rounds.iter().rev().take(TOP_TO_PRINT) {
            let skipped_rounds: Vec<_> = skipped_rounds[*vid as usize]
                .iter()
                .map(|rid| format!("{}", round_num(dump, *rid)))
                .collect();
            println!(
                "{} skipped rounds: {}",
                validators.id((*vid).into()).unwrap(),
                skipped_rounds.join(", ")
            );
        }
    }

    println!();
}

fn print_lowest_quorum_participation(validators: &Validators<PublicKey>, dump: &EraDump) {
    let state = &dump.highway_state;
    let highest_block = state.fork_choice(state.panorama()).unwrap();
    let mut quora_sum = vec![0.0; validators.len()];
    let mut num_rounds = 0;

    let hb_unit = state.unit(highest_block);
    for bhash in state.ancestor_hashes(highest_block) {
        let proposal_unit = state.unit(bhash);
        let r_id = proposal_unit.round_id();

        let (assigned_weight, latest) =
            assigned_weight_and_latest_unit(state, &hb_unit.panorama, r_id);

        let max_quora = find_max_quora(state, bhash, &latest);
        let max_quora: Vec<f32> = max_quora
            .into_iter()
            .map(|quorum_w| quorum_w.0 as f32 / assigned_weight.0 as f32 * 100.0)
            .collect();

        for (q_sum, max_q) in quora_sum.iter_mut().zip(&max_quora) {
            *q_sum += max_q;
        }
        num_rounds += 1;
    }

    let mut quora_avg: Vec<_> = quora_sum
        .into_iter()
        .enumerate()
        .map(|(vid, q_sum)| (vid as u32, q_sum / num_rounds as f32))
        .collect();
    quora_avg.sort_by(|(_, q_avg1), (_, q_avg2)| q_avg1.partial_cmp(q_avg2).unwrap());

    println!("{} validators with lowest average max quora:", TOP_TO_PRINT);
    for (vid, q_avg) in quora_avg.iter().take(TOP_TO_PRINT) {
        println!(
            "{}: average max quorum {:3.1}%",
            validators.id((*vid).into()).unwrap(),
            q_avg
        );
    }
}
