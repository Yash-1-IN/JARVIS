//! The permission gateway (spec §9). The model only proposes: every tool call goes through
//! `gateway::Gateway`, which decides from static policy, validated arguments and the taint
//! flag. Nothing here executes a tool.

// The tools that call the gateway arrive in the next M4 PR.
#![allow(dead_code)]

pub mod activity;
pub mod gateway;
pub mod paths;
