#![cfg_attr(target_arch = "wasm32", no_main)]

pub mod contracts;
pub mod traits;

#[cfg(test)]
mod tests {

    use alloc::collections::{BTreeMap, BTreeSet};

    use casper_macros::selector;
    use casper_sdk::{
        host::native::{self, dispatch, ExportKind},
        schema::CasperSchema,
    };
    use vm_common::{flags::EntryPointFlags, selector::Selector};

    use crate::{
        contracts::harness::{Harness, HarnessRef, INITIAL_GREETING},
        traits::FallbackRef,
    };

    use super::*;

    #[test]
    fn trait_has_interface() {
        let interface = FallbackRef::SELECTOR;
        assert_eq!(interface, Selector::zero());
    }

    #[test]
    fn fallback_trait_flags_selectors() {
        // let mut value = 0;

        // for entry_point in HarnessRef::ENTRY_POINTS.iter().map(|e| *e).flatten() {
        //     if entry_point.flags == EntryPointFlags::FALLBACK.bits() {
        //         value ^= entry_point.selector;
        //     }
        // }

        // assert_eq!(value, 0);
    }

    #[test]
    fn test() {
        dispatch(|| {
            native::call_export("call");
        })
        .unwrap();
    }

    #[test]
    fn exports() {
        let exports = native::list_exports()
            .into_iter()
            .filter_map(|e| match e.kind {
                ExportKind::SmartContract {} => None,
                ExportKind::TraitImpl {} => None,
                ExportKind::Function { name } => Some(name),
            })
            .collect::<Vec<_>>();
        assert_eq!(exports, vec!["call"]);
    }

    #[test]
    fn compile_time_schema() {
        let schema = Harness::schema();
        dbg!(&schema);
        // println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }

    #[test]
    fn should_greet() {
        assert_eq!(Harness::name(), "Harness");
        let mut flipper = Harness::constructor_with_args("Hello".into());
        assert_eq!(flipper.get_greeting(), "Hello"); // TODO: Initializer
        flipper.set_greeting("Hi".into());
        assert_eq!(flipper.get_greeting(), "Hi");
    }

    #[test]
    fn unittest() {
        dispatch(|| {
            let mut foo = Harness::initialize();
            assert_eq!(foo.get_greeting(), INITIAL_GREETING);
            foo.set_greeting("New greeting".to_string());
            assert_eq!(foo.get_greeting(), "New greeting");
        })
        .unwrap();
    }

    #[test]
    fn list_of_constructors() {
        let schema = Harness::schema();
        let constructors: BTreeSet<_> = schema
            .entry_points
            .iter()
            .filter_map(|e| {
                if e.flags.contains(EntryPointFlags::CONSTRUCTOR) {
                    Some(e.name.as_str())
                } else {
                    None
                }
            })
            .collect();
        let expected = BTreeSet::from_iter([
            "constructor_with_args",
            "failing_constructor",
            "trapping_constructor",
            "initialize",
        ]);

        assert_eq!(constructors, expected);
    }

    #[test]
    fn check_schema_selectors() {
        let schema = Harness::schema();

        let schema_mapping: BTreeMap<_, _> = schema
            .entry_points
            .iter()
            .map(|e| (e.name.as_str(), e.selector))
            .collect();

        // let manifest = &Harness::MANIFEST;
        todo!();

        // let manifest_selectors: BTreeSet<u32> = manifest.iter().map(|e| e.selector).collect();

        // assert_eq!(schema_mapping["constructor_with_args"], Some(4116419170),);
        // assert!(manifest_selectors.contains(&4116419170));
    }

    #[test]
    fn verify_check_private_and_public_methods() {
        // dispatch(|| {
        //     Harness::default().private_function_that_should_not_be_exported();
        //     Harness::default().restricted_function_that_should_be_part_of_manifest();
        // })
        // .expect("No trap");

        // let manifest = &Harness::MANIFEST;
        // const PRIVATE_SELECTOR: Selector =
        //     selector!("private_function_that_should_not_be_exported");
        // const PUB_CRATE_SELECTOR: Selector =
        //     selector!("restricted_function_that_should_be_part_of_manifest");
        // assert!(
        //     manifest
        //         .iter()
        //         .find(|e| e.selector == PRIVATE_SELECTOR.get())
        //         .is_none(),
        //     "This entry point should not be part of manifest"
        // );
        // assert!(
        //     manifest
        //         .iter()
        //         .find(|e| e.selector == PUB_CRATE_SELECTOR.get())
        //         .is_some(),
        //     "This entry point should be part of manifest"
        // );

        // let schema = Harness::schema();
        // assert!(
        //     schema
        //         .entry_points
        //         .iter()
        //         .find(|e| e.selector == Some(PRIVATE_SELECTOR.get()))
        //         .is_none(),
        //     "This entry point should not be part of schema"
        // );
        // assert!(
        //     schema
        //         .entry_points
        //         .iter()
        //         .find(|e| e.selector == Some(PUB_CRATE_SELECTOR.get()))
        //         .is_some(),
        //     "This entry point should be part ozf schema"
        // );
    }

    #[test]
    fn foo() {
        assert_eq!(Harness::default().into_greeting(), "Default value");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    panic!("Execute \"cargo test\" to test the contract, \"cargo build\" to build it");
}
