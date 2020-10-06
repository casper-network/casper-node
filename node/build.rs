use vergen::ConstantsFlags;

fn main() {
    let mut flags = ConstantsFlags::empty();
    flags.toggle(ConstantsFlags::SEMVER_LIGHTWEIGHT);
    flags.toggle(ConstantsFlags::SHA_SHORT);
    flags.toggle(ConstantsFlags::REBUILD_ON_HEAD_CHANGE);
    vergen::generate_cargo_keys(flags).expect("should generate the cargo keys");
}
