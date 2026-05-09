use crate::entity::PlayerId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerInput {
    pub xaxis: f32,
    pub yaxis: f32,
    pub fire: bool,
}

/// Map of player IDs to their inputs for one tick.
///
/// `BTreeMap` so iteration order is deterministic across machines, which
/// matters when multiple players act on the same tick.
pub type PlayerInputs = BTreeMap<PlayerId, PlayerInput>;
