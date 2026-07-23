//! Query preview pipeline: validate, wrap, execute, render.
//! Each stage is a separate module for clarity — the validator runs first
//! to reject unsafe queries; the engine (Phase 7) runs previews against
pub mod engine;
pub mod wrapper;
pub mod validator;
