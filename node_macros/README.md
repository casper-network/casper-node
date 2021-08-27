# `casper-node-macros`

[![LOGO](https://raw.githubusercontent.com/casper-network/casper-node/master/images/casper-association-logo-primary.svg)](https://casper.network/)

[![Build Status](https://drone-auto-casper-network.casperlabs.io/api/badges/casper-network/casper-node/status.svg?branch=dev)](http://drone-auto-casper-network.casperlabs.io/casper-network/casper-node)
[![Crates.io](https://img.shields.io/crates/v/casper-node-macros)](https://crates.io/crates/casper-node-macros)
[![Documentation](https://docs.rs/casper-node-macros/badge.svg)](https://docs.rs/casper-node-macros)
[![License](https://img.shields.io/badge/license-Apache-blue)](https://github.com/CasperLabs/casper-node/blob/master/LICENSE)

## License

Licensed under the [Apache License Version 2.0](https://github.com/casper-network/casper-node/blob/master/LICENSE).

---

The `casper-node-macros` crate offers an easy-to-use macro to create reactor implementations for the component system. It enforces a set of convention and allows generating a large amount of otherwise boilerplate heavy setup code comfortably.

The macro is invoked by calling the `cosy_macro::reactor` macro as follows:

```rust
reactor!(NameOfReactor {
    type Config = ConfigurationType;

    components: {
        component_a = CompA<TypeArg>(constructor_arg_1, constructor_arg_2, ...);
        component_b = has_effects CompB(constructor_arg1, ..);
        // ...
    }

    events: {
        component_a = Event<TypeArg>;
    }

    requests: {
        StorageRequest -> component_a;
        NetworkRequest -> component_b;
        ThirdRequest -> component_a;
        AnotherRequest -> !;
    }

    announcements: {
        NetworkAnnouncement -> [component_a, component_b];
        StorageAnnouncement -> [];
    }
});
```

The sections in detail:

## Outer definition

The definition of

```rust
reactor!(NameOfReactor {
    type Config = ConfigurationType;
    // ...
});
```

indicates that

* the newly created reactor will be named `NameOfReactor` and
* its configuration type will be `ConfigurationType`.

The types `NameOfReactorEvent` and `NameOfReactorError` will be automatically generated as well.

## Component definition

Components are defined in the first section:

```rust
    components: {
        component_a = CompA<TypeArg>(constructor_arg_1, constructor_arg_2, ...);
        component_b = has_effects CompB(constructor_arg1, ..);
        // ...
    }
```

Here

* two components will be defined as fields on the reactor struct, the fields being named `component_a` and `component_b` respectively,
* their type will be `crate::components::comp_a::CompA` (automatically deduced from the name),
* they will be constructed passing in `constructor_arg_1`, `constructor_arg_2` to the first and `constructor_arg1` to the second component,
* `CompA::new` will return just the component, while `CompB::new` will return a tuple of `(component, effects)`, indicated by the `has_effects` keyword,
* two variants `NameOfReactorEvent::ComponentA` and `NameOfReactorEvent::ComponentB` will be added to the reactors event type,
* these events will wrap `crate::components::comp_a::Event` (see caveat below) and `crate::components::comp_b::Event` respectively,
* a `From<crate::components::comp_a::Event>` impl will be generated for `NameOfReactorEvent` (similarly for `comp_b`),
* the appropriate variants will similarly be added to the `NameOfReactorError` enum,
* and all variants of `NameOfReactorEvent` that wrap a component event will be forwarded to that component's `handle_event` function.

Note that during construction, the parameters `cfg`, `registry`, `event_queue` and `rng` are available, as well as the local variable `effect_builder`.

## Event overrides

Ideally all `NameOfReactorEvent` newtype variants would be written as `NameOfReactorEvent::SomeComponent(<crate::components::some_component::SomeComponent as Component<Self>::Event>` in the generated code, which unfortunately is not possible due to a current shortcoming in the Rust trait system that will likely only be fixed with [chalk](https://github.com/rust-lang/chalk).

As a workaround, `NameOfReactorEvent::SomeComponent(crate::components::some_component::Event` will be used instead. This solution only works for event types that do not have their own type parameters. If they have, the `Event` portion can be replaced using the event override section of the macro invocation:

```rust
    events: {
        component_a = Event<TypeArg>;
    }
```

This will result in `crate::components::comp_a::Event<TypeArg>` to be used to set the newtypes inner value instead.

## Request routing

The third section defines how requests are routed:

```rust
    requests: {
        StorageRequest -> component_a;
        NetworkRequest -> component_b;
        ThirdRequest -> component_a;
        AnotherRequest -> #;
    }
```

In the example,

* `StorageRequest`s are routed to `component_a`,
* `NetworkRequest`s are routed to `component_b`,
* `ThirdRequest`s are routed to `component_a` (note that multiple requests can be routed to a single component instance), and
* `AnotherRequest` is discarded quietly.

Instead of `#`, a request can be routed to `!`, which will panic once it receives a request.

Routing a request `ExampleRequest` to an `example_target` means that

* a `NameOfReactorEvent::ExampleRequest(ExampleRequest)` variant is generated,
* a `From<ExampleRequest>` is generated for `NameOfReactorEvent` and
* when dispatching a `NameOfReactorEvent::ExampleRequest` it will be turned into `example_target`'s event type using `From`, then dispatched to `example_target`'s `handle_event`.

Not routing a request means that the reactor does not support components that require it.

## Announcement routing

Announcements are routed almost exactly like requests

```rust
    announcements: {
        NetworkAnnouncement -> [component_a, component_b];
        StorageAnnouncement -> [];
    }
```

with the key difference being that instead of a single target, an announcement is routed to zero or more instead. `!` and `#` can be used as targets the same way they are used with requests as well.
