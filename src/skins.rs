//! Player cosmetics — hats, facial hair, and accessories for the Rustler character.
//!
//! Skins are purely visual: they never affect gameplay. A `PlayerSkin` is a bundle of optional
//! cosmetic slots (hat, facial hair, accessory) that the draw layer reads when rendering the
//! player character. The available options are defined as plain enums so new items can be added
//! without touching draw code — each draw function matches on the enum to pick a draw recipe.
//!
//! Persistence: skins are saved/loaded alongside the career file so choices survive across
//! sessions. The format is a single line appended to career.txt: `skin <hat> <facial> <acc>`,
//! where each field is the variant name (or "None").
//!
//! Unlock model (placeholder): all cosmetics start unlocked so the skeleton is immediately
//! playable. A proper unlock gate (career milestone, perk spend) can be layered in later without
//! changing this file's structure.

/// A hat worn by the player character on their claw.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Hat {
    #[default]
    None,
    Cowboy,     // classic wide-brim — fits the Rustler theme
    TopHat,     // formal; good contrast against the arena floor
    Sombrero,   // wide, festive; reads well at small size
    Bucket,     // casual; a nod to crab-catching culture
}

impl Hat {
    pub const ALL: &'static [Hat] = &[Hat::None, Hat::Cowboy, Hat::TopHat, Hat::Sombrero, Hat::Bucket];

    pub fn name(self) -> &'static str {
        match self {
            Hat::None => "None",
            Hat::Cowboy => "Cowboy",
            Hat::TopHat => "Top Hat",
            Hat::Sombrero => "Sombrero",
            Hat::Bucket => "Bucket",
        }
    }
}

/// Facial hair on the player character's face.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FacialHair {
    #[default]
    None,
    Mustache,       // classic curled ends
    Handlebar,      // wider, more dramatic
    Beard,          // full; makes the rustler look experienced
    GoateePatch,    // small chin patch
}

impl FacialHair {
    pub const ALL: &'static [FacialHair] = &[
        FacialHair::None,
        FacialHair::Mustache,
        FacialHair::Handlebar,
        FacialHair::Beard,
        FacialHair::GoateePatch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FacialHair::None => "None",
            FacialHair::Mustache => "Mustache",
            FacialHair::Handlebar => "Handlebar",
            FacialHair::Beard => "Beard",
            FacialHair::GoateePatch => "Goatee",
        }
    }
}

/// A small accessory item (badge, bandana, monocle, etc.).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Accessory {
    #[default]
    None,
    Bandana,    // around the neck; classic outlaw look
    Monocle,   // on the eyestalk
    StarBadge,  // sheriff's star on the shell
    BowTie,     // formal contrast to the chaos
}

impl Accessory {
    pub const ALL: &'static [Accessory] = &[
        Accessory::None,
        Accessory::Bandana,
        Accessory::Monocle,
        Accessory::StarBadge,
        Accessory::BowTie,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Accessory::None => "None",
            Accessory::Bandana => "Bandana",
            Accessory::Monocle => "Monocle",
            Accessory::StarBadge => "Star Badge",
            Accessory::BowTie => "Bow Tie",
        }
    }
}

/// The full cosmetic loadout for the player character. All fields default to `None` (bare crab).
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerSkin {
    pub hat: Hat,
    pub facial_hair: FacialHair,
    pub accessory: Accessory,
}

impl PlayerSkin {
    pub fn default_skin() -> Self {
        PlayerSkin::default()
    }

    /// Serialize to a single whitespace-separated line for appending to career.txt.
    pub fn to_save_line(&self) -> String {
        format!(
            "skin {:?} {:?} {:?}",
            self.hat, self.facial_hair, self.accessory
        )
    }

    /// Parse from the `skin ...` line in career.txt. Returns the default skin on any parse error
    /// so a corrupt line never crashes the game.
    pub fn from_save_line(line: &str) -> Self {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 4 || parts[0] != "skin" {
            return PlayerSkin::default();
        }
        let hat = match parts[1] {
            "Cowboy" => Hat::Cowboy,
            "TopHat" => Hat::TopHat,
            "Sombrero" => Hat::Sombrero,
            "Bucket" => Hat::Bucket,
            _ => Hat::None,
        };
        let facial_hair = match parts[2] {
            "Mustache" => FacialHair::Mustache,
            "Handlebar" => FacialHair::Handlebar,
            "Beard" => FacialHair::Beard,
            "GoateePatch" => FacialHair::GoateePatch,
            _ => FacialHair::None,
        };
        let accessory = match parts[3] {
            "Bandana" => Accessory::Bandana,
            "Monocle" => Accessory::Monocle,
            "StarBadge" => Accessory::StarBadge,
            "BowTie" => Accessory::BowTie,
            _ => Accessory::None,
        };
        PlayerSkin { hat, facial_hair, accessory }
    }
}
