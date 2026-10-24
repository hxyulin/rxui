//! SVG path data (the `d` attribute) parsed into absolute moves, lines, quadratic
//! and cubic Béziers. Arcs become cubics; horizontal, vertical and smooth commands
//! become their explicit forms.
use std::fmt;

/// One absolute path command in view box units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Segment {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad([f32; 2], [f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

/// Malformed icon data: SVG path syntax or an invalid view box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconError {
    offset: usize,
    reason: &'static str,
}
impl IconError {
    pub(crate) fn new(offset: usize, reason: &'static str) -> Self {
        Self { offset, reason }
    }
    /// Byte offset into the path data where parsing failed; zero for a view box
    /// error.
    pub fn offset(&self) -> usize {
        self.offset
    }
}
impl fmt::Display for IconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid icon at byte {}: {}", self.offset, self.reason)
    }
}
impl std::error::Error for IconError {}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}
impl Parser<'_> {
    fn skip_space(&mut self) {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }
    /// Skips whitespace with at most one comma, as between arguments.
    fn skip_separator(&mut self) {
        self.skip_space();
        if self.s.get(self.i) == Some(&b',') {
            self.i += 1;
            self.skip_space();
        }
    }
    fn error(&self, reason: &'static str) -> IconError {
        IconError::new(self.i, reason)
    }
    fn at_number(&self) -> bool {
        self.s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.'))
    }
    fn digits(&mut self) -> usize {
        let start = self.i;
        while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
            self.i += 1;
        }
        self.i - start
    }
    /// One number in the compact SVG grammar: `1.5.5` is two numbers, `-1-2` too.
    fn number(&mut self) -> Result<f32, IconError> {
        self.skip_separator();
        let start = self.i;
        if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
            self.i += 1;
        }
        let mut mantissa = self.digits();
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            mantissa += self.digits();
        }
        if mantissa == 0 {
            self.i = start;
            return Err(self.error("expected a number"));
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            let mark = self.i;
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                self.i = mark;
                return Err(self.error("expected exponent digits"));
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).expect("ASCII number");
        match text.parse::<f32>() {
            Ok(v) if v.is_finite() => Ok(v),
            _ => {
                self.i = start;
                Err(self.error("number out of range"))
            }
        }
    }
    fn pair(&mut self) -> Result<[f32; 2], IconError> {
        Ok([self.number()?, self.number()?])
    }
    /// An arc flag: a single `0` or `1`, which needs no separator after it.
    fn flag(&mut self) -> Result<bool, IconError> {
        self.skip_separator();
        let flag = match self.s.get(self.i) {
            Some(b'0') => false,
            Some(b'1') => true,
            _ => return Err(self.error("expected an arc flag")),
        };
        self.i += 1;
        Ok(flag)
    }
}

/// Parses SVG path data into absolute segments. Every contour begins with a
/// [`Segment::Move`], including one continuing after a close.
pub(crate) fn parse(d: &str) -> Result<Vec<Segment>, IconError> {
    let mut p = Parser {
        s: d.as_bytes(),
        i: 0,
    };
    let mut out = Vec::new();
    let mut current = [0f32; 2];
    let mut start = [0f32; 2];
    // Second cubic and quadratic control points, for smooth-curve reflection.
    let mut last_cubic = None;
    let mut last_quad = None;
    // A drawing command right after a close continues from the contour's start.
    let mut closed = false;
    p.skip_space();
    while p.i < p.s.len() {
        let command = p.s[p.i];
        if !b"MmLlHhVvCcSsQqTtAaZz".contains(&command) {
            return Err(p.error("expected a path command"));
        }
        if out.is_empty() && !matches!(command, b'M' | b'm') {
            return Err(p.error("path data must start with a move"));
        }
        p.i += 1;
        let relative = command.is_ascii_lowercase();
        let upper = command.to_ascii_uppercase();
        if upper == b'Z' {
            out.push(Segment::Close);
            current = start;
            closed = true;
            (last_cubic, last_quad) = (None, None);
            p.skip_space();
            continue;
        }
        let mut first = true;
        loop {
            let base = if relative { current } else { [0., 0.] };
            let abs = |v: [f32; 2]| [base[0] + v[0], base[1] + v[1]];
            if closed && upper != b'M' {
                out.push(Segment::Move(start));
            }
            closed = false;
            let (mut cubic, mut quad) = (None, None);
            match upper {
                // Pairs after a move's first are implicit lines.
                b'M' if first => {
                    current = abs(p.pair()?);
                    start = current;
                    out.push(Segment::Move(current));
                }
                b'M' | b'L' => {
                    current = abs(p.pair()?);
                    out.push(Segment::Line(current));
                }
                b'H' => {
                    current[0] = p.number()? + base[0];
                    out.push(Segment::Line(current));
                }
                b'V' => {
                    current[1] = p.number()? + base[1];
                    out.push(Segment::Line(current));
                }
                b'C' => {
                    let (c1, c2, end) = (abs(p.pair()?), abs(p.pair()?), abs(p.pair()?));
                    out.push(Segment::Cubic(c1, c2, end));
                    (current, cubic) = (end, Some(c2));
                }
                b'S' => {
                    let c1 = reflect(last_cubic, current);
                    let (c2, end) = (abs(p.pair()?), abs(p.pair()?));
                    out.push(Segment::Cubic(c1, c2, end));
                    (current, cubic) = (end, Some(c2));
                }
                b'Q' => {
                    let (c, end) = (abs(p.pair()?), abs(p.pair()?));
                    out.push(Segment::Quad(c, end));
                    (current, quad) = (end, Some(c));
                }
                b'T' => {
                    let c = reflect(last_quad, current);
                    let end = abs(p.pair()?);
                    out.push(Segment::Quad(c, end));
                    (current, quad) = (end, Some(c));
                }
                b'A' => {
                    let radii = [p.number()?, p.number()?];
                    let rotation = p.number()?;
                    let (large, sweep) = (p.flag()?, p.flag()?);
                    let end = abs(p.pair()?);
                    arc(&mut out, current, radii, rotation, large, sweep, end);
                    current = end;
                }
                _ => unreachable!("command letters are checked above"),
            }
            (last_cubic, last_quad) = (cubic, quad);
            first = false;
            p.skip_separator();
            if !p.at_number() {
                break;
            }
        }
    }
    Ok(out)
}

/// The reflection of `control` about `point`, or `point` without a previous curve.
fn reflect(control: Option<[f32; 2]>, point: [f32; 2]) -> [f32; 2] {
    control.map_or(point, |c| [2. * point[0] - c[0], 2. * point[1] - c[1]])
}

/// Appends an SVG elliptical arc as cubic Béziers of at most a quarter turn each,
/// following the SVG implementation notes: endpoint to center parameterization
/// with out-of-range radii scaled up.
fn arc(
    out: &mut Vec<Segment>,
    from: [f32; 2],
    radii: [f32; 2],
    rotation: f32,
    large: bool,
    sweep: bool,
    to: [f32; 2],
) {
    use std::f64::consts::{PI, TAU};
    if from == to {
        return;
    }
    let (mut rx, mut ry) = (f64::from(radii[0]).abs(), f64::from(radii[1]).abs());
    if rx == 0. || ry == 0. {
        out.push(Segment::Line(to));
        return;
    }
    let (sin, cos) = f64::from(rotation).to_radians().sin_cos();
    let [x1, y1] = from.map(f64::from);
    let [x2, y2] = to.map(f64::from);
    // Midpoint in the ellipse's unrotated frame.
    let (dx, dy) = ((x1 - x2) / 2., (y1 - y2) / 2.);
    let (px, py) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let lambda = (px / rx).powi(2) + (py / ry).powi(2);
    if lambda > 1. {
        (rx, ry) = (rx * lambda.sqrt(), ry * lambda.sqrt());
    }
    let numerator = (rx * ry).powi(2) - (rx * py).powi(2) - (ry * px).powi(2);
    let denominator = (rx * py).powi(2) + (ry * px).powi(2);
    let mut scale = (numerator / denominator).max(0.).sqrt();
    if large == sweep {
        scale = -scale;
    }
    let (cxp, cyp) = (scale * rx * py / ry, -scale * ry * px / rx);
    let cx = cos * cxp - sin * cyp + (x1 + x2) / 2.;
    let cy = sin * cxp + cos * cyp + (y1 + y2) / 2.;
    let angle = |ux: f64, uy: f64| uy.atan2(ux);
    let theta = angle((px - cxp) / rx, (py - cyp) / ry);
    let mut delta = angle((-px - cxp) / rx, (-py - cyp) / ry) - theta;
    if sweep && delta < 0. {
        delta += TAU;
    } else if !sweep && delta > 0. {
        delta -= TAU;
    }
    // Snap away float noise so a half turn is two segments, not three.
    let count = (delta.abs() / (PI / 2.) - 1e-9).ceil().max(1.);
    let step = delta / count;
    let k = 4. / 3. * (step / 4.).tan();
    let point = |t: f64| {
        let (s, c) = t.sin_cos();
        let (x, y) = (rx * c, ry * s);
        [cx + cos * x - sin * y, cy + sin * x + cos * y]
    };
    let tangent = |t: f64| {
        let (s, c) = t.sin_cos();
        let (x, y) = (-rx * s, ry * c);
        [cos * x - sin * y, sin * x + cos * y]
    };
    let narrow = |p: [f64; 2]| p.map(|v| v as f32);
    for i in 0..count as usize {
        let (t0, t1) = (theta + step * i as f64, theta + step * (i + 1) as f64);
        let (p0, p1) = (point(t0), point(t1));
        let (d0, d1) = (tangent(t0), tangent(t1));
        let c1 = [p0[0] + k * d0[0], p0[1] + k * d0[1]];
        let c2 = [p1[0] - k * d1[0], p1[1] - k * d1[1]];
        // The last segment ends exactly at the requested point.
        let end = if i + 1 == count as usize {
            to
        } else {
            narrow(p1)
        };
        out.push(Segment::Cubic(narrow(c1), narrow(c2), end));
    }
}

/// Axis-aligned extent of the drawn geometry as `[min_x, min_y, max_x, max_y]`,
/// sampling curves, or None without any segment.
#[cfg(test)]
pub(crate) fn bounds(segments: &[Segment]) -> Option<[f32; 4]> {
    let mut b: Option<[f32; 4]> = None;
    let mut add = |p: [f32; 2]| {
        let e = b.get_or_insert([p[0], p[1], p[0], p[1]]);
        *e = [
            e[0].min(p[0]),
            e[1].min(p[1]),
            e[2].max(p[0]),
            e[3].max(p[1]),
        ];
    };
    let mut current = [0.; 2];
    let lerp =
        |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    for segment in segments {
        match *segment {
            Segment::Move(p) => current = p,
            Segment::Line(p) => {
                add(current);
                add(p);
                current = p;
            }
            Segment::Quad(c, p) => {
                for i in 0..=16 {
                    let t = i as f32 / 16.;
                    add(lerp(lerp(current, c, t), lerp(c, p, t), t));
                }
                current = p;
            }
            Segment::Cubic(c1, c2, p) => {
                for i in 0..=16 {
                    let t = i as f32 / 16.;
                    let (a, m, z) = (lerp(current, c1, t), lerp(c1, c2, t), lerp(c2, p, t));
                    add(lerp(lerp(a, m, t), lerp(m, z, t), t));
                }
                current = p;
            }
            Segment::Close => {}
        }
    }
    b
}
