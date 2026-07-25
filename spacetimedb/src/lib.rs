mod tables;

pub use tables::*;

use spacetimedb::{ReducerContext, reducer};

#[reducer(init)]
pub fn init(_ctx: &ReducerContext) {
    log::info!("stelofinance module initialized");
}
