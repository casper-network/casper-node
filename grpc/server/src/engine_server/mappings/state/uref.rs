use std::convert::TryFrom;

use types::{AccessRights, URef};

use crate::engine_server::{
    mappings::{self, ParsingError},
    state::{Key_URef, Key_URef_AccessRights},
};

impl From<AccessRights> for Key_URef_AccessRights {
    fn from(access_rights: AccessRights) -> Self {
        match access_rights {
            AccessRights::NONE => Key_URef_AccessRights::NONE,
            AccessRights::READ => Key_URef_AccessRights::READ,
            AccessRights::WRITE => Key_URef_AccessRights::WRITE,
            AccessRights::ADD => Key_URef_AccessRights::ADD,
            AccessRights::READ_ADD => Key_URef_AccessRights::READ_ADD,
            AccessRights::READ_WRITE => Key_URef_AccessRights::READ_WRITE,
            AccessRights::ADD_WRITE => Key_URef_AccessRights::ADD_WRITE,
            AccessRights::READ_ADD_WRITE => Key_URef_AccessRights::READ_ADD_WRITE,
            _ => Key_URef_AccessRights::NONE,
        }
    }
}

impl From<URef> for Key_URef {
    fn from(uref: URef) -> Self {
        let mut pb_uref = Key_URef::new();
        pb_uref.set_uref(uref.addr().to_vec());
        let access_rights = uref.access_rights();
        pb_uref.set_access_rights(access_rights.into());
        pb_uref
    }
}

impl TryFrom<Key_URef> for URef {
    type Error = ParsingError;

    fn try_from(pb_uref: Key_URef) -> Result<Self, Self::Error> {
        let addr = mappings::vec_to_array(pb_uref.uref, "Protobuf URef addr")?;

        let access_rights = match pb_uref.access_rights {
            Key_URef_AccessRights::NONE => AccessRights::NONE,
            Key_URef_AccessRights::READ => AccessRights::READ,
            Key_URef_AccessRights::WRITE => AccessRights::WRITE,
            Key_URef_AccessRights::ADD => AccessRights::ADD,
            Key_URef_AccessRights::READ_ADD => AccessRights::READ_ADD,
            Key_URef_AccessRights::READ_WRITE => AccessRights::READ_WRITE,
            Key_URef_AccessRights::ADD_WRITE => AccessRights::ADD_WRITE,
            Key_URef_AccessRights::READ_ADD_WRITE => AccessRights::READ_ADD_WRITE,
        };

        let uref = URef::new(addr, access_rights);

        Ok(uref)
    }
}

#[cfg(test)]
mod tests {
    use types::UREF_ADDR_LENGTH;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    #[test]
    fn round_trip() {
        for access_rights in &[
            AccessRights::READ,
            AccessRights::WRITE,
            AccessRights::ADD,
            AccessRights::READ_ADD,
            AccessRights::READ_WRITE,
            AccessRights::ADD_WRITE,
            AccessRights::READ_ADD_WRITE,
        ] {
            let uref = URef::new(rand::random(), *access_rights);
            test_utils::protobuf_round_trip::<URef, Key_URef>(uref);
        }

        let uref = URef::new(rand::random(), AccessRights::READ).remove_access_rights();
        test_utils::protobuf_round_trip::<URef, Key_URef>(uref);
    }

    #[test]
    fn should_fail_to_parse() {
        // Check we handle invalid Protobuf URefs correctly.
        let empty_pb_uref = Key_URef::new();
        assert!(URef::try_from(empty_pb_uref).is_err());

        let mut pb_uref_invalid_addr = Key_URef::new();
        pb_uref_invalid_addr.set_uref(vec![1; UREF_ADDR_LENGTH - 1]);
        assert!(URef::try_from(pb_uref_invalid_addr).is_err());

        // Check Protobuf URef with `AccessRights::UNKNOWN` parses to a URef with no access rights.
        let addr: [u8; UREF_ADDR_LENGTH] = rand::random();
        let mut pb_uref = Key_URef::new();
        pb_uref.set_uref(addr.to_vec());
        pb_uref.set_access_rights(Key_URef_AccessRights::NONE);
        let parsed_uref = URef::try_from(pb_uref).unwrap();
        assert_eq!(addr, parsed_uref.addr());
        assert!(parsed_uref.access_rights().is_none());
    }
}
