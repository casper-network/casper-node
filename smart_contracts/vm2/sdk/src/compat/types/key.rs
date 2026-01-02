use borsh::{BorshDeserialize, BorshSerialize};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Debug, BorshDeserialize, BorshSerialize)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum KeyTag {
    Hash = 1,
    Package = 16,
}

/// Subset of node keys that can be stored through the sdk API
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Debug)]
pub enum Key {
    Hash([u8; 32]),
    Package([u8; 32]),
}

impl Key {
    fn get_tag(&self) -> KeyTag {
        match self {
            Key::Hash(_) => KeyTag::Hash,
            Key::Package(_) => KeyTag::Package,
        }
    }
}

impl BorshSerialize for Key {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let tag_bytes = self.get_tag() as u8;
        let _ = writer.write(&[tag_bytes])?;
        let _ = match self {
            Key::Hash(hash_addr) => writer.write(hash_addr)?,
            Key::Package(package_addr) => writer.write(package_addr)?,
        };
        Ok(())
    }
}

impl BorshDeserialize for Key {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let tag = <KeyTag as borsh::de::BorshDeserialize>::deserialize_reader(reader)?;
        let key = match tag {
            KeyTag::Hash => {
                let addr = <[u8; 32] as borsh::de::BorshDeserialize>::deserialize_reader(reader)?;
                Key::Hash(addr)
            }
            KeyTag::Package => {
                let addr = <[u8; 32] as borsh::de::BorshDeserialize>::deserialize_reader(reader)?;
                Key::Package(addr)
            }
        };
        Ok(key)
    }
}

#[test]
pub fn test_key_tag_serialization() {
    assert_eq!(borsh::to_vec(&KeyTag::Hash).unwrap(), [1]);
    assert_eq!(borsh::to_vec(&KeyTag::Package).unwrap(), [16]);
    assert_eq!(borsh::from_slice::<KeyTag>(&[1]).unwrap(), KeyTag::Hash);
    assert_eq!(borsh::from_slice::<KeyTag>(&[16]).unwrap(), KeyTag::Package);
    assert!(borsh::from_slice::<KeyTag>(&[3]).is_err());
}

#[test]
pub fn test_key_serialization() {
    assert_eq!(
        borsh::to_vec(&Key::Hash([1; 32])).unwrap(),
        [
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1
        ]
    );
    assert_eq!(
        borsh::to_vec(&Key::Package([2; 32])).unwrap(),
        [
            16, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            2, 2, 2, 2
        ]
    );
    let hash_bytes = [
        1_u8, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
        7, 7, 7, 7,
    ];
    assert_eq!(
        borsh::from_slice::<Key>(&hash_bytes).unwrap(),
        Key::Hash([7; 32])
    );
    let package_bytes = [
        16, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
        7, 7, 7,
    ];
    assert_eq!(
        borsh::from_slice::<Key>(&package_bytes).unwrap(),
        Key::Package([7; 32])
    );
}
