use super::*;
use bevy::math::DVec2 as V;
use std::cmp::Ordering;

impl Point {
    pub(super) fn plus(self, b: Self) -> Self {
        Self {
            x: self.x + b.x,
            y: self.y + b.y,
        }
    }

    fn vec(self) -> V {
        V::new(self.x.into(), self.y.into())
    }

    fn rounded(v: V) -> Self {
        Self {
            x: v.x.round() as i32,
            y: v.y.round() as i32,
        }
    }
}

impl Heading {
    const PERIOD: f64 = 4294967296.;

    pub const STEP: u32 =
        (TURN_RAD_S / HZ as f64 * Self::PERIOD / std::f64::consts::TAU + 0.5) as u32;

    pub const fn bearing(b: u8) -> Self {
        Self((b as u32) << 24)
    }

    pub fn radians(self) -> f64 {
        self.0 as f64 * (std::f64::consts::TAU / Self::PERIOD)
    }

    pub fn bearing_units(self) -> f64 {
        self.0 as f64 / (1u32 << 24) as f64
    }

    fn axis(self) -> V {
        let (s, c) = libm::sincos(self.radians());
        V::new(c, s)
    }

    pub fn direction(self, speed: i32) -> Point {
        Point::rounded(self.axis() * f64::from(speed))
    }
}

impl Tank {
    /// 观察与实际开火共用同一个炮口和初速度；朝向只需换算一次。
    pub(super) fn shot_path(&self) -> (Point, Point) {
        let axis = self.heading.axis();
        let offset = Point::rounded(axis * f64::from(SIZES.muzzle));
        let velocity = Point::rounded(axis * f64::from(SIZES.shell_speed));
        (self.center.plus(offset), velocity)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Fraction(f64);

impl Fraction {
    pub fn cmp(self, b: Self) -> Ordering {
        self.0.total_cmp(&b.0)
    }

    pub fn part(self, p: Point) -> Point {
        Point::rounded(p.vec() * self.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Contact {
    pub t: Fraction,
    normal: V,
}

impl Contact {
    pub fn reflect(self, p: Point) -> Point {
        Point::rounded(p.vec().reflect(self.normal))
    }
}

#[derive(Clone, Copy)]
struct Rect {
    center: V,
    axis: V,
    half: V,
}

impl Rect {
    fn wall(w: WallRect) -> Self {
        Self {
            center: (w.min.vec() + w.max.vec()) * 0.5,
            axis: V::X,
            half: (w.max.vec() - w.min.vec()) * 0.5,
        }
    }

    fn tank(p: Point, h: Heading) -> [Self; 2] {
        let axis = h.axis();
        let rect = |offset: i32, x: i32, y: i32| Self {
            center: p.vec() + axis * f64::from(offset),
            axis,
            half: V::new(x.into(), y.into()),
        };
        [
            rect(0, SIZES.half_length, SIZES.half_width),
            rect(
                SIZES.turret_offset,
                SIZES.turret_half_length,
                SIZES.turret_half_width,
            ),
        ]
    }

    fn local(self, v: V) -> V {
        V::new(v.dot(self.axis), v.dot(self.axis.perp()))
    }

    fn overlaps(self, other: Self) -> bool {
        let gap = other.center - self.center;
        [self.axis, self.axis.perp(), other.axis, other.axis.perp()]
            .into_iter()
            .all(|axis| {
                gap.dot(axis).abs()
                    < self.local(axis).abs().dot(self.half)
                        + other.local(axis).abs().dot(other.half)
                        - 1e-8
            })
    }

    // 用炮弹中心扫过由半径扩张的矩形：四条直边和四段四分之一圆弧。
    // 若直接扩大轴对齐包围盒，会把擦过圆角误判为命中。
    fn cast(self, p: Point, d: Point, r: i32, solid: bool) -> Option<Contact> {
        let p = self.local(p.vec() - self.center);
        let d = self.local(d.vec());
        let r = f64::from(r);
        let h = self.half;
        let gap = p - p.clamp(-h, h);
        let mut best = None;
        let mut insert = |t: f64, n: V| {
            if (0.0..=1.0).contains(&t) && best.is_none_or(|c: Contact| t < c.t.0) {
                best = Some(Contact {
                    t: Fraction(t.max(0.)),
                    normal: self.axis * n.x + self.axis.perp() * n.y,
                });
            }
        };
        if gap.length_squared() <= r * r {
            let n = if gap.length_squared() > 0. {
                gap.normalize()
            } else if h.x - p.x.abs() < h.y - p.y.abs() {
                V::X * p.x.signum()
            } else {
                V::Y * p.y.signum()
            };
            if solid || d.dot(n) < -1e-9 {
                insert(0., n);
            }
        }
        for (v, w, a, b, k, l, n) in [
            (p.x, p.y, d.x, d.y, h.x, h.y, V::X),
            (p.y, p.x, d.y, d.x, h.y, h.x, V::Y),
        ] {
            if a == 0. {
                continue;
            }

            let sign = -a.signum();
            let t = (sign * (k + r) - v) / a;
            if (w + t * b).abs() <= l {
                insert(t, n * sign);
            }
        }

        let a = d.length_squared();
        if a > 0. {
            for sx in [-1., 1.] {
                for sy in [-1., 1.] {
                    let q = p - V::new(sx * h.x, sy * h.y);
                    let b = q.dot(d);
                    let det = b * b - a * (q.length_squared() - r * r);
                    if det <= 0. {
                        continue;
                    }

                    let t = (-b - det.sqrt()) / a;
                    let n = q + t * d;
                    if n.x * sx >= 0. && n.y * sy >= 0. {
                        insert(t, n / r);
                    }
                }
            }
        }
        best
    }
}

pub(super) fn tank_clear(map: &ArenaMap, p: Point, h: Heading) -> bool {
    let mut walls = map.layout.candidates(p, p, TANK_REACH);
    let Some(first) = walls.next() else {
        return true;
    };
    let fixtures = Rect::tank(p, h);
    !std::iter::once(first).chain(walls).any(|w| {
        let wall = Rect::wall(*w);
        fixtures.iter().any(|f| f.overlaps(wall))
    })
}

pub(super) fn tanks_overlap((a, ha): (Point, Heading), (b, hb): (Point, Heading)) -> bool {
    let bound = 2 * TANK_REACH;
    if (a.x - b.x).abs() > bound || (a.y - b.y).abs() > bound {
        return false;
    }

    let other = Rect::tank(b, hb);
    Rect::tank(a, ha)
        .iter()
        .any(|f| other.iter().any(|g| f.overlaps(*g)))
}

pub(super) fn body(p: Point, d: Point, t: &Tank, r: i32) -> Option<Contact> {
    // 先用整段弹道的包围盒排除远车，再对车身和炮塔做精确的圆角碰撞检测。
    let end = p.plus(d);
    let reach = TANK_REACH + r;
    if t.center.x < p.x.min(end.x) - reach
        || t.center.x > p.x.max(end.x) + reach
        || t.center.y < p.y.min(end.y) - reach
        || t.center.y > p.y.max(end.y) + reach
    {
        return None;
    }

    Rect::tank(t.center, t.heading)
        .iter()
        .filter_map(|f| f.cast(p, d, r, true))
        .min_by(|a, b| a.t.cmp(b.t))
}

pub(super) fn walls(map: &ArenaMap, p: Point, d: Point, r: i32) -> Option<Contact> {
    let mut first: Option<Contact> = None;
    for w in map.layout.candidates(p, p.plus(d), r) {
        if let Some(hit) = Rect::wall(*w).cast(p, d, r, false) {
            match &mut first {
                Some(c) if (c.t.0 - hit.t.0).abs() < 1e-10 => {
                    if let Some(n) = (c.normal + hit.normal).try_normalize() {
                        c.normal = n;
                    }
                }
                Some(c) if c.t.0 < hit.t.0 => (),
                _ => first = Some(hit),
            }
        }
    }

    first
}

/// Earliest inward crossing of a circular shield. An outgoing shot can leave it freely.
pub(super) fn shield(p: Point, d: Point, center: Point, radius: i32) -> Option<Contact> {
    let q = p.vec() - center.vec();
    let d = d.vec();
    let a = d.length_squared();
    let b = q.dot(d);
    if a == 0. || b >= 0. {
        return None;
    }
    let c = q.length_squared() - f64::from(radius).powi(2);
    let disc = b * b - a * c;
    if disc < 0. {
        return None;
    }
    let t = ((-b - libm::sqrt(disc)) / a).max(0.);
    if t > 1. {
        return None;
    }
    Some(Contact {
        t: Fraction(t),
        normal: (q + t * d).try_normalize()?,
    })
}

#[cfg(test)]
mod broadphase_tests {
    use super::*;

    #[test]
    fn wall_runs_keep_every_exact_hit_and_boundary_tie_in_order() {
        let map = ArenaMap::generate(93021, [12, 8]).unwrap();
        let walls: Vec<_> = map.wall_rectangles().collect();
        for &w in &walls {
            for p in [
                w.min,
                w.max,
                w.max.plus(Point {
                    x: SIZES.shell_radius,
                    y: 0,
                }),
            ] {
                for d in [
                    Point::default(),
                    Point { x: -CELL, y: 0 },
                    Point {
                        x: CELL / 2,
                        y: -CELL,
                    },
                ] {
                    for r in [SIZES.shell_radius, 0] {
                        let narrow = |w: WallRect| {
                            Rect::wall(w)
                                .cast(p, d, r, false)
                                .map(|hit| (w, hit.t.0.to_bits()))
                        };
                        let all = walls.iter().copied().filter_map(narrow).collect::<Vec<_>>();
                        let filtered = map
                            .layout
                            .candidates(p, p.plus(d), r)
                            .copied()
                            .filter_map(narrow)
                            .collect::<Vec<_>>();
                        assert_eq!(all, filtered);
                    }
                }
            }
        }
        for x in (0..=2400).step_by(37) {
            for y in (0..=1600).step_by(83) {
                let p = Point {
                    x: x * PX,
                    y: y * PX,
                };
                let h = Heading::bearing((x ^ y) as u8);
                let fixtures = Rect::tank(p, h);
                let all_clear = walls
                    .iter()
                    .all(|w| fixtures.iter().all(|f| !f.overlaps(Rect::wall(*w))));
                assert_eq!(tank_clear(&map, p, h), all_clear);
            }
        }
    }
}
