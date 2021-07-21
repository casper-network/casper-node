//! The engine which executes smart contracts on the Casper network.

<<<<<<< HEAD
#![doc(html_root_url = "https://docs.rs/casper-execution-engine/1.2.1")]
=======
#![doc(html_root_url = "https://docs.rs/casper-execution-engine/1.3.0")]
>>>>>>> release-1.3.0
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/casper-network/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/casper-network/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unused_qualifications
)]

pub mod config;
pub mod core;
pub mod shared;
pub mod storage;
