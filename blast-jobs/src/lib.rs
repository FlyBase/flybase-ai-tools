//! `blast-jobs`: a minimal BLAST+ job runner implementing the FlyBase MCP's internal BLAST job API
//! (`docs/handoff-private-side.md` §7). Database mapping comes from a catalogue file and every limit
//! from the config file; nothing site-specific is compiled in.
pub mod advanced;
pub mod auth;
pub mod catalogue;
pub mod command;
pub mod config;
pub mod http;
pub mod jobid;
pub mod runner;
pub mod store;
pub mod types;
pub mod validate;
