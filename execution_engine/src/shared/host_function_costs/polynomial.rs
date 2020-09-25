use bytesrepr::FromBytes;
use casper_types::bytesrepr::{self, ToBytes};
use serde::{Deserialize, Serialize};

const POLYNOMIAL_COEFFICIENT_TAG: u8 = 0;
const POLYNOMIAL_VARIABLE_TAG: u8 = 1;
const POLYNOMIAL_TAG_SIZE: usize = 1;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum PolynomialExpr {
    Coefficient(u32),
    Variable { name: String, value: u32 },
}

impl ToBytes for PolynomialExpr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        match self {
            PolynomialExpr::Coefficient(value) => {
                ret.push(POLYNOMIAL_COEFFICIENT_TAG);
                ret.append(&mut value.to_bytes()?);
            }
            PolynomialExpr::Variable { name, value } => {
                ret.push(POLYNOMIAL_VARIABLE_TAG);
                ret.append(&mut name.to_bytes()?);
                ret.append(&mut value.to_bytes()?);
            }
        }

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        POLYNOMIAL_TAG_SIZE
            + match self {
                PolynomialExpr::Coefficient(fixed_size) => fixed_size.serialized_length(),
                PolynomialExpr::Variable { name, value } => {
                    name.serialized_length() + value.serialized_length()
                }
            }
    }
}

impl FromBytes for PolynomialExpr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (variant_tag, bytes): (u8, _) = FromBytes::from_bytes(bytes)?;
        match variant_tag {
            POLYNOMIAL_COEFFICIENT_TAG => {
                let (value, bytes) = FromBytes::from_bytes(bytes)?;
                Ok((PolynomialExpr::Coefficient(value), bytes))
            }

            POLYNOMIAL_VARIABLE_TAG => {
                let (name, bytes) = FromBytes::from_bytes(bytes)?;
                let (value, bytes) = FromBytes::from_bytes(bytes)?;
                Ok((PolynomialExpr::Variable { name, value }, bytes))
            }

            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

pub type Polynomial = Vec<PolynomialExpr>; // TODO: Optimize for fixed costs with smallvec once bytesrepr supports it

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::prelude::*;

    use super::{Polynomial, PolynomialExpr};

    pub fn polynomial_expr_arb() -> impl Strategy<Value = PolynomialExpr> {
        prop_oneof![
            any::<u32>().prop_map(PolynomialExpr::Coefficient),
            (".+", any::<u32>()).prop_map(|(name, value)| PolynomialExpr::Variable { name, value })
        ]
    }

    pub fn polynomial_arb() -> impl Strategy<Value = Polynomial> {
        prop::collection::vec(polynomial_expr_arb(), 1..10)
    }
}
