# Architecture FAQ

Here are some commonly asked questions regarding the reactor architecture.

## How do I make component X talk to component Y?

Components do not talk to each other directly but instead emit announcements, or make a request, both using the `effect_builder`.

Which component handles a specific announcement or request is decided outside the originating component in the reactor's `dispatch_event` function.

See the [announcement](announcement-howto.md) or [request](request-howto.md) HOWTO for details.

## What's the difference between a request and an announcement?

An announcement is "fire-and-forget" from the sender's perspective and can be handled by zero or more other components. In contrast, a request is usually handled by exactly one component and must include a reply to the original sender through a `Responder`.
