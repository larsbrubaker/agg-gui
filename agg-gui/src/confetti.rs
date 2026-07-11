//! Confetti particle overlay — a reusable win-celebration burst effect.
//!
//! This is a self-contained simulation + paint helper, not a [`Widget`]: it has
//! no layout, bounds, or event handling. It sits alongside [`crate::animation`]
//! (the `Tween` interpolator) and [`crate::timestep`] (the fixed-step scheduler)
//! as one of the animation/simulation primitives the framework offers to apps.
//!
//! A caller creates a [`ConfettiSystem`] with [`ConfettiSystem::burst`], then
//! each frame:
//!
//! 1. advances the physics with [`ConfettiSystem::tick`], passing the wall-clock
//!    delta since the previous frame in seconds, and
//! 2. draws it with [`ConfettiSystem::paint`].
//!
//! `tick` returns `false` once every particle has expired, so the caller can
//! drop the system and stop drawing. While particles are alive, `tick` calls
//! [`crate::animation::request_draw`] so the host keeps producing frames without
//! any user input — the same pattern [`crate::animation::Tween::tick`] uses.
//!
//! Time is taken entirely from the caller (`dt_seconds`); the module reads no
//! clock and pulls in no OS randomness, so it is `wasm32`-clean. Randomness
//! comes from a small deterministic splitmix64 PRNG seeded by the caller, which
//! makes a burst fully reproducible from its `seed`.
//!
//! All coordinates follow the crate-wide **Y-up, first-quadrant** convention:
//! positive Y is up, so gravity pulls particles toward smaller Y and the initial
//! upward spread uses positive vertical velocity.

use crate::animation::request_draw;
use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;

/// Downward gravitational acceleration in logical px/s².
const GRAVITY: f64 = 520.0;

/// Seconds of alpha fade at the tail end of a particle's life.
const FADE_SECS: f64 = 0.8;

/// Deterministic splitmix64 PRNG.
///
/// Kept private and tiny on purpose: the crate has no RNG dependency and the
/// confetti burst only needs a reproducible stream of uniform values seeded by
/// the caller. splitmix64 has excellent statistical quality for a single 64-bit
/// state word and is trivially portable across targets (identical output on
/// native and `wasm32`).
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform `f64` in `[0.0, 1.0)`.
    fn next_unit(&mut self) -> f64 {
        // Top 53 bits → exact double in [0, 1).
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform `f64` in `[lo, hi)`.
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_unit()
    }
}

/// A single confetti flake: a small rotated rectangle drifting under gravity.
struct Particle {
    /// Position of the flake's center, Y-up.
    x: f64,
    y: f64,
    /// Linear velocity in logical px/s.
    vx: f64,
    vy: f64,
    /// Rectangle dimensions in logical px.
    w: f64,
    h: f64,
    /// Orientation and spin.
    rot: f64,
    ang_vel: f64,
    /// Sinusoidal horizontal flutter.
    sway_amp: f64,
    sway_freq: f64,
    sway_phase: f64,
    color: Color,
    /// Elapsed and total life in seconds.
    age: f64,
    lifetime: f64,
}

impl Particle {
    /// Fade factor in `[0, 1]`: `1.0` for most of the life, ramping to `0.0` at
    /// `age == lifetime` so the flake vanishes smoothly instead of popping.
    fn alpha_factor(&self) -> f32 {
        let remaining = self.lifetime - self.age;
        if remaining >= FADE_SECS {
            1.0
        } else {
            (remaining / FADE_SECS).clamp(0.0, 1.0) as f32
        }
    }

    /// Advance one step. `dt` is already sanitized (finite, non-negative).
    fn integrate(&mut self, dt: f64) {
        self.age += dt;
        self.vy -= GRAVITY * dt;
        let sway = self.sway_amp * (self.sway_freq * self.age + self.sway_phase).sin();
        self.x += (self.vx + sway) * dt;
        self.y += self.vy * dt;
        self.rot += self.ang_vel * dt;
    }
}

/// An owned collection of confetti particles that animate together.
///
/// Create with [`ConfettiSystem::burst`], drive per frame with
/// [`ConfettiSystem::tick`] + [`ConfettiSystem::paint`]. The system owns all
/// particle state; there is no shared or global mutable state.
pub struct ConfettiSystem {
    particles: Vec<Particle>,
}

impl ConfettiSystem {
    /// Spawn a burst of `count` flakes across the top region of `bounds`.
    ///
    /// Flakes start near the top edge with a slight upward/outward velocity
    /// spread, then fall under gravity while fluttering horizontally. Colors are
    /// drawn from `palette` (cycled deterministically); an empty palette yields
    /// white flakes. `seed` fully determines the burst — the same seed produces
    /// the same particle stream on every platform.
    ///
    /// Coordinates are Y-up: the "top region" is the high-Y band of `bounds`.
    pub fn burst(bounds: Rect, count: usize, palette: &[Color], seed: u64) -> Self {
        let mut rng = SplitMix64::new(seed);
        let mut particles = Vec::with_capacity(count);

        // Spawn band: the top ~15% of the bounds height (at least a few px).
        let band = (bounds.height * 0.15).max(4.0);
        let top = bounds.top();

        for i in 0..count {
            let color = if palette.is_empty() {
                Color::white()
            } else {
                palette[i % palette.len()]
            };
            let w = rng.range(4.0, 10.0);
            let h = w * rng.range(0.45, 0.9);
            particles.push(Particle {
                x: rng.range(bounds.left(), bounds.right()),
                y: rng.range(top - band, top),
                // Upward (+Y) launch with an outward (±X) horizontal spread.
                vx: rng.range(-140.0, 140.0),
                vy: rng.range(40.0, 190.0),
                w,
                h,
                rot: rng.range(0.0, std::f64::consts::TAU),
                ang_vel: rng.range(-7.0, 7.0),
                sway_amp: rng.range(18.0, 60.0),
                sway_freq: rng.range(2.0, 6.0),
                sway_phase: rng.range(0.0, std::f64::consts::TAU),
                color,
                age: 0.0,
                lifetime: rng.range(2.5, 4.0),
            });
        }

        Self { particles }
    }

    /// Number of live particles remaining.
    pub fn particle_count(&self) -> usize {
        self.particles.len()
    }

    /// Whether any particle is still alive.
    pub fn is_active(&self) -> bool {
        !self.particles.is_empty()
    }

    /// Advance the simulation by `dt_seconds` and return whether the burst is
    /// still alive (any particle remaining).
    ///
    /// Expired particles are removed. A non-finite or negative `dt` is treated
    /// as `0.0`, so a hostile or degenerate frame delta can never introduce NaN
    /// into particle positions. While particles remain, this calls
    /// [`request_draw`] so the host keeps drawing frames until the burst
    /// finishes on its own.
    pub fn tick(&mut self, dt_seconds: f64) -> bool {
        let dt = if dt_seconds.is_finite() {
            dt_seconds.max(0.0)
        } else {
            0.0
        };

        for p in &mut self.particles {
            p.integrate(dt);
        }
        self.particles.retain(|p| p.age < p.lifetime);

        let alive = !self.particles.is_empty();
        if alive {
            request_draw();
        }
        alive
    }

    /// Paint every live particle as a rotated, alpha-faded filled rectangle.
    ///
    /// Drawn in the current coordinate space (Y-up); callers position the burst
    /// by setting up `ctx`'s transform before calling, or by passing bounds in
    /// the same space they paint in. A few hundred particles paint comfortably
    /// with the standard `DrawCtx` fill path.
    pub fn paint(&self, ctx: &mut dyn DrawCtx) {
        for p in &self.particles {
            let a = p.alpha_factor();
            if a <= 0.0 {
                continue;
            }
            ctx.set_fill_color(p.color.with_alpha(p.color.a * a));
            ctx.save();
            ctx.translate(p.x, p.y);
            ctx.rotate(p.rot);
            ctx.begin_path();
            ctx.rect(-p.w * 0.5, -p.h * 0.5, p.w, p.h);
            ctx.fill();
            ctx.restore();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Vec<Color> {
        vec![
            Color::from_rgb8(0xE8, 0x3A, 0x3A),
            Color::from_rgb8(0x3A, 0xC8, 0x5A),
            Color::from_rgb8(0x3A, 0x6A, 0xE8),
            Color::from_rgb8(0xF2, 0xC4, 0x2A),
        ]
    }

    fn bounds() -> Rect {
        Rect::new(0.0, 0.0, 800.0, 600.0)
    }

    /// Same seed → byte-identical first-frame particle state.
    #[test]
    fn deterministic_from_seed() {
        let a = ConfettiSystem::burst(bounds(), 200, &palette(), 0xDEAD_BEEF);
        let b = ConfettiSystem::burst(bounds(), 200, &palette(), 0xDEAD_BEEF);

        assert_eq!(a.particle_count(), b.particle_count());
        for (pa, pb) in a.particles.iter().zip(b.particles.iter()) {
            assert_eq!(pa.x.to_bits(), pb.x.to_bits());
            assert_eq!(pa.y.to_bits(), pb.y.to_bits());
            assert_eq!(pa.vx.to_bits(), pb.vx.to_bits());
            assert_eq!(pa.vy.to_bits(), pb.vy.to_bits());
            assert_eq!(pa.rot.to_bits(), pb.rot.to_bits());
            assert_eq!(pa.lifetime.to_bits(), pb.lifetime.to_bits());
        }
    }

    /// Different seeds → different bursts (guards against a constant stream).
    #[test]
    fn distinct_seeds_differ() {
        let a = ConfettiSystem::burst(bounds(), 64, &palette(), 1);
        let b = ConfettiSystem::burst(bounds(), 64, &palette(), 2);
        let any_diff = a
            .particles
            .iter()
            .zip(b.particles.iter())
            .any(|(pa, pb)| pa.x.to_bits() != pb.x.to_bits());
        assert!(any_diff, "distinct seeds produced identical bursts");
    }

    /// Requested particle count is honored, and spawns land inside bounds.
    #[test]
    fn count_and_spawn_region() {
        let b = bounds();
        let sys = ConfettiSystem::burst(b, 300, &palette(), 7);
        assert_eq!(sys.particle_count(), 300);

        let band = (b.height * 0.15).max(4.0);
        for p in &sys.particles {
            assert!(
                p.x >= b.left() && p.x <= b.right(),
                "x {} out of bounds",
                p.x
            );
            // Spawn band is the top slice of the bounds.
            assert!(
                p.y >= b.top() - band && p.y <= b.top(),
                "y {} outside spawn band",
                p.y
            );
            assert!(p.x.is_finite() && p.y.is_finite());
        }
    }

    /// Empty palette falls back to white rather than panicking on modulo.
    #[test]
    fn empty_palette_is_white() {
        let sys = ConfettiSystem::burst(bounds(), 10, &[], 3);
        assert_eq!(sys.particle_count(), 10);
        for p in &sys.particles {
            assert_eq!(p.color, Color::white());
        }
    }

    /// Ticking advances particles and eventually reports finished. Alpha
    /// reaches 0 at end of life, and positions never go NaN across normal,
    /// zero, and huge dt steps.
    #[test]
    fn ticks_to_completion_without_nan() {
        let mut sys = ConfettiSystem::burst(bounds(), 128, &palette(), 42);
        assert!(sys.is_active());

        // Zero dt: still alive, no state corruption.
        assert!(sys.tick(0.0));
        for p in &sys.particles {
            assert!(p.x.is_finite() && p.y.is_finite());
        }

        // Normal frames advance the sim without producing NaN.
        for _ in 0..30 {
            sys.tick(1.0 / 60.0);
            for p in &sys.particles {
                assert!(
                    p.x.is_finite() && p.y.is_finite(),
                    "NaN/inf during normal ticks"
                );
                assert!(p.alpha_factor() >= 0.0 && p.alpha_factor() <= 1.0);
            }
        }

        // Non-finite / negative dt is treated as zero — no NaN leaks in.
        assert!(sys.tick(f64::NAN));
        assert!(sys.tick(-5.0));
        for p in &sys.particles {
            assert!(p.x.is_finite() && p.y.is_finite());
        }

        // A single huge step exhausts every lifetime → finished, no infinite life.
        assert!(!sys.tick(100.0));
        assert_eq!(sys.particle_count(), 0);
        assert!(!sys.is_active());
    }

    /// Alpha fades to exactly 0 as a particle reaches the end of its life.
    #[test]
    fn alpha_reaches_zero_at_end_of_life() {
        let mut sys = ConfettiSystem::burst(bounds(), 1, &palette(), 99);
        let lifetime = sys.particles[0].lifetime;

        // Early on, fully opaque.
        assert_eq!(sys.particles[0].alpha_factor(), 1.0);

        // Step to just shy of the end so the particle is still present but
        // deep into its fade window.
        sys.tick(lifetime - 0.001);
        assert!(sys.is_active());
        let a = sys.particles[0].alpha_factor();
        assert!(
            (0.0..0.01).contains(&a),
            "expected near-zero alpha, got {a}"
        );
    }

    /// `paint` executes without panicking on a small software context.
    #[test]
    fn paint_smoke() {
        use crate::framebuffer::Framebuffer;
        use crate::gfx_ctx::GfxCtx;

        let mut sys = ConfettiSystem::burst(Rect::new(0.0, 0.0, 64.0, 48.0), 128, &palette(), 5);
        sys.tick(0.1);

        let mut fb = Framebuffer::new(64, 48);
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        sys.paint(&mut ctx);
    }
}
