use std::fmt::{self, Display, Formatter};

/// A networking layer announcement.
#[derive(Debug)]
pub(crate) enum NetworkAnnouncement<I, P> {
    /// A payload message has been received from a peer.
    MessageReceived { sender: I, payload: P },
}

impl<I, P> Display for NetworkAnnouncement<I, P>
where
    I: Display,
    P: Display,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetworkAnnouncement::MessageReceived { sender, payload } => {
                write!(formatter, "received from {}: {}", sender, payload)
            }
        }
    }
}
