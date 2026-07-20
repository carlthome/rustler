//! Campaign world map — the screen between the title and a campaign run.
//!
//! Wraps the existing `Level` list as a linear chain of `WorldMapNode`s laid out on a map
//! canvas. The player navigates with arrow keys, selects a node, and launches it as a campaign
//! run. Completing a node unlocks the next. Content is intentionally sparse at this stage —
//! the skeleton is here so future agents can add branches, story beats, and biome art without
//! rearchitecting anything.
//!
//! The first four nodes are tutorial sandboxes — the new-player on-ramp lives here, not on a
//! separate "How to Play" menu screen. Each one teaches one core mechanic, then hands off to
//! the regular campaign levels that follow.

use crate::levels::get_levels;
use crate::tutorial::TutorialKind;

/// What a world-map node launches when the player confirms. Tutorial nodes run a scripted
/// sandbox; campaign nodes load a `Level` from `get_levels()`.
pub enum NodeKind {
    Tutorial(TutorialKind),
    /// Index into `get_levels()`.
    Level(usize),
}

/// One stop on the world map. Either a tutorial sandbox or a campaign level.
pub struct WorldMapNode {
    /// What this node launches.
    pub kind: NodeKind,
    /// Short display name shown on the map.
    pub name: &'static str,
    /// Normalized position (0..1, 0..1) on the map canvas. Converted to screen coords at draw time.
    pub position: (f32, f32),
    pub completed: bool,
    pub unlocked: bool,
}

impl WorldMapNode {
    /// Returns the level index if this is a campaign node, or None for tutorial nodes.
    pub fn level_index(&self) -> Option<usize> {
        match self.kind {
            NodeKind::Level(i) => Some(i),
            NodeKind::Tutorial(_) => None,
        }
    }

    /// Returns the tutorial kind if this is a tutorial node, or None for campaign nodes.
    pub fn tutorial_kind(&self) -> Option<TutorialKind> {
        match self.kind {
            NodeKind::Tutorial(k) => Some(k),
            NodeKind::Level(_) => None,
        }
    }
}

/// The campaign world map. Owns the node list and tracks which node is selected.
pub struct WorldMap {
    pub nodes: Vec<WorldMapNode>,
    /// Index of the currently highlighted node.
    pub selected: usize,
}

impl WorldMap {
    /// Build the world map. The first four nodes are tutorial sandboxes (the player's on-ramp);
    /// the remaining nodes wrap the regular campaign levels. First node always unlocked; the rest
    /// start locked until the previous one is completed.
    pub fn new() -> Self {
        // Tutorial nodes — teach one mechanic each, in escalating complexity.
        let tutorial_nodes: &[(TutorialKind, &'static str, (f32, f32))] = &[
            (TutorialKind::BeatTiming,  "The Beach — Catch the Beat",     (0.08, 0.60)),
            (TutorialKind::LassoGrab,   "The Docks — Throw the Lasso",    (0.22, 0.42)),
            (TutorialKind::ChainDeliver,"The Cove — Build a Train",        (0.36, 0.65)),
            (TutorialKind::ShellCrack,  "The Reef — Crack the Shells",     (0.50, 0.40)),
        ];

        // Campaign nodes — the regular levels follow after the tutorials.
        let campaign_positions: &[(f32, f32)] = &[
            (0.65, 0.62),
            (0.78, 0.38),
            (0.90, 0.55),
            (0.84, 0.78),
            // The Desktop sits off on its own, past the "end" of the map — you shouldn't be here.
            (0.96, 0.30),
        ];
        let campaign_names: &[&'static str] = &[
            "Sunny Meadow",
            "Tide Pools",
            "Crab Rave",
            "Neon Kelp",
            "The Desktop",
        ];

        let levels = get_levels();
        let total = tutorial_nodes.len() + levels.len();
        let mut nodes: Vec<WorldMapNode> = Vec::with_capacity(total);

        for (i, &(kind, name, position)) in tutorial_nodes.iter().enumerate() {
            nodes.push(WorldMapNode {
                kind: NodeKind::Tutorial(kind),
                name,
                position,
                completed: false,
                unlocked: i == 0,
            });
        }

        for (i, _level) in levels.iter().enumerate() {
            let map_i = tutorial_nodes.len() + i;
            nodes.push(WorldMapNode {
                kind: NodeKind::Level(i),
                name: campaign_names.get(i).copied().unwrap_or("???"),
                position: campaign_positions.get(i).copied().unwrap_or((0.5, 0.5)),
                completed: false,
                unlocked: map_i == 0,
            });
        }

        WorldMap { nodes, selected: 0 }
    }

    /// The `Level` index that should be loaded when the player confirms from this map.
    /// Returns None if the selected node is a tutorial node.
    pub fn selected_level_index(&self) -> Option<usize> {
        self.nodes[self.selected].level_index()
    }

    /// The tutorial kind for the selected node, if it is a tutorial node.
    pub fn selected_tutorial_kind(&self) -> Option<TutorialKind> {
        self.nodes[self.selected].tutorial_kind()
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
