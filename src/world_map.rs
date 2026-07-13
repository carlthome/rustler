//! Campaign world map — the screen between the title and a campaign run.
//!
//! Wraps the existing `Level` list as a linear chain of `WorldMapNode`s laid out on a map
//! canvas. The player navigates with arrow keys, selects a node, and launches it as a campaign
//! run. Completing a node unlocks the next. Content is intentionally sparse at this stage —
//! the skeleton is here so future agents can add branches, story beats, and biome art without
//! rearchitecting anything.

use crate::levels::get_levels;

/// One stop on the world map. Wraps a `Level` by index so the existing `get_levels()` data
/// stays authoritative — WorldMapNode is purely campaign metadata on top of it.
pub struct WorldMapNode {
    /// Index into `get_levels()`.
    pub level_index: usize,
    /// Short display name shown on the map (may differ from `Level::title`).
    pub name: &'static str,
    /// Normalized position (0..1, 0..1) on the map canvas. Converted to screen coords at draw time.
    pub position: (f32, f32),
    pub completed: bool,
    pub unlocked: bool,
}

/// The campaign world map. Owns the node list and tracks which node is selected.
pub struct WorldMap {
    pub nodes: Vec<WorldMapNode>,
    /// Index of the currently highlighted node.
    pub selected: usize,
}

impl WorldMap {
    /// Build the world map from the current level list. First node always unlocked; the rest
    /// start locked until the previous one is completed.
    pub fn new() -> Self {
        let levels = get_levels();
        // Positions trace a gentle S-curve across the map so nodes read as a journey, not a
        // spreadsheet row. Add more entries here as new levels land.
        let positions: &[(f32, f32)] = &[
            (0.12, 0.55),
            (0.35, 0.38),
            (0.58, 0.62),
            (0.82, 0.45),
        ];
        let names: &[&'static str] = &[
            "Sunny Meadow",
            "Tide Pools",
            "Rocky Shore",
            "Crab Rave",
        ];
        let nodes = levels
            .iter()
            .enumerate()
            .map(|(i, _level)| WorldMapNode {
                level_index: i,
                name: names.get(i).copied().unwrap_or("???"),
                position: positions.get(i).copied().unwrap_or((0.5, 0.5)),
                completed: false,
                unlocked: i == 0,
            })
            .collect();
        WorldMap { nodes, selected: 0 }
    }

    /// The `Level` index that should be loaded when the player confirms from this map.
    pub fn selected_level_index(&self) -> usize {
        self.nodes[self.selected].level_index
    }

    /// Mark the currently selected node complete and unlock the next one.
    pub fn complete_selected(&mut self) {
        self.nodes[self.selected].completed = true;
        let next = self.selected + 1;
        if next < self.nodes.len() {
            self.nodes[next].unlocked = true;
        }
    }

    /// Move selection left (delta = -1) or right (delta = +1), skipping locked nodes.
    /// Clamps at the ends so it never wraps.
    pub fn move_selection(&mut self, delta: i32) {
        let len = self.nodes.len() as i32;
        let mut target = self.selected as i32 + delta;
        while target >= 0 && target < len {
            if self.nodes[target as usize].unlocked {
                self.selected = target as usize;
                return;
            }
            target += delta;
        }
        // Nothing unlocked in that direction — stay put.
    }

    /// True once every node has been completed (end of campaign).
    pub fn is_complete(&self) -> bool {
        self.nodes.iter().all(|n| n.completed)
    }
}
