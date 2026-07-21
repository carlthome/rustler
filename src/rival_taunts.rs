//! Original post-clash quips for rival King Crabs, grouped by the personality implied by their name.

use rand::prelude::IndexedRandom;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RivalPersonality {
    Plain,
    Pirate,
    Pompous,
    Raver,
    Serious,
    Silly,
}

fn personality_for(name: &str) -> RivalPersonality {
    if [
        "Gravelord",
        "Devourer",
        "Lord ",
        "Sir ",
        "Eternal",
        "Abysswalker",
        "Brinewraith",
        "Moltveil",
        "Shellreaper",
        "Scuttlefiend",
    ]
    .iter()
    .any(|marker| name.contains(marker))
    {
        RivalPersonality::Pompous
    } else if [
        "DJ",
        "MC ",
        "Rave",
        "Drop Lord",
        "Selecta",
        "Beat-Droppin",
        "Neon",
        "Groove",
        "Bass",
        "Shellshaker",
        "Clawdrop",
        "Beatpincer",
        "Glowclaw",
        "Ravescuttle",
    ]
    .iter()
    .any(|marker| name.contains(marker))
    {
        RivalPersonality::Raver
    } else if [
        "Admiral",
        "Commodore",
        "Quartermaster",
        "First Mate",
        "Ironpincer",
        "Silverclaw",
    ]
    .iter()
    .any(|marker| name.contains(marker))
    {
        RivalPersonality::Serious
    } else if [
        "Cap'n",
        "Captain",
        "Bosun",
        "Pirate",
        "Corsair",
        "Buccaneer",
        "Peg-Leg",
        "One-Eyed",
        "Barnacle",
        "Scurvy",
        "Clawbeard",
        "Blackclaw",
        "Saltbeard",
        "Redbeard",
        "Bootstrap",
        "Flintclaw",
        "Longshanks",
        "Saltbitten",
    ]
    .iter()
    .any(|marker| name.contains(marker))
    {
        RivalPersonality::Pirate
    } else if [
        "Misterhult",
        "Uncle",
        "Sideways Champion",
        "Moultzilla",
        "Snippy",
        "Snapsalot",
    ]
    .iter()
    .any(|marker| name.contains(marker))
    {
        RivalPersonality::Silly
    } else {
        RivalPersonality::Plain
    }
}

const PLAIN: &[&str] = &[
    "That went badly for you.",
    "You missed the beat.",
    "I think these are mine now.",
    "Please collect your dignity.",
    "That was your big charge?",
    "I barely had to sidestep.",
    "Your conga needs practice.",
    "Try again, but less badly.",
    "Thanks for the spare crabs.",
    "I saw that coming.",
    "A bold plan. Not a good one.",
    "You zigged. I simply stood here.",
    "Perhaps start with clapping.",
    "Your tail seems lighter.",
    "No rhythm, no crabs.",
    "Kevin would have timed that better.",
];

const PIRATE: &[&str] = &[
    "Arrr, your stern is mine!",
    "That tail belongs to my crew now!",
    "Ye ram like a sleepy dinghy!",
    "Off beat and overboard!",
    "I plundered the rhythm right out of ye!",
    "Hoist yer claws and surrender the conga!",
    "A fine donation to me crew!",
    "Ye brought a wobble to a broadside!",
    "Dead crabs tell no tempo!",
    "Yer wake is full of loose followers!",
    "That charge sank before it sailed!",
    "I claim this clash in the name of me!",
    "Mind the barnacles on yer way back!",
    "Yer conga leaks from the stern!",
    "Another beat buried at sea!",
    "Come back when ye can count to four!",
];

const POMPOUS: &[&str] = &[
    "Kneel before superior synchronization.",
    "Your defeat was historically inevitable.",
    "I accept this tribute of followers.",
    "Behold: the correct way to clash.",
    "A footnote has challenged the legend.",
    "Your little rhythm amused me.",
    "The reef shall remember my timing.",
    "I have conquered worthier metronomes.",
    "Your conga now improves my procession.",
    "Even my recoil was magnificent.",
    "You have scuffed a royal shell.",
    "This beat answers to me.",
    "An adequate entrance. A dismal ending.",
    "My victory title grows longer.",
    "History will omit your charge.",
    "Announce another triumph for me.",
];

const RAVER: &[&str] = &[
    "You dropped the beat. I picked it up!",
    "Wrong beat, right into my claws!",
    "Your conga just joined my remix!",
    "That clash needs more bass!",
    "I own the dance floor now!",
    "Tempo checked. Shell wrecked.",
    "Your tail is my new backing track!",
    "The crowd goes sideways!",
    "That drop was mostly you falling.",
    "Four on the floor, you on the beach!",
    "Your rhythm just got rustled!",
    "My mix has more crabs now!",
    "You brought silence to a beat fight!",
    "Rewind that embarrassing charge!",
    "Feel the bass, lose the race!",
    "Next time, clash on the one!",
];

const SERIOUS: &[&str] = &[
    "Formation broken. Followers secured.",
    "Your timing was tactically unsound.",
    "Clash concluded in my favor.",
    "You exposed the rear of your line.",
    "Predictable charge. Clean response.",
    "Discipline beats enthusiasm.",
    "Your formation requires revision.",
    "The conga line will remain orderly.",
    "I counted your approach precisely.",
    "An avoidable loss.",
    "Your cadence betrayed your intent.",
    "Return when your line can hold.",
    "The weaker rhythm yields.",
    "Objective complete: tail disrupted.",
    "Poor timing compromises any charge.",
    "Consider this a practical lesson.",
];

const SILLY: &[&str] = &[
    "Bonk! Your crabs fell out!",
    "I call that the sideways surprise!",
    "Oops! Did I win again?",
    "Your conga went all wibbly!",
    "Clash goes the crab cymbal!",
    "I put the beat in beetroot!",
    "Your tail did a little escape!",
    "Boop first, questions later!",
    "That was shell-arious!",
    "My victory dance has knees now!",
    "You charged my most bonkable side!",
    "Crabs acquired. Hat still imaginary.",
    "I win! Somebody ring a coconut!",
    "Your rhythm needs more noodles!",
    "A tactical whoopsie for you!",
    "Snip snap, nice gap!",
];

const EMPTY_TRAIN: &[&str] = &[
    "You charged without a conga?",
    "No tail to lose, just pride.",
    "Bring followers next time!",
    "That was a solo, not a clash.",
    "Your invisible conga fled first.",
    "A one-crab parade? Adorable.",
];

const LAST_LINK: &[&str] = &[
    "One crab left. Guard it well!",
    "Your conga is nearly a solo!",
    "I can count your crew on one claw!",
    "That tail is looking very short!",
    "One more clash ought to do it!",
    "Your last follower looks nervous!",
];

/// Sample a quip matching both the rival's generated name and the result of the clash.
pub(crate) fn clash_taunt(
    name: &str,
    crabs_lost: usize,
    remaining_crabs: usize,
    rng: &mut impl rand::Rng,
) -> &'static str {
    let pool = if crabs_lost == 0 {
        EMPTY_TRAIN
    } else if remaining_crabs <= 1 {
        LAST_LINK
    } else {
        match personality_for(name) {
            RivalPersonality::Plain => PLAIN,
            RivalPersonality::Pirate => PIRATE,
            RivalPersonality::Pompous => POMPOUS,
            RivalPersonality::Raver => RAVER,
            RivalPersonality::Serious => SERIOUS,
            RivalPersonality::Silly => SILLY,
        }
    };
    pool.choose(rng).copied().unwrap_or("Mind the beat!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_name_styles_map_to_distinct_personalities() {
        assert_eq!(personality_for("Kevin"), RivalPersonality::Plain);
        assert_eq!(personality_for("Cap'n Clawbeard"), RivalPersonality::Pirate);
        assert_eq!(
            personality_for("Gravelord Brinewraith"),
            RivalPersonality::Pompous
        );
        assert_eq!(personality_for("DJ Bassline"), RivalPersonality::Raver);
        assert_eq!(
            personality_for("Admiral Ironpincer"),
            RivalPersonality::Serious
        );
        assert_eq!(personality_for("Uncle Snippy"), RivalPersonality::Silly);
    }

    #[test]
    fn clash_result_overrides_personality_when_the_train_is_empty_or_nearly_empty() {
        let mut rng = crate::rng::rng();
        let empty = clash_taunt("DJ Bassline", 0, 0, &mut rng);
        let last = clash_taunt("DJ Bassline", 2, 1, &mut rng);
        assert!(EMPTY_TRAIN.contains(&empty));
        assert!(LAST_LINK.contains(&last));
    }
}
