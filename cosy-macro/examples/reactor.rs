use cosy_macros::reactor;

type NodeId = usize;

pub mod effect {
    pub mod requests {
        #[derive(Debug)]
        pub struct NetworkRequest;

        impl std::fmt::Display for NetworkRequest {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(self, f)
            }
        }
        #[derive(Debug)]
        pub struct StorageRequest;

        impl std::fmt::Display for StorageRequest {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(self, f)
            }
        }

        #[derive(Debug)]
        pub struct PublisherRequest;

        impl std::fmt::Display for PublisherRequest {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(self, f)
            }
        }
    }
}

pub mod reactor {
    use rand::RngCore;
    use std::fmt::{Debug, Display};

    pub type Effects<T> = Vec<T>;

    pub fn wrap_effects<Ev, REv, F>(wrap: F, effects: Effects<Ev>) -> Effects<REv>
    where
        F: Fn(Ev) -> REv + Send + 'static + Clone,
        Ev: Send + 'static,
        REv: Send + 'static,
    {
        todo!()
    }

    pub struct Registry;
    pub struct EventQueueHandle<Ev>(Ev);

    pub struct EffectBuilder<Ev>(Ev);

    pub trait Reactor: Sized {
        type Event: Send + Debug + Display + 'static;
        type Config;
        type Error: Send + Sync + 'static;

        fn dispatch_event(
            &mut self,
            effect_builder: EffectBuilder<Self::Event>,
            rng: &mut dyn RngCore,
            event: Self::Event,
        ) -> Effects<Self::Event>;

        fn new(
            cfg: Self::Config,
            registry: &Registry,
            event_queue: EventQueueHandle<Self::Event>,
            rng: &mut dyn RngCore,
        ) -> Result<(Self, Effects<Self::Event>), Self::Error>;
    }
}

pub mod components {
    use std::fmt;
    pub mod small_network {
        pub type Event = String;

        #[derive(Debug)]
        pub struct SmallNetwork<N> {
            id: N,
        }

        impl<REv, N> super::Component<REv> for SmallNetwork<N> {
            type Event = Event;
        }
    }

    pub mod storage {
        pub type Event = usize;

        #[derive(Debug)]
        pub struct Storage;

        impl<REv> super::Component<REv> for Storage {
            type Event = Event;
        }
    }

    pub mod publisher {
        pub type Event = usize;

        #[derive(Debug)]
        pub struct Publisher;

        impl<REv> super::Component<REv> for Publisher {
            type Event = usize;
        }
    }

    use rand::RngCore;

    pub trait Component<REv> {
        type Event: fmt::Debug + fmt::Display;

        fn handle_event(
            &mut self,
            effect_builder: crate::reactor::EffectBuilder<REv>,
            rng: &mut dyn RngCore,
            event: Self::Event,
        ) -> crate::reactor::Effects<Self::Event> {
            todo!()
        }
    }
}

fn main() {}

// ---------------------- CODE GENERATION BELOW -------------------------

// use components::{publisher::Publisher, storage::Storage};

reactor!(ExampleReactor {
    type Config = String;

    components: {
        net = SmallNetwork<NodeId>(cfg.network, true);
        net2 = SmallNetwork<NodeId>(cfg.network, true);
        storage = Storage(cfg.storage);
        publisher = Publisher();
    }

    events: {
        net = Event;
        net2 = Event;
    }

    requests: {
        StorageRequest -> storage;
        NetworkRequest -> net;
        PublisherRequest -> !;
    }

    announcements: {
        NetworkAnnouncement -> [publisher, net];
        StorageAnnouncement -> [];
    }
});
