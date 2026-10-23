//! Walking along geography lines, as PostGIS's geography linear referencing does: segment
//! lengths on the spheroid or the sphere, and points at a distance along a segment, along its
//! geodesic or great circle from its start.

use wkt::types::Coord;

use crate::udf::native::util::geodesic::Geodesic;
use crate::udf::native::util::geodetic_distance::nearest_on_line;

/// A line's coordinates, as longitude and latitude in degrees, with its segments' lengths.
pub(crate) struct GeodeticLine<'a> {
    geodesic: &'a Geodesic,
    use_spheroid: bool,
    coords: Vec<Coord<f64>>,
    lengths: Vec<f64>,
    total: f64,
}

impl<'a> GeodeticLine<'a> {
    pub(crate) fn new(geodesic: &'a Geodesic, coords: Vec<Coord<f64>>, use_spheroid: bool) -> Self {
        let lengths: Vec<f64> = coords
            .windows(2)
            .map(|pair| geodesic.distance(xy(&pair[0]), xy(&pair[1]), use_spheroid))
            .collect();
        let total = lengths.iter().sum();
        Self {
            geodesic,
            use_spheroid,
            coords,
            lengths,
            total,
        }
    }

    /// The point `fraction` of the way along the line, by length; its ends exactly at 0 and 1.
    /// Z and M are interpolated linearly. `None` for a line without points.
    pub(crate) fn point_at(&self, fraction: f64) -> Option<Coord<f64>> {
        let (first, last) = (self.coords.first()?, self.coords.last()?);
        if fraction <= 0.0 || self.coords.len() < 2 {
            return Some(*first);
        }
        if fraction >= 1.0 {
            return Some(*last);
        }
        let target = fraction * self.total;
        let mut walked = 0.0;
        // At a vertex exactly, the next segment starts there, as in PostGIS.
        for (index, &length) in self.lengths.iter().enumerate() {
            if walked + length > target {
                return Some(self.along(index, target - walked));
            }
            walked += length;
        }
        Some(*last)
    }

    /// The point `distance` along segment `index` from its start.
    fn along(&self, index: usize, distance: f64) -> Coord<f64> {
        let (start, end) = (self.coords[index], self.coords[index + 1]);
        let length = self.lengths[index];
        let (x, y) = self
            .geodesic
            .along(xy(&start), xy(&end), distance, self.use_spheroid);
        let t = if length > 0.0 { distance / length } else { 0.0 };
        let mix = |a: Option<f64>, b: Option<f64>| a.zip(b).map(|(a, b)| a + (b - a) * t);
        Coord {
            x,
            y,
            z: mix(start.z, end.z),
            m: mix(start.m, end.m),
        }
    }

    /// The points every `fraction` of the way along the line, or only the first unless `repeat`.
    /// As in PostGIS, the fraction is added up rather than multiplied.
    pub(crate) fn points_every(&self, fraction: f64, repeat: bool) -> Vec<Coord<f64>> {
        let count = if repeat && fraction > 0.0 {
            (1.0 / fraction).floor() as usize
        } else {
            1
        };
        let mut at = 0.0;
        (0..count)
            .filter_map(|_| {
                at += fraction;
                self.point_at(at)
            })
            .collect()
    }

    /// The part of the line from `start` to `end`, as fractions of its length: the point at
    /// `start`, the vertices strictly between, and the point at `end`, without repeated points.
    pub(crate) fn substring(&self, start: f64, end: f64) -> Vec<Coord<f64>> {
        let (Some(first), Some(last)) = (self.point_at(start), self.point_at(end)) else {
            return vec![];
        };
        let (from, to) = (start * self.total, end * self.total);
        let mut out = vec![first];
        let mut push = |coord: Coord<f64>| {
            if out.last() != Some(&coord) {
                out.push(coord);
            }
        };
        let mut walked = 0.0;
        for (index, &length) in self.lengths.iter().enumerate() {
            walked += length;
            if walked > from && walked < to {
                push(self.coords[index + 1]);
            }
        }
        push(last);
        out
    }

    /// The fraction of the line's length before its point nearest to `point`, found on the
    /// sphere; 0 for a line of no length.
    pub(crate) fn locate(&self, point: (f64, f64)) -> f64 {
        let line: Vec<(f64, f64)> = self.coords.iter().map(xy).collect();
        let Some((index, nearest)) = nearest_on_line(&line, point) else {
            return 0.0;
        };
        if self.total == 0.0 {
            return 0.0;
        }
        let walked: f64 = self.lengths[..index].iter().sum();
        let rest = self
            .geodesic
            .distance(line[index], nearest, self.use_spheroid);
        ((walked + rest) / self.total).clamp(0.0, 1.0)
    }
}

fn xy(coord: &Coord<f64>) -> (f64, f64) {
    (coord.x, coord.y)
}
