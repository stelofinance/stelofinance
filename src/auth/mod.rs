pub mod bitauth;
pub mod cookies;
pub mod jwt_peek;
pub mod session;

#[allow(unused_imports)] // public API for handlers / future /app gate
pub use session::{Bearer, EnsureBearerError, ensure_bearer};
