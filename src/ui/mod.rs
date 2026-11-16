//! Shared UI: marketing chrome, icons, illustrations, runtime widgets and display labels.

pub mod footer;
pub mod icons;
pub mod illustrations;
pub mod nav;
pub mod runtime;
pub mod text;

pub use footer::public_footer;
pub use icons::{
	activity, discord, github, logo_colored, logo_full, right_arrow, transfer, wallet,
};
pub use illustrations::nintron;
pub use nav::public_nav;
