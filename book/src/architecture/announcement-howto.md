# HOWTO: Adding an announcement

A question that sometimes comes up is how to notify a component about a change that happened in another one. As components are decoupled they do not know about specific other components, so the way we achieve this is by having one component announce new information, and one or more react to said announcement, all wired up inside the reactor.

Let's look at a concrete example (that we will never implement): We want the diagnostics console to send a message to all connected clients whenever a node starts or ceases to be come a validator. Our plan is to

* make the consensus component announce when the validator status changed,
* have the diagnostics console send a message on a validator status changed and
* wire these components up in the reactor to actually do so.

This is usually simple to do, since after a brief start, the compiler will guide us through most steps necessary.

## Adding an announcement

First, we create a new announcement in `announcements.rs`.

```rust,noplayground
/// An announcement indicating that the validator/non-validator status of the node changed.
#[derive(Debug, Serialize)]
pub(crate) struct ValidatorStatusChangedAnnouncement {
    /// Whether or not we are a validator now.
    pub(crate) new_status: bool,
}
```

A `Display` implementation will be needed later:

```rust,noplayground
impl Display for ValidatorStatusChangedAnnouncement {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "validator status changed to {}", self.new_status)
    }
}
```

We also add a method in the `EffectBuilder` in `effects.rs` to emit it:

```rust,noplayground
/// Announce that the validator status has been changed.
pub(crate) async fn announce_validator_status_changed(self, new_status: bool)
where
    REv: From<ValidatorStatusChangedAnnouncement>,
{
    self.event_queue
        .schedule(ValidatorStatusChangedAnnouncement { new_status }, QueueKind::Regular)
        .await
}
```

Now we can call `effect_builder.announce_validator_status_changed(..)` in another component and use it to return an effect that will cause the announcement to emitted.

## Emitting the announcement from the era supervisor

We gloss over the internal changes to the consensus/era supervisor component and assume that at some point, we detect the change in validator status, leading us to the following code in a nested call started from `handle_event` (`ProtocolOutcome` is domain logic from the consensus component):

```rust,noplayground
ProtocolOutcome::ValidatorStatusChanged(new_status) => effect_builder
    .announce_validator_status_changed(new_status)
    .ignore(),
```

Note that this will initially result in a compiler error like this:

```
error[E0277]: the trait bound `REv: From<ValidatorStatusChanged>` is not satisfied
    --> node/src/components/consensus/era_supervisor.rs:1166:18
```

This is because we have not declared that the component will now also emit this specific kind of event, which we need to do as well. The `REv` type in `Component<REv>` type dictates this, which in the case of `EraSupervisor`, is bound by the `ReactorEventT` trait:

```rust,noplayground
impl<REv> Component<REv> for EraSupervisor
where
    REv: ReactorEventT,
// ...
```

So we add this on the trait and in the generic implementation:

```rust,noplayground
pub(crate) trait ReactorEventT:
    // ...
    + From<ValidatorStatusChangedAnnouncement> {
}

// ...

impl<REv> ReactorEventT for REv where
    // ...
    + From<ValidatorStatusChangedAnnouncement> {
}
```

This concludes our work on the consensus component.

## Routing the announcement (to nowhere)

Since the compiler enforces that all outputs of a component need to be handled, we still get errors now:

```
error[E0277]: the trait bound `ParticipatingEvent: From<ValidatorStatusChangedAnnouncement>` is not satisfied
   --> node/src/reactor/participating.rs:881:32
```

This tells us that the participating reactor does not handle the `ValidatorStatusChangedAnnouncement`, but since it has a consensus component, it must do so. The only recommended way of handling any kind of event is by adding a variant for it to the reactor component, for which we use the `#[from]` macro:

```rust,noplayground
pub(crate) enum ParticipatingEvent {
    // ...
    /// Validator status change announcement.
    #[from]
    ValidatorStatusChangedAnnouncement(ValidatorStatusChangedAnnouncement),
```

The compiler will tell us that we need to handle that variant in the routing code

```
error[E0004]: non-exhaustive patterns: `ValidatorStatusChangedAnnouncement(_)` not covered
   --> node/src/reactor/participating.rs:776:15
```

so we add an action, by just logging it -- we do not have an interested party for it yet:

```rust,noplayground
fn dispatch_event(
  // ...
    ParticipatingEvent::ValidatorStatusChangedAnnouncement(_) => {
        debug!("TODO: swallowed a validator status changed announcement");
        Effects::new()
    }
  // ...
```

The necessary additions to the `description` and `Display::fmt` methods of `ParticipatingEvent` are not shown here.

With these changes, the node will actually compile again and output a debug-level log messages when the announcement is made. We can now continue by implementing a consumer of this announcement.

## Consuming an announcement

In general, making a component handle a request or announcement `A` is done by adding a `From<A>` implementation to its event type. These should almost always be handled by adding a variant to its own event type, then handling it, so we do just that, again using the `#[from]` macro. We're in luck - currently the diagnostics console has no events it handles, so we get to add the first one (and change it from a `struct` to an `enum`) in `diagnostics_console.rs`:

```rust,noplayground
use derive_more::From;

// ...

#[derive(Debug, From, Serialize)]
pub(crate) enum Event {
    /// Received a validator status changed announcement.
    #[from]
    ValidatorStatusChangedAnnouncement(ValidatorStatusChangedAnnouncement),
}
```

After having added it, we must process the event (when editing a component that already has `enum` events, we would even get a compiler error):

```rust,noplayground
fn handle_event(
    &mut self,
    _effect_builder: EffectBuilder<REv>,
    _rng: &mut NodeRng,
    event: Event,
) -> Effects<Event> {
    match event {
        Event::ValidatorStatusChangedAnnouncement(ValidatorStatusChangedAnnouncement {
            new_status,
        }) => {
            debug!(%new_status, "notify connected clients about new validator status");
            // not shown: actual sending of the message to connected clients
            Effects::new()
        }
    }
}
```

Now we have almost all the parts we need - a component that can send the new announcement, and one willing to receive it. All we have left to do is connect them.

## Connecting the two components through our announcement

To connect the components, we revisit the part in the routing code (in `dispatch_event`) and change it as follows:

```rust,noplayground
ParticipatingEvent::ValidatorStatusChangedAnnouncement(ann) => reactor::wrap_effects(
    ParticipatingEvent::DiagnosticsPort,
    self.diagnostics_port.handle_event(effect_builder, rng, ann.into()),
),
```

This pattern is very mechanical and should always look like this. In expresses that the announcement is handled (only) by the `self.diagnostics_port` and resulting effects should have their new events routed back to the `DiagnosticsPort`. If there were multiple parties interested in `ann`, it should be cloned before the effects of each are collected.
