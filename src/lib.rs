//! # CasperLabs blockchain node
//!
//! This crate contain the core application for the CasperLabs blockchain. Run with `--help` to see
//! available command-line arguments.
//!
//! ## Application structure
//!
//! While the [`main`](fn.main.html) function is the central entrypoint for the node application,
//! its core event loop is found inside the [reactor](reactor/index.html). To get a tour of the
//! sourcecode, be sure to run `cargo doc --open`.

#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_pub,
    unused_qualifications
)]

mod components;
pub mod effect;
pub mod reactor;
pub mod tls;
mod utils;

pub use components::small_network::Config as SmallNetworkConfig;
pub(crate) use components::small_network::{self, SmallNetwork};
