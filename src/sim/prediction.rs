//! Read-only collision queries for hypothetical motion; never advance the referee.
use super::*;
pub fn clearance(m: &ArenaMap, p: Point, h: Heading) -> bool {
    geometry::tank_clear(m, p, h)
}
pub fn overlap(a: (Point, Heading), b: (Point, Heading)) -> bool {
    geometry::tanks_overlap(a, b)
}
pub fn wall(m: &ArenaMap, p: Point, d: Point, r: i32) -> Option<(Point, Point)> {
    geometry::walls(m, p, d, r).map(|h| (h.t.part(d), h.reflect(d)))
}
pub fn body_hit(p: Point, d: Point, t: &Tank, r: i32) -> Option<Point> {
    geometry::body(p, d, t, r).map(|h| h.t.part(d))
}
/// Shot sweep, matching referee wall/shield tie order and rounding of BOTH reflected vectors.
pub fn reflect_surface(
    map: &ArenaMap,
    p: Point,
    d: Point,
    v: Point,
    r: i32,
    shields: impl Iterator<Item = Point>,
) -> Option<(Point, Point, Point)> {
    let mut hit = geometry::walls(map, p, d, r);
    for center in shields {
        if let Some(c) = geometry::shield(p, d, center, SHIELD_RADIUS + r)
            && hit.is_none_or(|h| c.t.cmp(h.t).is_lt())
        {
            hit = Some(c)
        }
    }
    hit.map(|c| {
        let used = c.t.part(d);
        (
            used,
            c.reflect(v),
            c.reflect(Point {
                x: d.x - used.x,
                y: d.y - used.y,
            }),
        )
    })
}
