//! Player cosmetics — crab persona customisation for the Rustler character.
//!
//! The pitch: everybody loves cool drip. Let players craft their own crab persona before a run —
//! a hat, some facial hair, an accessory. Purely visual, no gameplay effect. The combination
//! is what makes it expressive: a sombrero + handlebar mustache + star badge is a whole vibe.
//!
//! `PlayerSkin` is a bundle of three cosmetic slots. The available options are plain enums so
//! new items can be added without touching draw code — each draw function matches on the enum
//! to pick a draw recipe. Aim for options that are readable at crab-player scale (roughly 20px)
//! and have strong silhouettes so they're instantly recognisable.
//!
//! **Persistence:** saved alongside career.txt as `skin <Hat> <FacialHair> <Accessory>` (Debug
//! variant names). Corrupt lines fall back to the default (bare) skin so saves never crash.
//!
//! **Unlock model (placeholder):** everything unlocked from the start so the skeleton is
//! immediately playable. A proper unlock gate (career milestone → new cosmetic) can slot in
//! later without changing this file's structure — just add an `unlocked: bool` field to a
//! wrapper type when that work lands.

/// A hat worn on the player crab's shell/claw.
/// Strong silhouettes win at small size — prefer wide brims, tall crowns, distinctive shapes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Hat {
    #[default]
    None,
    Cowboy,   // wide brim, classic Rustler energy
    TopHat,   // tall and formal — unexpected on a crab, which is the point
    Sombrero, // extra-wide, festive, reads great at a glance
    Bucket,   // low-key bucket hat; the casual pick
    Bandana,  // tied around the shell; outlaw chic
    Beret,    // sideways on a claw; artiste vibes
    Crown,    // gold crown — reserved for champions
    HardHat,  // safety first, even while wrangling crabs
}

impl Hat {
    pub const ALL: &'static [Hat] = &[
        Hat::None,
        Hat::Cowboy,
        Hat::TopHat,
        Hat::Sombrero,
        Hat::Bucket,
        Hat::Bandana,
        Hat::Beret,
        Hat::Crown,
        Hat::HardHat,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Hat::None => "Bare",
            Hat::Cowboy => "Cowboy Hat",
            Hat::TopHat => "Top Hat",
            Hat::Sombrero => "Sombrero",
            Hat::Bucket => "Bucket Hat",
            Hat::Bandana => "Bandana",
            Hat::Beret => "Beret",
            Hat::Crown => "Crown",
            Hat::HardHat => "Hard Hat",
        }
    }

    /// One-line flavour text shown in the skin picker — the "vibe" of this hat.
    pub fn flavour(self) -> &'static str {
        match self {
            Hat::None => "Au naturel. The crab speaks for itself.",
            Hat::Cowboy => "Born on the beach. Died rustling crabs.",
            Hat::TopHat => "Formal occasion? Every run is a formal occasion.",
            Hat::Sombrero => "Wide brim. Wider attitude.",
            Hat::Bucket => "Chill out. The crabs aren't going anywhere.",
            Hat::Bandana => "The law doesn't come to the tidal zone.",
            Hat::Beret => "Caught 400 crabs. Called it a statement.",
            Hat::Crown => "You didn't find this. You earned this.",
            Hat::HardHat => "Health & Safety approved crab wrangler.",
        }
    }
}

/// Facial hair on the player crab's face/eyestalks.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FacialHair {
    #[default]
    None,
    Mustache,    // classic curled ends — the timeless choice
    Handlebar,   // wide, dramatic; pairs with anything formal
    Beard,       // full beard; this crab has been out here a while
    GoateePatch, // small chin tuft; understated
    Mutton,      // mutton chops on the claw joints; baroque energy
    FuManchu,    // long thin drops; maximum drama
}

impl FacialHair {
    pub const ALL: &'static [FacialHair] = &[
        FacialHair::None,
        FacialHair::Mustache,
        FacialHair::Handlebar,
        FacialHair::Beard,
        FacialHair::GoateePatch,
        FacialHair::Mutton,
        FacialHair::FuManchu,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FacialHair::None => "Clean",
            FacialHair::Mustache => "Mustache",
            FacialHair::Handlebar => "Handlebar",
            FacialHair::Beard => "Full Beard",
            FacialHair::GoateePatch => "Goatee",
            FacialHair::Mutton => "Mutton Chops",
            FacialHair::FuManchu => "Fu Manchu",
        }
    }

    pub fn flavour(self) -> &'static str {
        match self {
            FacialHair::None => "Smooth operator.",
            FacialHair::Mustache => "Distinguished. Trustworthy. Fast.",
            FacialHair::Handlebar => "Waxed before every run.",
            FacialHair::Beard => "Many runs. Much wisdom.",
            FacialHair::GoateePatch => "Subtle but intentional.",
            FacialHair::Mutton => "Victorian-era crab wrangling champion.",
            FacialHair::FuManchu => "Long enough to get caught in the lasso.",
        }
    }
}

/// A badge, accessory, or trinket the crab wears.
/// Think of it as the finishing touch — what makes the look complete.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Accessory {
    #[default]
    None,
    StarBadge, // sheriff's star on the shell — law of the beach
    Monocle,   // on the eyestalk; maximum class
    BowTie,    // formal; pair with the Top Hat for full tuxedo
    NeonChain, // gold chain around the shell; for the drip-forward players
    Shades,    // sunglasses on the eyestalks; effortlessly cool
    LassoLoop, // a coiled lasso worn on a claw; off-duty rustler
    GoldTooth, // glint in the smile; player knows what they're doing
}

impl Accessory {
    pub const ALL: &'static [Accessory] = &[
        Accessory::None,
        Accessory::StarBadge,
        Accessory::Monocle,
        Accessory::BowTie,
        Accessory::NeonChain,
        Accessory::Shades,
        Accessory::LassoLoop,
        Accessory::GoldTooth,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Accessory::None => "None",
            Accessory::StarBadge => "Star Badge",
            Accessory::Monocle => "Monocle",
            Accessory::BowTie => "Bow Tie",
            Accessory::NeonChain => "Neon Chain",
            Accessory::Shades => "Shades",
            Accessory::LassoLoop => "Lasso Loop",
            Accessory::GoldTooth => "Gold Tooth",
        }
    }

    pub fn flavour(self) -> &'static str {
        match self {
            Accessory::None => "Nothing extra. The drip is internal.",
            Accessory::StarBadge => "I don't enforce the rules. I AM the rules.",
            Accessory::Monocle => "One eye on the crabs. One eye on excellence.",
            Accessory::BowTie => "Even in chaos, presentation matters.",
            Accessory::NeonChain => "They see you coming. That's the point.",
            Accessory::Shades => "Too bright for the beach? Never.",
            Accessory::LassoLoop => "Retired? No. Just resting between catches.",
            Accessory::GoldTooth => "Earned it on a Crab Rave run.",
        }
    }
}

/// The full cosmetic loadout — a crab persona. Three slots, all optional.
/// Default is `None`/`None`/`None` (bare crab, no judgement).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerSkin {
    pub hat: Hat,
    pub facial_hair: FacialHair,
    pub accessory: Accessory,
}

impl PlayerSkin {
    pub fn default_skin() -> Self {
        PlayerSkin::default()
    }

    /// One sentence describing the current combo — shown as a persona tagline in the skin picker.
    pub fn tagline(&self) -> String {
        match (self.hat, self.facial_hair, self.accessory) {
            (Hat::Crown, _, _) => "Royalty on the beach.".into(),
            (Hat::Cowboy, FacialHair::Mustache, Accessory::StarBadge) => {
                "The full Sheriff. Nobody runs from the Sheriff.".into()
            }
            (Hat::TopHat, FacialHair::Handlebar, Accessory::BowTie) => {
                "Tuxedo crab. Black tie. Zero crabs escaped.".into()
            }
            (Hat::Sombrero, _, Accessory::NeonChain) => "Fiesta energy. Maximum drip.".into(),
            (_, _, Accessory::Shades) => "Too cool to panic. Crabs panic instead.".into(),
            (Hat::None, FacialHair::None, Accessory::None) => {
                "The raw crab. Unfiltered. Dangerous.".into()
            }
            _ => format!(
                "{} / {} / {}",
                self.hat.name(),
                self.facial_hair.name(),
                self.accessory.name()
            ),
        }
    }

    /// Serialize to a single whitespace-separated line for appending to career.txt.
    pub fn to_save_line(&self) -> String {
        format!(
            "skin {:?} {:?} {:?}",
            self.hat, self.facial_hair, self.accessory
        )
    }

    /// Parse from the `skin ...` line in career.txt. Falls back to default on any error.
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
            "Bandana" => Hat::Bandana,
            "Beret" => Hat::Beret,
            "Crown" => Hat::Crown,
            "HardHat" => Hat::HardHat,
            _ => Hat::None,
        };
        let facial_hair = match parts[2] {
            "Mustache" => FacialHair::Mustache,
            "Handlebar" => FacialHair::Handlebar,
            "Beard" => FacialHair::Beard,
            "GoateePatch" => FacialHair::GoateePatch,
            "Mutton" => FacialHair::Mutton,
            "FuManchu" => FacialHair::FuManchu,
            _ => FacialHair::None,
        };
        let accessory = match parts[3] {
            "StarBadge" => Accessory::StarBadge,
            "Monocle" => Accessory::Monocle,
            "BowTie" => Accessory::BowTie,
            "NeonChain" => Accessory::NeonChain,
            "Shades" => Accessory::Shades,
            "LassoLoop" => Accessory::LassoLoop,
            "GoldTooth" => Accessory::GoldTooth,
            _ => Accessory::None,
        };
        PlayerSkin {
            hat,
            facial_hair,
            accessory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_round_trip() {
        let skin = PlayerSkin {
            hat: Hat::Cowboy,
            facial_hair: FacialHair::Mustache,
            accessory: Accessory::StarBadge,
        };
        let line = skin.to_save_line();
        let loaded = PlayerSkin::from_save_line(&line);
        assert_eq!(loaded.hat, Hat::Cowboy);
        assert_eq!(loaded.facial_hair, FacialHair::Mustache);
        assert_eq!(loaded.accessory, Accessory::StarBadge);
    }

    #[test]
    fn default_skin_round_trip() {
        let skin = PlayerSkin::default_skin();
        let line = skin.to_save_line();
        let loaded = PlayerSkin::from_save_line(&line);
        assert_eq!(loaded.hat, Hat::None);
        assert_eq!(loaded.facial_hair, FacialHair::None);
        assert_eq!(loaded.accessory, Accessory::None);
    }

    #[test]
    fn corrupt_line_returns_default() {
        let loaded = PlayerSkin::from_save_line("garbage data here");
        assert_eq!(loaded.hat, Hat::None);
        assert_eq!(loaded.accessory, Accessory::None);
    }

    #[test]
    fn tagline_full_sheriff() {
        let skin = PlayerSkin {
            hat: Hat::Cowboy,
            facial_hair: FacialHair::Mustache,
            accessory: Accessory::StarBadge,
        };
        assert!(skin.tagline().contains("Sheriff"));
    }

    #[test]
    fn all_hats_have_names_and_flavour() {
        for hat in Hat::ALL {
            assert!(!hat.name().is_empty());
            assert!(!hat.flavour().is_empty());
        }
    }

    #[test]
    fn all_accessories_have_names_and_flavour() {
        for acc in Accessory::ALL {
            assert!(!acc.name().is_empty());
            assert!(!acc.flavour().is_empty());
        }
    }
}
