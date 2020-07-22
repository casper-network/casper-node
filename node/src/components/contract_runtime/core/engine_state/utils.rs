use casperlabs_types::{account::AccountHash, U512};

/// In PoS, the validators are stored under named keys with names formatted as
/// "v_<hex-formatted-AccountHash>_<bond-amount>".  This function attempts to parse such a string
/// back into the `AccountHash` and bond amount.
pub fn pos_validator_key_name_to_tuple(pos_key_name: &str) -> Option<(AccountHash, U512)> {
    let mut split_bond = pos_key_name.split('_'); // expected format is "v_{account_hash}_{bond}".
    if Some("v") != split_bond.next() {
        None
    } else {
        let hex_key: &str = split_bond.next()?;
        if hex_key.len() != 64 {
            return None;
        }
        let mut key_bytes = [0u8; 32];
        let _bytes_written = base16::decode_slice(hex_key, &mut key_bytes).ok()?;
        debug_assert!(_bytes_written == key_bytes.len());
        let pub_key = AccountHash::new(key_bytes);
        let balance = split_bond.next().and_then(|b| {
            if b.is_empty() {
                None
            } else {
                U512::from_dec_str(b).ok()
            }
        })?;
        Some((pub_key, balance))
    }
}

#[cfg(test)]
mod tests {
    use hex_fmt::HexFmt;

    use casperlabs_types::{account::AccountHash, U512};

    use super::pos_validator_key_name_to_tuple;

    #[test]
    fn should_parse_string_to_validator_tuple() {
        let account_hash = AccountHash::new([1u8; 32]);
        let stake = U512::from(100);
        let named_key_name = format!("v_{}_{}", HexFmt(&account_hash.as_bytes()), stake);

        let parsed = pos_validator_key_name_to_tuple(&named_key_name);
        assert!(parsed.is_some());
        let (parsed_account_hash, parsed_stake) = parsed.unwrap();
        assert_eq!(parsed_account_hash, account_hash);
        assert_eq!(parsed_stake, stake);
    }

    #[test]
    fn should_not_parse_string_to_validator_tuple() {
        let account_hash = AccountHash::new([1u8; 32]);
        let stake = U512::from(100);

        let bad_prefix = format!("a_{}_{}", HexFmt(&account_hash.as_bytes()), stake);
        assert!(pos_validator_key_name_to_tuple(&bad_prefix).is_none());

        let no_prefix = format!("_{}_{}", HexFmt(&account_hash.as_bytes()), stake);
        assert!(pos_validator_key_name_to_tuple(&no_prefix).is_none());

        let short_key = format!("v_{}_{}", HexFmt(&[1u8; 31]), stake);
        assert!(pos_validator_key_name_to_tuple(&short_key).is_none());

        let long_key = format!("v_{}00_{}", HexFmt(&account_hash.as_bytes()), stake);
        assert!(pos_validator_key_name_to_tuple(&long_key).is_none());

        let bad_key = format!("v_{}0g_{}", HexFmt(&[1u8; 31]), stake);
        assert!(pos_validator_key_name_to_tuple(&bad_key).is_none());

        let no_key = format!("v__{}", stake);
        assert!(pos_validator_key_name_to_tuple(&no_key).is_none());

        let no_key = format!("v_{}", stake);
        assert!(pos_validator_key_name_to_tuple(&no_key).is_none());

        let bad_stake = format!("v_{}_a", HexFmt(&account_hash.as_bytes()));
        assert!(pos_validator_key_name_to_tuple(&bad_stake).is_none());

        let no_stake = format!("v_{}_", HexFmt(&account_hash.as_bytes()));
        assert!(pos_validator_key_name_to_tuple(&no_stake).is_none());

        let no_stake = format!("v_{}", HexFmt(&account_hash.as_bytes()));
        assert!(pos_validator_key_name_to_tuple(&no_stake).is_none());
    }
}
