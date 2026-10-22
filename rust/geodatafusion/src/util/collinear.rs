//! Removing collinear vertices, as PostGIS's simplification does for a tolerance of 0.

use wkt::types::Coord;

/// Removes, in order, every vertex on the segment from the last vertex kept to the next
/// vertex, ends included. A vertex is kept if that segment is a single point.
///
/// This is what PostGIS does for a tolerance of 0, derived from its output: unlike Douglas-Peucker
/// with a tolerance of 0, it keeps vertices that backtrack along a line, and all but one of the
/// copies of a line's first vertex.
pub(crate) fn remove_collinear(coords: &[Coord<f64>]) -> Vec<Coord<f64>> {
    let mut kept = vec![coords[0]];
    for window in coords.windows(3) {
        let (a, p, b) = (
            xy(kept.last().expect("the first vertex is kept")),
            xy(&window[1]),
            xy(&window[2]),
        );
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length_squared = dx * dx + dy * dy;
        let dot = (p.0 - a.0) * dx + (p.1 - a.1) * dy;
        let cross = dx * (p.1 - a.1) - dy * (p.0 - a.0);
        let on_segment =
            length_squared != 0.0 && cross == 0.0 && (0.0..=length_squared).contains(&dot);
        if !on_segment {
            kept.push(window[1]);
        }
    }
    kept.push(coords[coords.len() - 1]);
    kept
}

fn xy(coord: &Coord<f64>) -> (f64, f64) {
    (coord.x, coord.y)
}
