use std::{collections::BTreeMap, fmt, fs::File, io::Read};

use clap::ArgMatches;

use casper_types::{
    bytesrepr::FromBytes, system::auction::SeigniorageRecipientsSnapshotV2, CLType,
    GlobalStateUpdate, GlobalStateUpdateConfig, Key, StoredValue,
};

struct Entries(BTreeMap<Key, StoredValue>);

impl fmt::Debug for Entries {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut map = f.debug_map();
        for (k, v) in &self.0 {
            let debug_v: Box<dyn fmt::Debug> = match v {
                StoredValue::CLValue(clv) => match clv.cl_type() {
                    CLType::Map { key, value: _ } if **key == CLType::U64 => {
                        // this should be the seigniorage recipient snapshot
                        let snapshot: SeigniorageRecipientsSnapshotV2 =
                            clv.clone().into_t().unwrap();
                        Box::new(snapshot)
                    }
                    _ => Box::new(clv),
                },
                _ => Box::new(v),
            };
            map.key(k).value(&debug_v);
        }
        map.finish()
    }
}

pub(crate) fn decode_file(matches: &ArgMatches<'_>) {
    let file_name = matches.value_of("file").unwrap();
    let mut file = File::open(file_name).unwrap();

    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();

    let config: GlobalStateUpdateConfig = toml::from_str(&contents).unwrap();
    let update_data: GlobalStateUpdate = config.try_into().unwrap();

    println!("validators = {:#?}", &update_data.validators);
    let entries: BTreeMap<_, _> = update_data
        .entries
        .iter()
        .map(|(key, bytes)| (*key, StoredValue::from_bytes(bytes).unwrap().0))
        .collect();
    println!("entries = {:#?}", Entries(entries));
}
