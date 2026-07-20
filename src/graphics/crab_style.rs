//! Per-archetype visual identity for the crab renderer.
//!
//! Every crab used to share one silhouette and differ only in colour (see
//! `EnemyCrab::crab_color`). That read as "simplistic" — a Big crab and a Sneaky crab
//! had the same body, legs and claws. This module is the *shape* half of a crab's
//! identity: a pure-data table (`style_for`) that varies proportions, leg/claw geometry,
//! eyes, an accent colour and a shell pattern per `CrabType`, so each archetype reads at
//! a glance from its silhouette — a heavy Big crab, a skittish Sneaky one, a flashy
//! Dancer, an armour-plated tank, a masked Thief, etc.
//!
//! It holds no ggez state and issues no draw calls: `graphics::draw_crab` consumes a
//! `CrabStyle` and turns it into the same batched `UNIT_CIRCLE` / `UNIT_LINE` DrawParams
//! every crab part already uses, so the instanced-batch performance contract is untouched.

use crate::enemies::CrabType;

/// Shell-surface decoration that gives an archetype a distinct read on top of its colour.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShellPattern {
    /// Smooth carapace with the default two faint ridge dashes.
    Plain,
    /// Armored — segmented armour plates with rivet studs.
    Plates,
    /// Dancer — scattered disco polka spots that catch the light.
    Spots,
    /// Splitter — a bright cleaver line straight down the middle of the shell.
    Split,
    /// Hermit — a borrowed-shell spiral whorl.
    Whorl,
    /// Magnet — a polarity band across the top half of the shell.
    Bands,
    /// Thief — a bandit mask stripe across the eyes.
    Mask,
    /// Golden — bright treasure facets.
    Shine,
    /// Boss — a regal ridged crown along the top of the shell.
    Crown,
}

/// A crab's per-archetype visual identity. Every field is a factor on the baseline crab
/// (Normal ≈ all 1.0), so a Big crab is `body_w: 1.35` heavy, a Fast crab is streamlined
/// and long-legged, etc. `draw_crab` multiplies these onto the shared base geometry.
#[derive(Clone, Copy)]
pub struct CrabStyle {
    /// Shell width factor (baseline shell is `size * 0.62` wide).
    pub body_w: f32,
    /// Shell height factor (baseline shell is `size * 0.48` tall).
    pub body_h: f32,
    /// Legs per side at full detail (2–4).
    pub leg_pairs: usize,
    /// Leg length factor.
    pub leg_len: f32,
    /// Leg thickness factor.
    pub leg_thick: f32,
    /// How far the legs fan front-to-back (0 = bunched, 1 = wide stance).
    pub leg_splay: f32,
    /// Dominant (crusher) claw size factor.
    pub claw_scale: f32,
    /// Claw symmetry: 0 = one big crusher + one tiny pincer, 1 = matched twins.
    pub claw_sym: f32,
    /// Resting claw raise: 0 = held low, 1 = arms up (Dancer).
    pub claw_lift: f32,
    /// How far forward of the shell the claws reach (Thief grabs forward).
    pub claw_reach: f32,
    /// Eye radius factor.
    pub eye_size: f32,
    /// Eye separation factor.
    pub eye_spread: f32,
    /// Eyestalk length factor.
    pub stalk_len: f32,
    /// Secondary colour used for pattern accents, claw tips and eye rims.
    pub accent: [f32; 3],
    /// Shell-surface decoration.
    pub pattern: ShellPattern,
    /// Scuttle cadence factor (fast crabs skitter quicker).
    pub gait: f32,
}

impl CrabStyle {
    /// The neutral baseline every archetype tweaks from.
    const fn base() -> Self {
        CrabStyle {
            body_w: 1.0,
            body_h: 1.0,
            leg_pairs: 3,
            leg_len: 1.0,
            leg_thick: 1.0,
            leg_splay: 1.0,
            claw_scale: 1.0,
            claw_sym: 0.35,
            claw_lift: 0.15,
            claw_reach: 1.0,
            eye_size: 1.0,
            eye_spread: 1.0,
            stalk_len: 1.0,
            accent: [0.95, 0.95, 1.0],
            pattern: ShellPattern::Plain,
            gait: 1.0,
        }
    }
}

/// The visual identity for a crab archetype. Pure lookup — cheap enough to call per crab
/// per frame (it's a `match` returning a `Copy` struct).
pub fn style_for(t: CrabType) -> CrabStyle {
    use CrabType::*;
    let base = CrabStyle::base();
    match t {
        Normal => CrabStyle {
            accent: [0.95, 0.55, 0.5],
            ..base
        },
        // Streamlined sprinter: narrow low body, long thin swept legs, small claws, big
        // alert eyes on tall stalks. Reads "fast".
        Fast => CrabStyle {
            body_w: 0.86,
            body_h: 0.78,
            leg_pairs: 3,
            leg_len: 1.4,
            leg_thick: 0.75,
            leg_splay: 1.25,
            claw_scale: 0.7,
            claw_sym: 0.4,
            claw_lift: 0.05,
            claw_reach: 0.9,
            eye_size: 1.15,
            eye_spread: 1.15,
            stalk_len: 1.3,
            accent: [1.0, 0.85, 0.35],
            pattern: ShellPattern::Plain,
            gait: 1.6,
        },
        // Heavy tank: very wide tall dome, thick short legs, a huge asymmetric crusher,
        // small beady low-set eyes. Reads "heavy".
        Big => CrabStyle {
            body_w: 1.38,
            body_h: 1.2,
            leg_pairs: 4,
            leg_len: 0.85,
            leg_thick: 1.75,
            leg_splay: 0.95,
            claw_scale: 1.95,
            claw_sym: 0.4,
            claw_lift: 0.0,
            claw_reach: 1.1,
            eye_size: 0.72,
            eye_spread: 0.8,
            stalk_len: 0.7,
            accent: [0.75, 0.45, 0.9],
            pattern: ShellPattern::Plain,
            gait: 0.72,
        },
        // Skittish evader: small narrow hunched body, tiny nervous claws held up, huge
        // shifty eyes on very long stalks. Reads "twitchy".
        Sneaky => CrabStyle {
            body_w: 0.8,
            body_h: 0.86,
            leg_pairs: 3,
            leg_len: 1.15,
            leg_thick: 0.7,
            leg_splay: 1.15,
            claw_scale: 0.55,
            claw_sym: 0.45,
            claw_lift: 0.4,
            claw_reach: 0.85,
            eye_size: 1.35,
            eye_spread: 1.3,
            stalk_len: 1.55,
            accent: [0.7, 1.0, 1.0],
            pattern: ShellPattern::Plain,
            gait: 1.35,
        },
        // Armour-plated: wide flat carapace with segmented plates + rivets, thick stubby
        // legs, blunt medium claws, tiny tucked eyes. Reads "armored".
        Armored => CrabStyle {
            body_w: 1.22,
            body_h: 0.9,
            leg_pairs: 4,
            leg_len: 0.8,
            leg_thick: 1.55,
            leg_splay: 0.9,
            claw_scale: 1.1,
            claw_sym: 0.6,
            claw_lift: 0.0,
            claw_reach: 0.95,
            eye_size: 0.65,
            eye_spread: 0.72,
            stalk_len: 0.5,
            accent: [0.78, 0.86, 0.95],
            pattern: ShellPattern::Plates,
            gait: 0.78,
        },
        // Flashy performer: upright body dusted with disco spots, long slender legs, claws
        // thrown up like arms, big lashy eyes. Reads "dancer".
        Dancer => CrabStyle {
            body_w: 0.96,
            body_h: 1.06,
            leg_pairs: 3,
            leg_len: 1.45,
            leg_thick: 0.8,
            leg_splay: 1.1,
            claw_scale: 0.85,
            claw_sym: 0.6,
            claw_lift: 0.85,
            claw_reach: 1.0,
            eye_size: 1.15,
            eye_spread: 1.1,
            stalk_len: 1.35,
            accent: [0.6, 1.0, 1.0],
            pattern: ShellPattern::Spots,
            gait: 1.25,
        },
        // Lodestone: chunky round body with a polarity band, horseshoe-ish matched claws.
        // Reads "magnetic".
        Magnet => CrabStyle {
            body_w: 1.08,
            body_h: 1.02,
            leg_pairs: 3,
            leg_len: 1.0,
            leg_thick: 1.15,
            leg_splay: 1.0,
            claw_scale: 1.05,
            claw_sym: 0.75,
            claw_lift: 0.2,
            claw_reach: 1.0,
            eye_size: 0.95,
            eye_spread: 0.95,
            stalk_len: 0.95,
            accent: [1.0, 0.75, 0.3],
            pattern: ShellPattern::Bands,
            gait: 0.9,
        },
        // Masked raider: small wiry body, bandit mask stripe, long grabby forward claws,
        // quick thin legs. Reads "thief".
        Thief => CrabStyle {
            body_w: 0.82,
            body_h: 0.82,
            leg_pairs: 3,
            leg_len: 1.28,
            leg_thick: 0.72,
            leg_splay: 1.2,
            claw_scale: 1.0,
            claw_sym: 0.85,
            claw_lift: 0.55,
            claw_reach: 1.35,
            eye_size: 1.0,
            eye_spread: 1.15,
            stalk_len: 1.0,
            accent: [0.06, 0.14, 0.1],
            pattern: ShellPattern::Mask,
            gait: 1.4,
        },
        // Shelled lump: big round borrowed-shell whorl, only a couple of stubby legs poking
        // out front, small shy claws and eyes. Reads "hermit".
        Hermit => CrabStyle {
            body_w: 1.16,
            body_h: 1.16,
            leg_pairs: 2,
            leg_len: 0.72,
            leg_thick: 1.15,
            leg_splay: 0.8,
            claw_scale: 0.68,
            claw_sym: 0.5,
            claw_lift: 0.2,
            claw_reach: 0.85,
            eye_size: 0.9,
            eye_spread: 0.68,
            stalk_len: 0.78,
            accent: [1.0, 0.86, 0.62],
            pattern: ShellPattern::Whorl,
            gait: 0.9,
        },
        // Treasure prize: elegant faceted shell, bright gem eyes. Reads "shiny".
        Golden => CrabStyle {
            body_w: 0.98,
            body_h: 0.96,
            leg_pairs: 3,
            leg_len: 1.15,
            leg_thick: 0.9,
            leg_splay: 1.05,
            claw_scale: 0.85,
            claw_sym: 0.55,
            claw_lift: 0.25,
            claw_reach: 1.0,
            eye_size: 1.1,
            eye_spread: 1.05,
            stalk_len: 1.15,
            accent: [1.0, 1.0, 0.7],
            pattern: ShellPattern::Shine,
            gait: 1.25,
        },
        // Cleaver: shell split down the middle, symmetric scissor claws. Reads "splitter".
        Splitter => CrabStyle {
            body_w: 1.0,
            body_h: 0.95,
            leg_pairs: 3,
            leg_len: 1.1,
            leg_thick: 0.95,
            leg_splay: 1.05,
            claw_scale: 1.05,
            claw_sym: 1.0,
            claw_lift: 0.3,
            claw_reach: 1.05,
            eye_size: 1.0,
            eye_spread: 1.1,
            stalk_len: 1.05,
            accent: [0.7, 1.0, 0.95],
            pattern: ShellPattern::Split,
            gait: 1.1,
        },
        // Bosses: oversized, heavy-legged, giant crushers, crowned shell. Accent tints the
        // crown per boss so the three read apart even before their auras.
        Boss => CrabStyle {
            body_w: 1.15,
            body_h: 1.1,
            leg_pairs: 4,
            leg_len: 0.9,
            leg_thick: 1.85,
            leg_splay: 1.0,
            claw_scale: 2.05,
            claw_sym: 0.5,
            claw_lift: 0.1,
            claw_reach: 1.1,
            eye_size: 0.9,
            eye_spread: 0.9,
            stalk_len: 1.0,
            accent: [1.0, 0.92, 0.5],
            pattern: ShellPattern::Crown,
            gait: 0.7,
        },
        TideBoss => CrabStyle {
            body_w: 1.15,
            body_h: 1.1,
            leg_pairs: 4,
            leg_len: 0.9,
            leg_thick: 1.8,
            leg_splay: 1.05,
            claw_scale: 1.9,
            claw_sym: 0.55,
            claw_lift: 0.1,
            claw_reach: 1.1,
            eye_size: 0.9,
            eye_spread: 0.9,
            stalk_len: 1.0,
            accent: [0.55, 0.9, 1.0],
            pattern: ShellPattern::Crown,
            gait: 0.75,
        },
        RhythmBoss => CrabStyle {
            body_w: 1.12,
            body_h: 1.1,
            leg_pairs: 4,
            leg_len: 0.92,
            leg_thick: 1.75,
            leg_splay: 1.0,
            claw_scale: 1.85,
            claw_sym: 0.6,
            claw_lift: 0.2,
            claw_reach: 1.05,
            eye_size: 0.95,
            eye_spread: 0.92,
            stalk_len: 1.05,
            accent: [0.85, 0.6, 1.0],
            pattern: ShellPattern::Crown,
            gait: 0.95,
        },
    }
}
