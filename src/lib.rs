//! An HTTP load generator that sends requests at a fixed rate and measures each one's latency
//! from when it was supposed to be sent. A load generator that measures from when it actually
//! sent the request stops sending while the server is stalled, so the stall barely shows in its
//! numbers: Gil Tene's "coordinated omission".

pub mod http;
pub mod run;
pub mod server;
pub mod stats;

pub use run::{Mode, Plan, Report, run};
pub use stats::Summary;
