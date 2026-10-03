//! Shared UI: marketing chrome, icons, illustrations (no domain logic).

pub mod footer;
pub mod icons;
pub mod illustrations;
pub mod nav;

pub use footer::public_footer;
pub use icons::{
	activity, discord, github, logo_colored, logo_full, right_arrow, transfer, wallet,
};
pub use illustrations::nintron;
pub use nav::public_nav;
