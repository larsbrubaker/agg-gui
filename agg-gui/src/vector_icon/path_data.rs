//! SVG path-data parser for [`VectorIcon`](super::VectorIcon).
//!
//! Deliberately a *subset*: `M`, `L`, `H`, `V`, `C`, `A`, `Z` in both
//! absolute and relative form. That is everything hand-authored icon
//! artwork uses, and it lets a host paste an SVG's `d` attribute in
//! verbatim rather than re-deriving the geometry by hand — the icon in
//! the app then stays byte-comparable with the artwork it came from.
//! Full SVG (gradients, transforms, text, `S`/`T`/`Q` shorthands) is the
//! job of [`crate::svg`], which brings in `usvg`; this module exists so
//! a 200-byte icon does not need a document parser.
//!
//! Output is already-flattened closed contours in icon space, because
//! everything downstream (painting, hit-free bounds queries, tests)
//! wants points rather than curves, and the flattening tolerance only
//! has to satisfy a ~16-px icon.
//!
//! ## One thing the subset does NOT accept: compressed arc flags
//!
//! SVG lets an arc's two boolean flags run together with the numbers
//! around them (`a1 1 0 0110 10` means `0 1 10 10`), because a flag is
//! by definition one character. This scanner reads every argument as a
//! full number, so it sees `0110` instead. That is a deliberate
//! omission — the artwork this module exists for is written with
//! separators, and the alternative is a scanner whose number parsing
//! depends on which argument position it is in.
//!
//! It fails *loudly*, which is why the omission is safe: swallowing
//! `0110` as one number leaves the arc one argument short, so the path
//! is rejected with [`IconPathError::MissingArgument`] rather than
//! drawn wrong (pinned by `compressed_arc_flags_are_rejected_not_misdrawn`
//! in this module's tests). A path that got past that check would still
//! land on the stated endpoint — the arc's own bulge is the only thing
//! flags control.
//!
//! Curves are flattened at a fixed angular / parametric step chosen for
//! that size: arcs are cut at 7.5° (chord error `r·(1−cos 3.75°)`, about
//! 0.05 units on a 64-unit box — a thousandth of the icon) and cubics
//! into 16 pieces. Icon artwork is commonly *already* chopped into short
//! arc segments so that naive flatteners look right; this step is fine
//! enough that such artwork is unaffected either way.

use std::f64::consts::PI;
use std::fmt;

/// One closed contour: consecutive points in icon space.
pub type Contour = Vec<[f64; 2]>;

/// Angular step used to flatten arcs.
const ARC_STEP: f64 = PI / 24.0;
/// Number of line segments per cubic Bézier.
const CUBIC_STEPS: usize = 16;

/// Why a path-data string could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconPathError {
    /// A command letter this subset does not implement.
    UnsupportedCommand(char),
    /// A command ran out of numbers (e.g. `L 10`).
    MissingArgument(char),
    /// A token where a number was expected.
    BadNumber(String),
    /// Coordinates appeared before any `M`.
    NoCurrentPoint,
    /// A number appeared where no command could consume it — the only
    /// way to reach this is a number right after `Z`, which takes no
    /// arguments, so there is nothing for SVG's implicit-repetition rule
    /// to repeat.
    UnexpectedNumber,
}

impl fmt::Display for IconPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IconPathError::UnsupportedCommand(c) => {
                write!(f, "unsupported SVG path command '{c}'")
            }
            IconPathError::MissingArgument(c) => {
                write!(f, "SVG path command '{c}' is missing arguments")
            }
            IconPathError::BadNumber(s) => write!(f, "'{s}' is not a number"),
            IconPathError::NoCurrentPoint => {
                write!(
                    f,
                    "SVG path starts with a drawing command before any moveto"
                )
            }
            IconPathError::UnexpectedNumber => {
                write!(
                    f,
                    "SVG path has a number after a closepath, which takes no arguments"
                )
            }
        }
    }
}

impl std::error::Error for IconPathError {}

/// Parse SVG path data into closed contours.
///
/// A subpath is closed whether or not it ends in `Z`: icon paths are
/// fills, and an unclosed fill contour is closed implicitly by every
/// rasteriser anyway.
pub fn parse_path(d: &str) -> Result<Vec<Contour>, IconPathError> {
    let mut scan = Scanner::new(d);
    let mut contours: Vec<Contour> = Vec::new();
    let mut current: Contour = Vec::new();
    let mut pos = [0.0f64, 0.0];
    let mut start = [0.0f64, 0.0];
    let mut cmd: Option<char> = None;

    loop {
        scan.skip_separators();
        let next = match scan.peek() {
            None => break,
            Some(c) if c.is_ascii_alphabetic() => {
                scan.bump();
                cmd = Some(c);
                c
            }
            // A bare number repeats the previous command (SVG's implicit
            // repetition; an implicit repeat of `M` is a `L`).
            Some(_) => match cmd {
                Some('M') => 'L',
                Some('m') => 'l',
                // `Z` takes no arguments, so there is nothing to repeat
                // — and repeating it would consume no input, spinning
                // the loop forever. The spec calls this invalid data.
                Some('Z') | Some('z') => return Err(IconPathError::UnexpectedNumber),
                Some(c) => c,
                None => return Err(IconPathError::NoCurrentPoint),
            },
        };

        match next {
            'M' | 'm' => {
                if current.len() > 1 {
                    contours.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                let x = scan.number(next)?;
                let y = scan.number(next)?;
                pos = if next == 'M' {
                    [x, y]
                } else {
                    [pos[0] + x, pos[1] + y]
                };
                start = pos;
                current.push(pos);
            }
            'L' | 'l' => {
                let x = scan.number(next)?;
                let y = scan.number(next)?;
                pos = if next == 'L' {
                    [x, y]
                } else {
                    [pos[0] + x, pos[1] + y]
                };
                push_point(&mut current, pos)?;
            }
            'H' | 'h' => {
                let x = scan.number(next)?;
                pos = [if next == 'H' { x } else { pos[0] + x }, pos[1]];
                push_point(&mut current, pos)?;
            }
            'V' | 'v' => {
                let y = scan.number(next)?;
                pos = [pos[0], if next == 'V' { y } else { pos[1] + y }];
                push_point(&mut current, pos)?;
            }
            'C' | 'c' => {
                let mut p = [[0.0f64; 2]; 3];
                for slot in p.iter_mut() {
                    let x = scan.number(next)?;
                    let y = scan.number(next)?;
                    *slot = if next == 'C' {
                        [x, y]
                    } else {
                        [pos[0] + x, pos[1] + y]
                    };
                }
                if current.is_empty() {
                    return Err(IconPathError::NoCurrentPoint);
                }
                flatten_cubic(&mut current, pos, p[0], p[1], p[2]);
                pos = p[2];
            }
            'A' | 'a' => {
                let rx = scan.number(next)?;
                let ry = scan.number(next)?;
                let rot = scan.number(next)?;
                let large = scan.number(next)? != 0.0;
                let sweep = scan.number(next)? != 0.0;
                let x = scan.number(next)?;
                let y = scan.number(next)?;
                let end = if next == 'A' {
                    [x, y]
                } else {
                    [pos[0] + x, pos[1] + y]
                };
                if current.is_empty() {
                    return Err(IconPathError::NoCurrentPoint);
                }
                flatten_arc(&mut current, pos, end, rx, ry, rot, large, sweep);
                pos = end;
            }
            'Z' | 'z' => {
                if current.len() > 1 {
                    contours.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                pos = start;
                current.push(pos);
            }
            other => return Err(IconPathError::UnsupportedCommand(other)),
        }
    }

    if current.len() > 1 {
        contours.push(current);
    }
    Ok(contours)
}

fn push_point(current: &mut Contour, p: [f64; 2]) -> Result<(), IconPathError> {
    if current.is_empty() {
        return Err(IconPathError::NoCurrentPoint);
    }
    current.push(p);
    Ok(())
}

fn flatten_cubic(out: &mut Contour, p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2]) {
    for i in 1..=CUBIC_STEPS {
        let t = i as f64 / CUBIC_STEPS as f64;
        let u = 1.0 - t;
        let x = u * u * u * p0[0]
            + 3.0 * u * u * t * p1[0]
            + 3.0 * u * t * t * p2[0]
            + t * t * t * p3[0];
        let y = u * u * u * p0[1]
            + 3.0 * u * u * t * p1[1]
            + 3.0 * u * t * t * p2[1]
            + t * t * t * p3[1];
        out.push([x, y]);
    }
}

/// Endpoint → centre arc conversion (SVG spec F.6.5), then flattening.
///
/// Degenerate cases follow the spec's out-of-range radius handling: a
/// zero radius draws a straight line, and radii too small to span the
/// endpoints are scaled up until they just fit.
#[allow(clippy::too_many_arguments)]
fn flatten_arc(
    out: &mut Contour,
    from: [f64; 2],
    to: [f64; 2],
    rx: f64,
    ry: f64,
    x_rot_deg: f64,
    large: bool,
    sweep: bool,
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    // Identical endpoints: the spec omits the segment entirely. Pushing
    // `to` here would duplicate the current point — harmless to fill,
    // but it corrupts point counts and bounds queries.
    if (from[0] - to[0]).abs() + (from[1] - to[1]).abs() < 1e-12 {
        return;
    }
    // A zero radius degenerates to a straight line (spec F.6.2).
    if rx < 1e-12 || ry < 1e-12 {
        out.push(to);
        return;
    }
    let phi = x_rot_deg.to_radians();
    let (cos_p, sin_p) = (phi.cos(), phi.sin());

    let dx2 = (from[0] - to[0]) * 0.5;
    let dy2 = (from[1] - to[1]) * 0.5;
    let x1p = cos_p * dx2 + sin_p * dy2;
    let y1p = -sin_p * dx2 + cos_p * dy2;

    // Scale radii up when they are too small to reach (spec F.6.6).
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }

    let num = (rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p).max(0.0);
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let coef = if den <= 0.0 {
        0.0
    } else {
        let c = (num / den).sqrt();
        if large == sweep {
            -c
        } else {
            c
        }
    };
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cos_p * cxp - sin_p * cyp + (from[0] + to[0]) * 0.5;
    let cy = sin_p * cxp + cos_p * cyp + (from[1] + to[1]) * 0.5;

    let theta = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut delta = angle(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && delta > 0.0 {
        delta -= 2.0 * PI;
    } else if sweep && delta < 0.0 {
        delta += 2.0 * PI;
    }

    let steps = ((delta.abs() / ARC_STEP).ceil() as usize).max(1);
    for i in 1..=steps {
        let t = theta + delta * (i as f64 / steps as f64);
        let (ct, st) = (t.cos(), t.sin());
        out.push([
            cx + cos_p * rx * ct - sin_p * ry * st,
            cy + sin_p * rx * ct + cos_p * ry * st,
        ]);
    }
    // Land exactly on the stated endpoint: the trigonometry above is
    // only as accurate as the artwork's rounded coordinates, and a
    // sub-unit gap here would open a seam between adjacent segments.
    if let Some(last) = out.last_mut() {
        *last = to;
    }
}

/// Signed angle from vector `u` to vector `v`.
fn angle(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let dot = ux * vx + uy * vy;
    let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
    if len <= 0.0 {
        return 0.0;
    }
    let mut a = (dot / len).clamp(-1.0, 1.0).acos();
    if ux * vy - uy * vx < 0.0 {
        a = -a;
    }
    a
}

/// Character scanner over path data: whitespace and commas separate
/// tokens, and a `-` or `+` may start a number with no separator before
/// it (`M0 0H52V22.404`).
struct Scanner<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl<'a> Scanner<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            i: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.bytes.get(self.i).map(|b| *b as char)
    }

    fn bump(&mut self) {
        self.i += 1;
    }

    fn skip_separators(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() || c == ',' {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// Read one number, or report which command ran dry.
    fn number(&mut self, cmd: char) -> Result<f64, IconPathError> {
        self.skip_separators();
        let start = self.i;
        if matches!(self.peek(), Some('+') | Some('-')) {
            self.bump();
        }
        let mut seen_digit = false;
        let mut seen_dot = false;
        while let Some(c) = self.peek() {
            match c {
                '0'..='9' => {
                    seen_digit = true;
                    self.bump();
                }
                '.' if !seen_dot => {
                    seen_dot = true;
                    self.bump();
                }
                'e' | 'E' if seen_digit => {
                    self.bump();
                    if matches!(self.peek(), Some('+') | Some('-')) {
                        self.bump();
                    }
                }
                _ => break,
            }
        }
        if !seen_digit {
            self.i = start;
            return Err(IconPathError::MissingArgument(cmd));
        }
        let text = std::str::from_utf8(&self.bytes[start..self.i])
            .map_err(|_| IconPathError::BadNumber(String::from("<non-utf8>")))?;
        text.parse::<f64>()
            .map_err(|_| IconPathError::BadNumber(text.to_string()))
    }
}
