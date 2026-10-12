//! Pure, deterministic rules for reviewing and selecting user-owned Slack messages.
//!
//! This crate never makes network requests or initiates deletion. Browser, Slack,
//! and storage integrations must live in separate adapters. In particular, a
//! locally confirmed batch is *not* proof of ownership at deletion time.

#![forbid(unsafe_code)]

pub mod identity;
pub mod links;
pub mod message;
pub mod navigation;
pub mod queue;
pub mod selection;
