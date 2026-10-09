//! Walking along a line by fractions of its length, as PostGIS's linear referencing does.

use wkt::types::Coord;

/// How segment lengths are measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Length {
    /// In 2D, as most linear referencing functions measure.
    Planar,
    /// In 3D when the line has Z, as ST_3DLineInterpolatePoint measures.
    Spatial,
}

fn segment_length(a: &Coord<f64>, b: &Coord<f64>, length: Length) -> f64 {
    let planar = (b.x - a.x).hypot(b.y - a.y);
    match (length, a.z.zip(b.z)) {
        (Length::Spatial, Some((za, zb))) => planar.hypot(zb - za),
        _ => planar,
    }
}

/// The length of a line.
pub(crate) fn line_length(coords: &[Coord<f64>], length: Length) -> f64 {
    coords
        .windows(2)
        .map(|pair| segment_length(&pair[0], &pair[1], length))
        .sum()
}

/// The segments of a line with their lengths as fractions of the whole, and the fraction where
/// each starts, accumulated in order as PostGIS accumulates them.
fn fractions(coords: &[Coord<f64>], length: Length) -> impl Iterator<Item = (usize, f64, f64)> {
    let total = line_length(coords, length);
    let mut start = 0.0;
    coords.windows(2).enumerate().map(move |(index, pair)| {
        let fraction = segment_length(&pair[0], &pair[1], length) / total;
        let segment = (index, start, fraction);
        start += fraction;
        segment
    })
}

/// The point at `fraction` (in [0, 1]) of a line's length, or `None` for an empty line: the first point for 0 or
/// a line of no length, the last for 1, and otherwise `a + (b - a) * d` on the segment (a, b) it
/// falls on, where `d` is the fraction of that segment, including Z and M. PostGIS's results
/// show it computes `d` from fractions of the whole length, as here.
pub(crate) fn interpolate(
    coords: &[Coord<f64>],
    fraction: f64,
    length: Length,
) -> Option<Coord<f64>> {
    let (first, last) = (coords.first()?, coords.last()?);
    if fraction <= 0.0 || line_length(coords, length) == 0.0 {
        return Some(*first);
    }
    if fraction >= 1.0 {
        return Some(*last);
    }
    for (index, start, segment) in fractions(coords, length) {
        if fraction < start + segment {
            return Some(lerp(
                &coords[index],
                &coords[index + 1],
                (fraction - start) / segment,
            ));
        }
    }
    Some(*last)
}

fn lerp(a: &Coord<f64>, b: &Coord<f64>, d: f64) -> Coord<f64> {
    let mix = |a: f64, b: f64| a + (b - a) * d;
    Coord {
        x: mix(a.x, b.x),
        y: mix(a.y, b.y),
        z: a.z.zip(b.z).map(|(a, b)| mix(a, b)),
        m: a.m.zip(b.m).map(|(a, b)| mix(a, b)),
    }
}

/// The part of a line between the fractions `from` and `to` of its 2D length: the points there
/// and the vertices strictly between, without consecutive repeats; empty for an empty line.
///
/// Unlike [`interpolate`], PostGIS measures this one in absolute lengths (`from * length` along
/// the segments), which rounds differently; its results show both.
pub(crate) fn substring(coords: &[Coord<f64>], from: f64, to: f64) -> Vec<Coord<f64>> {
    let Some(first) = coords.first() else {
        return vec![];
    };
    let total = line_length(coords, Length::Planar);
    if total == 0.0 {
        return vec![*first];
    }
    let (from, to) = (from * total, to * total);
    let at = |target: f64| {
        let mut start = 0.0;
        for pair in coords.windows(2) {
            let segment = segment_length(&pair[0], &pair[1], Length::Planar);
            if target < start + segment {
                return lerp(&pair[0], &pair[1], (target - start) / segment);
            }
            start += segment;
        }
        coords[coords.len() - 1]
    };
    let mut out = vec![at(from)];
    let mut start = 0.0;
    for pair in coords.windows(2) {
        start += segment_length(&pair[0], &pair[1], Length::Planar);
        if start > from && start < to {
            out.push(pair[1]);
        }
    }
    out.push(at(to));
    out.dedup();
    out
}
