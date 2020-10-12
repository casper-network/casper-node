# Cosy macros

The `cosy-macros` crate offers an easy-to-use macro to create reactor implementations for the component system. It enforces a set of convention and allows generating a large amount of otherwise boilerplate heavy setup code comfortably.

The macro is invoked by calling the `cosy_macros::reactor` macro as follows:

```rust
reactor!(NameOfReactor {
    type Config = ConfigurationType;

    components: {
        component_a = CompA<TypeArg>(constructor_arg_1, constructor_arg_2, ...);
        component_b = CompB(constructor_arg1, ..);
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
        component_b = CompB(constructor_arg1, ..);
        // ...
    }
```

Here

* two components will be defined as fields on the reactor struct, the fields being named `component_a` and `component_b` respectively,
* their type will be `crate::components::comp_a::CompA` (automatically deduced from the name),
* they will be constructed passing in `constructor_arg_1`, `constructor_arg_2` to the first and `constructor_arg1` to the second component in addition to the default parameters to constructors (TODO: document these),
* two variants `NameOfReactorEvent::ComponentA` and `NameOfReactorEvent::ComponentB` will be added to the reactors event type,
* these events will wrap `crate::components::comp_a::Event` (see caveat below) and `crate::components::comp_b::Event` respectively,
* a `From<crate::components::comp_a::Event>` impl will be generated for `NameOfReactorEvent` (similary for `comp_b`),
* the appropriate variants will similarly be added to the `NameOfReactorError` enum,
* and all variants of `NameOfReactorEvent` that wrap a components event will be forwarded to that components `handle_event` function.

## Event overrides

Ideally all `NameOfReactorEvent` newtype variants would be written as `NameOfReactorEvent::SomeComponent(<crate::components::some_component::SomeComponent as Component<Self>::Event>` in the generated code, which unfortunately is not possible due to a current shortcoming in the Rust trait system that will likely only be fixed with [chalk](https://github.com/rust-lang/chalk).

As a workround, `NameOfReactorEvent::SomeComponent(crate::components::some_component::Event` will be used as a workaround. This solution only works for event types that do not have their own type parameters. If they have, the `Event` portion can be replaced using the event override section of the macro invocation:

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
        AnotherRequest -> !;
    }
```

In the example,

* `StorageRequest`s are routed to `component_a`,
* `NetworkRequest`s are routed to `component_b`,
* `ThirdRequest`s are routed to `component_a` (note that multiple requests can be routed to a single component instance), and
* `AnotherRequest` is discarded entirely.

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

with the key difference being that instead of a single target, an announcement is routed to zero or more instead.
