//! mrox: a Slack bot that DMs a confession's submitter when Owl reports the confession's status.
//! Behaviour mirrors the TypeScript bot in `test.ts`; `rust/PARITY.md` lists every item and its status.
pub mod bot;
pub mod config;
pub mod js;
pub mod slack;
pub mod store;
#[cfg(test)]
mod testkit;
