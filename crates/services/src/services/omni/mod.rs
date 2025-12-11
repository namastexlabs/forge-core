//! Omni integration service for WhatsApp/SMS notifications
//!
//! This module provides integration with the Omni API for sending
//! notifications via WhatsApp and SMS.

mod client;
mod service;
mod types;

pub use client::OmniClient;
pub use service::OmniService;
pub use types::*;
