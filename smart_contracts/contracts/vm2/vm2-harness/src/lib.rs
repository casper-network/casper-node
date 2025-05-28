#![cfg_attr(target_arch = "wasm32", no_main)]

pub mod contracts;
pub mod traits;

#[cfg(test)]
mod tests {

    use casper_contract_sdk::casper::native::{self, dispatch, EntryPointKind};

    use crate::contracts::harness::{Harness, HarnessRef, INITIAL_GREETING};

    #[test]
    fn test() {
        dispatch(|| {
            native::invoke_export_by_name("call");
        })
        .unwrap();
    }

    #[test]
    fn exports() {
        let exports = native::ENTRY_POINTS
            .into_iter()
            .filter_map(|e| match e.kind {
                EntryPointKind::SmartContract { .. } => None,
                EntryPointKind::TraitImpl { .. } => None,
                EntryPointKind::Function { name } => Some(name),
            })
            .collect::<Vec<_>>();
        assert_eq!(exports, vec!["call"]);
    }

    #[test]
    fn should_greet() {
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
    fn foo() {
        assert_eq!(Harness::default().into_greeting(), "Default value");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    panic!("Execute \"cargo test\" to test the contract, \"cargo build\" to build it");
}
