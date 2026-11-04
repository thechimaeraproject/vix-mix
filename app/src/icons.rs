//! The prototype's icons, drawn as it draws them: stroked SVG paths in a 24-unit box with a
//! 1.9-unit line and round ends (`index.html`, `const I`). egui draws no SVG, so each icon
//! keeps the prototype's path data verbatim and [`paint`] follows it with egui's painter: the
//! subset of path syntax the icons use (`M L H V A C Z`, absolute and relative), plus the
//! `rect` and `circle` elements. Arcs and curves become short polylines.

use std::f32::consts::{PI, TAU};

use egui::epaint::PathShape;
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2};

/// The side of an icon's box, in SVG units.
const BOX: f32 = 24.0;
/// The prototype's `stroke-width`.
const LINE: f32 = 1.9;
/// The mute slash's `stroke-width`.
const SLASH_LINE: f32 = 2.4;

/// One element of an icon.
#[derive(Clone, Copy, Debug)]
pub enum Part {
    /// An SVG path's `d`.
    Path(&'static str),
    /// `<rect x y width height rx>`.
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rx: f32,
    },
    /// `<circle cx cy r>`.
    Circle { cx: f32, cy: f32, r: f32 },
}

/// An icon: its parts and its line width.
#[derive(Clone, Copy, Debug)]
pub struct Icon {
    pub parts: &'static [Part],
    pub line: f32,
}

const fn icon(parts: &'static [Part]) -> Icon {
    Icon { parts, line: LINE }
}

const fn rect(x: f32, y: f32, w: f32, h: f32, rx: f32) -> Part {
    Part::Rect { x, y, w, h, rx }
}

/// Headphones: Monitor's header, a channel's Monitor mute.
pub const PHONES: Icon = icon(&[
    Part::Path("M4 15v-3a8 8 0 0 1 16 0v3"),
    rect(3.0, 14.0, 4.5, 6.5, 1.0),
    rect(16.5, 14.0, 4.5, 6.5, 1.0),
]);
/// Four bars: Master's header, a channel's Master mute.
pub const MASTER: Icon = icon(&[Part::Path("M5 4v16M10 8v12M15 12v8M20 6v14")]);
/// A speaker: a mix's mute.
pub const SPEAKER: Icon = icon(&[
    Part::Path("M4 9.5h3.5L12 5.5v13l-4.5-4H4z"),
    Part::Path("M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11"),
]);
/// An ear: Master's preview button.
pub const EAR: Icon = icon(&[
    Part::Path("M7 9.5a5 5 0 0 1 10 0c0 3.2-3 4.2-3 7a2.8 2.8 0 0 1-5 1.6"),
    Part::Path("M10 9.5a2 2 0 0 1 4 0c0 1.2-1 1.6-1.5 2.4"),
]);
/// The slash over a pressed mute's icon.
pub const SLASH: Icon = Icon {
    parts: &[Part::Path("M4 4l16 16")],
    line: SLASH_LINE,
};
/// Two chain links: a channel's link toggle.
pub const LINK: Icon = icon(&[
    Part::Path("M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"),
    Part::Path("M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"),
]);
/// A microphone: a hardware channel's header.
pub const MIC: Icon = icon(&[
    rect(9.0, 3.0, 6.0, 11.0, 3.0),
    Part::Path("M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"),
]);
/// Four squares: an app channel's header.
pub const APPS: Icon = icon(&[
    rect(4.0, 4.0, 7.0, 7.0, 1.0),
    rect(13.0, 4.0, 7.0, 7.0, 1.0),
    rect(4.0, 13.0, 7.0, 7.0, 1.0),
    rect(13.0, 13.0, 7.0, 7.0, 1.0),
]);
/// An arrow to a bar: an output.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the mix panel's output rows (step 4)")
)]
pub const OUT: Icon = icon(&[Part::Path("M4 12h12M12 7l5 5-5 5M20 4v16")]);
/// A plus: "Add plugin", the add-channel column.
pub const PLUS: Icon = icon(&[Part::Path("M12 4v16M4 12h16")]);
/// A gear: an insert's settings, on hover.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "an insert's hover controls (step 3)")
)]
pub const GEAR: Icon = icon(&[
    Part::Circle {
        cx: 12.0,
        cy: 12.0,
        r: 3.2,
    },
    Part::Path(
        "M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1",
    ),
]);
/// A bin: remove an insert, on hover.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "an insert's hover controls (step 3)")
)]
pub const TRASH: Icon = icon(&[Part::Path("M4 7h16M9 7V4.5h6V7M6.5 7l1 13h9l1-13")]);
/// A caret: a strip header under the pointer.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "a strip header under the pointer (step 3)")
)]
pub const CARET: Icon = icon(&[Part::Path("M6 9l6 6 6-6")]);
/// A magnifier: the search fields.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the search fields (steps 4 and 5)")
)]
pub const SEARCH: Icon = icon(&[
    Part::Circle {
        cx: 11.0,
        cy: 11.0,
        r: 6.5,
    },
    Part::Path("M16 16l4.5 4.5"),
]);
/// A star: a favourite in the browser.
#[cfg_attr(not(test), expect(dead_code, reason = "the plugin browser (step 5)"))]
pub const STAR: Icon = icon(&[Part::Path(
    "M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.8L12 16.8l-5.2 2.8 1-5.8-4.3-4.1 5.9-.8z",
)]);
/// Three lines: the header's menu.
pub const MENU: Icon = icon(&[Part::Path("M4 7h16M4 12h16M4 17h16")]);

/// Every icon, by name, for the tests.
#[cfg(test)]
pub const ALL: [(&str, Icon); 16] = [
    ("PHONES", PHONES),
    ("MASTER", MASTER),
    ("SPEAKER", SPEAKER),
    ("EAR", EAR),
    ("SLASH", SLASH),
    ("LINK", LINK),
    ("MIC", MIC),
    ("APPS", APPS),
    ("OUT", OUT),
    ("PLUS", PLUS),
    ("GEAR", GEAR),
    ("TRASH", TRASH),
    ("CARET", CARET),
    ("SEARCH", SEARCH),
    ("STAR", STAR),
    ("MENU", MENU),
];

/// One stroked run of points in SVG units, and whether it closes on itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub points: Vec<Pos2>,
    pub closed: bool,
}

/// The icon's runs in SVG units. A path the parser cannot read draws nothing (and the test
/// over [`ALL`] says which).
pub fn runs(icon: &Icon) -> Vec<Run> {
    let mut out = Vec::new();
    for part in icon.parts {
        match *part {
            Part::Path(d) => out.extend(path(d).unwrap_or_default()),
            Part::Rect { x, y, w, h, rx } => out.push(rounded_rect(x, y, w, h, rx)),
            Part::Circle { cx, cy, r } => {
                let mut points = arc_points(pos2(cx, cy), r, 0.0, TAU);
                points.pop();
                out.push(Run {
                    points,
                    closed: true,
                });
            }
        }
    }
    out
}

/// Paints `icon` into `rect` (square; the icon's 24-unit box scaled to it) in `colour`.
pub fn paint(painter: &Painter, icon: &Icon, rect: Rect, colour: Color32) {
    let scale = rect.width() / BOX;
    let width = icon.line * scale;
    let stroke = Stroke::new(width, colour);
    let at = |p: Pos2| rect.min + p.to_vec2() * scale;
    for run in runs(icon) {
        let mut points: Vec<Pos2> = run.points.iter().map(|&p| at(p)).collect();
        // A rounded end whose straight side has no length repeats a point; the tessellator
        // wants distinct ones.
        points.dedup_by(|a, b| a.distance_sq(*b) < 1e-6);
        if run.closed && points.len() > 2 && points[0].distance_sq(points[points.len() - 1]) < 1e-6
        {
            points.pop();
        }
        if run.closed {
            painter.add(PathShape::closed_line(points, stroke));
        } else {
            // SVG's round line caps: a dot of the line's width at each open end.
            for end in [points.first(), points.last()].into_iter().flatten() {
                painter.circle_filled(*end, width / 2.0, colour);
            }
            painter.add(PathShape::line(points, stroke));
        }
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, rx: f32) -> Run {
    let r = rx.min(w / 2.0).min(h / 2.0);
    let mut points = Vec::new();
    // Clockwise from the top-right corner, a quarter arc at each corner.
    for (centre, start) in [
        (pos2(x + w - r, y + r), -PI / 2.0),
        (pos2(x + w - r, y + h - r), 0.0),
        (pos2(x + r, y + h - r), PI / 2.0),
        (pos2(x + r, y + r), PI),
    ] {
        points.extend(arc_points(centre, r, start, PI / 2.0));
    }
    Run {
        points,
        closed: true,
    }
}

/// Points along a circle's arc from `start` through `sweep` radians, both ends included.
fn arc_points(centre: Pos2, r: f32, start: f32, sweep: f32) -> Vec<Pos2> {
    let steps = ((sweep.abs() / (PI / 12.0)).ceil() as usize).max(1);
    (0..=steps)
        .map(|i| {
            let a = start + sweep * i as f32 / steps as f32;
            centre + vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

/// An SVG elliptical arc from `from` to `to` as points (from excluded), by the endpoint to
/// centre conversion of SVG's implementation notes; the icons' arcs are unrotated.
fn arc_to(from: Pos2, radii: Vec2, large: bool, sweep: bool, to: Pos2) -> Vec<Pos2> {
    let (mut rx, mut ry) = (radii.x.abs(), radii.y.abs());
    if rx == 0.0 || ry == 0.0 || from == to {
        return vec![to];
    }
    let half = (from - to) / 2.0;
    let lambda = (half.x / rx).powi(2) + (half.y / ry).powi(2);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let numerator = rx * rx * ry * ry - rx * rx * half.y * half.y - ry * ry * half.x * half.x;
    let denominator = rx * rx * half.y * half.y + ry * ry * half.x * half.x;
    let sign = if large == sweep { -1.0 } else { 1.0 };
    let coefficient = sign * (numerator / denominator).max(0.0).sqrt();
    let centre_prime = vec2(
        coefficient * rx * half.y / ry,
        -coefficient * ry * half.x / rx,
    );
    let centre = centre_prime + ((from.to_vec2() + to.to_vec2()) / 2.0);
    let u = vec2(
        (half.x - centre_prime.x) / rx,
        (half.y - centre_prime.y) / ry,
    );
    let v = vec2(
        (-half.x - centre_prime.x) / rx,
        (-half.y - centre_prime.y) / ry,
    );
    let start = u.y.atan2(u.x);
    let mut delta = v.y.atan2(v.x) - start;
    if sweep && delta < 0.0 {
        delta += TAU;
    } else if !sweep && delta > 0.0 {
        delta -= TAU;
    }
    let steps = ((delta.abs() / (PI / 12.0)).ceil() as usize).max(1);
    // The last step lands on `to` exactly, so the next segment starts where this one ends.
    (1..steps)
        .map(|i| {
            let a = start + delta * i as f32 / steps as f32;
            pos2(centre.x + rx * a.cos(), centre.y + ry * a.sin())
        })
        .chain([to])
        .collect()
}

fn cubic(from: Pos2, c1: Pos2, c2: Pos2, to: Pos2) -> Vec<Pos2> {
    const STEPS: usize = 10;
    (1..=STEPS)
        .map(|i| {
            let t = i as f32 / STEPS as f32;
            let s = 1.0 - t;
            let p = from.to_vec2() * (s * s * s)
                + c1.to_vec2() * (3.0 * s * s * t)
                + c2.to_vec2() * (3.0 * s * t * t)
                + to.to_vec2() * (t * t * t);
            p.to_pos2()
        })
        .collect()
}

/// The path data's numbers and commands, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Command(char),
    Number(f32),
}

fn tokens(d: &str) -> Option<Vec<Token>> {
    let bytes = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_whitespace() || c == ',' {
            i += 1;
        } else if c.is_ascii_alphabetic() {
            out.push(Token::Command(c));
            i += 1;
        } else {
            // A number: a sign, digits, one point, digits. A second point or a sign starts
            // the next number, as SVG's grammar has it (`5.9.8` is 5.9 then .8).
            let start = i;
            if c == '-' || c == '+' {
                i += 1;
            }
            let mut point = false;
            while i < bytes.len() {
                let b = bytes[i] as char;
                if b.is_ascii_digit() {
                    i += 1;
                } else if b == '.' && !point {
                    point = true;
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(Token::Number(d[start..i].parse().ok()?));
        }
    }
    Some(out)
}

/// The runs an SVG path's `d` draws, or `None` when it uses syntax the icons do not.
fn path(d: &str) -> Option<Vec<Run>> {
    let tokens = tokens(d)?;
    let mut runs: Vec<Run> = Vec::new();
    let mut current = Run {
        points: Vec::new(),
        closed: false,
    };
    let mut at = Pos2::ZERO;
    let mut start = Pos2::ZERO;
    let mut command = ' ';
    let mut i = 0;
    let numbers = |i: &mut usize, n: usize| -> Option<Vec<f32>> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            match tokens.get(*i) {
                Some(Token::Number(v)) => out.push(*v),
                _ => return None,
            }
            *i += 1;
        }
        Some(out)
    };
    while i < tokens.len() {
        match tokens[i] {
            Token::Command(c) => {
                command = c;
                i += 1;
                if c == 'Z' || c == 'z' {
                    current.closed = true;
                    at = start;
                    runs.push(std::mem::replace(
                        &mut current,
                        Run {
                            points: Vec::new(),
                            closed: false,
                        },
                    ));
                    continue;
                }
            }
            // Numbers after a command's own repeat it; after a move they are lines.
            Token::Number(_) => {
                command = match command {
                    'M' => 'L',
                    'm' => 'l',
                    ' ' | 'Z' | 'z' => return None,
                    other => other,
                };
            }
        }
        let relative = command.is_ascii_lowercase();
        let base = if relative { at.to_vec2() } else { Vec2::ZERO };
        match command.to_ascii_uppercase() {
            'M' => {
                let n = numbers(&mut i, 2)?;
                if current.points.len() > 1 {
                    runs.push(std::mem::replace(
                        &mut current,
                        Run {
                            points: Vec::new(),
                            closed: false,
                        },
                    ));
                }
                at = pos2(n[0], n[1]) + base;
                start = at;
                current.points = vec![at];
            }
            'L' => {
                let n = numbers(&mut i, 2)?;
                at = pos2(n[0], n[1]) + base;
                current.points.push(at);
            }
            'H' => {
                let n = numbers(&mut i, 1)?;
                at.x = n[0] + base.x;
                current.points.push(at);
            }
            'V' => {
                let n = numbers(&mut i, 1)?;
                at.y = n[0] + base.y;
                current.points.push(at);
            }
            'A' => {
                let n = numbers(&mut i, 7)?;
                let to = pos2(n[5], n[6]) + base;
                current
                    .points
                    .extend(arc_to(at, vec2(n[0], n[1]), n[3] != 0.0, n[4] != 0.0, to));
                at = to;
            }
            'C' => {
                let n = numbers(&mut i, 6)?;
                let c1 = pos2(n[0], n[1]) + base;
                let c2 = pos2(n[2], n[3]) + base;
                let to = pos2(n[4], n[5]) + base;
                current.points.extend(cubic(at, c1, c2, to));
                at = to;
            }
            _ => return None,
        }
    }
    if current.points.len() > 1 {
        runs.push(current);
    }
    Some(runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_reads_and_draws_inside_its_box() {
        for (name, icon) in ALL {
            for part in icon.parts {
                if let Part::Path(d) = part {
                    assert!(path(d).is_some(), "{name}: the parser cannot read `{d}`");
                }
            }
            let runs = runs(&icon);
            assert!(!runs.is_empty(), "{name} draws nothing");
            for run in &runs {
                assert!(run.points.len() > 1, "{name} has a run of one point");
                for p in &run.points {
                    assert!(
                        (0.0..=BOX).contains(&p.x) && (0.0..=BOX).contains(&p.y),
                        "{name} leaves its box at {p:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_path_follows_the_svg_grammar() {
        // Relative moves and lines, the implicit line after a move, H and V.
        let runs = path("M4 7h16M4 12h16").unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].points, vec![pos2(4.0, 7.0), pos2(20.0, 7.0)]);
        let caret = path("M6 9l6 6 6-6").unwrap();
        assert_eq!(
            caret[0].points,
            vec![pos2(6.0, 9.0), pos2(12.0, 15.0), pos2(18.0, 9.0)]
        );
        // Packed numbers: `5.9.8` is 5.9 then .8, `1-5` is 1 then -5.
        assert_eq!(
            tokens("l5.9.8-4.3").unwrap(),
            vec![
                Token::Command('l'),
                Token::Number(5.9),
                Token::Number(0.8),
                Token::Number(-4.3)
            ]
        );
        // A closed path closes; the headphones' arc is the top half of a circle of radius 8.
        assert!(path("M4 9.5h3.5L12 5.5z").unwrap()[0].closed);
        let arc = &path("M4 12a8 8 0 0 1 16 0").unwrap()[0].points;
        assert_eq!(*arc.last().unwrap(), pos2(20.0, 12.0));
        let top = arc.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        assert!((top - 4.0).abs() < 0.01, "the arc peaks at y {top}");
    }
}
