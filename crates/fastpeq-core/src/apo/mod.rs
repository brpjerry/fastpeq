//! Everything that knows about the Equalizer APO configuration format.

pub mod env;
pub mod model;
mod parse;
mod serialize;
pub mod writer;

pub use model::{Channel, Config, Filter, FilterKind, Line};
pub use parse::parse;
pub use serialize::serialize;
pub use writer::{backup_once, write_bypass, write_config_atomic, write_text_atomic};
