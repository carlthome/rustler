//! Deterministic RNG plumbing for the headless bot playtests.
//!
//! The bot scenarios assert on *emergent* outcomes ("a revenge steal happened", "the tutorial
//! completed"). For those asserts to be a stable pass/fail rather than a coin-flip, every bot run
//! has to be reproducible — the same sequence of frames producing the same sequence of random
//! draws. Two things break that: a wall-clock timestep (fixed elsewhere, see
//! `MainState::frame_dt`) and an entropy-seeded RNG (fixed here).
//!
//! Real gameplay is untouched: when the process is NOT a seeded bot run, [`rng`] falls straight
//! through to `rand::rng()` (the per-thread entropy RNG), so interactive play stays as random as
//! ever. In bot mode `main` calls [`seed`] once at startup and every `crate::rng::rng()` call
//! then draws from one deterministic `SmallRng` stream instead.
//!
//! Usage is a drop-in swap: replace `rand::rng()` with `crate::rng::rng()`. The returned
//! [`GameRng`] is a zero-sized proxy that implements `rand::RngCore` (and therefore `Rng` and the
//! `SliceRandom` helpers via rand's blanket impls), so every `.random_range(..)`, `.random()`,
//! `.choose(..)`, `.shuffle(..)` call site keeps working verbatim.

use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use std::cell::RefCell;

thread_local! {
    /// `Some` once [`seed`] has run (bot mode): all draws come from this deterministic stream.
    /// `None` in normal play: draws fall through to `rand::rng()`.
    static GAME_RNG: RefCell<Option<StdRng>> = const { RefCell::new(None) };
}

/// Install a deterministic RNG stream for this (single) thread. Call once, before any gameplay
/// RNG is drawn, in bot/headless mode only. Idempotent — re-seeding just restarts the stream.
/// `StdRng` (rand's default, ChaCha-based) is used so no extra cargo feature is needed.
pub fn seed(seed: u64) {
    GAME_RNG.with(|cell| *cell.borrow_mut() = Some(StdRng::seed_from_u64(seed)));
}

/// A zero-sized handle to the process RNG. In a seeded bot run it delegates to the deterministic
/// `SmallRng`; otherwise it delegates to `rand::rng()`. Because it implements `RngCore`, it is a
/// drop-in replacement for the `rand::rng()` handle everywhere in the codebase.
pub struct GameRng;

/// The gameplay RNG handle. Drop-in replacement for `rand::rng()`.
#[inline]
pub fn rng() -> GameRng {
    GameRng
}

impl RngCore for GameRng {
    #[inline]
    fn next_u32(&mut self) -> u32 {
        GAME_RNG.with(|cell| match cell.borrow_mut().as_mut() {
            Some(r) => r.next_u32(),
            None => rand::rng().next_u32(),
        })
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        GAME_RNG.with(|cell| match cell.borrow_mut().as_mut() {
            Some(r) => r.next_u64(),
            None => rand::rng().next_u64(),
        })
    }

    #[inline]
    fn fill_bytes(&mut self, dst: &mut [u8]) {
        GAME_RNG.with(|cell| match cell.borrow_mut().as_mut() {
            Some(r) => r.fill_bytes(dst),
            None => rand::rng().fill_bytes(dst),
        })
    }
}
