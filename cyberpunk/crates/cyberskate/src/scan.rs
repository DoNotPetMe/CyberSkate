//! The world around the skater as the game's ray casts see it, and the
//! collision the skate engine rides built from that.
//!
//! A scan is a square grid of downward casts (ground height and normal per
//! sample), the exact positions of the steps between samples found by
//! bisecting those edges, and a ring of horizontal casts that catches walls
//! too tall for the downward casts to start above.
//!
//! Ground becomes a terraced surface: each grid cell is cut by marching
//! squares wherever neighbouring samples are not one continuous surface, every
//! side keeps its own height, and a vertical face joins them. Neighbouring
//! cells share their sample and crossing vertices bit for bit, so the skate
//! engine's vertex welding finds the adjacency across the whole patch.
use bevy::math::{Vec2, Vec3};
use std::collections::{HashMap, HashSet};

/// Height differences a surface's own slope does not explain beyond this
/// are steps.
const STEP: f32 = 0.06;
/// Downward-cast normals flatter than this give no slope to extrapolate.
const MIN_SLOPE_NORMAL_Z: f32 = 0.35;
/// Ring hits steeper than this are walls.
const WALL_NORMAL_Z: f32 = 0.6;
/// How far walls reach below and above the ground they stand on.
const WALL_BELOW: f32 = 1.5;
const WALL_ABOVE: f32 = 4.0;
/// Low ring hits become posts this wide, this tall above their ground.
const POST_HALF: f32 = 0.15;
const POST_ABOVE: f32 = 2.2;
/// Ground this much higher just behind a low hit means the grid already
/// caught the obstacle.
const POST_RAISED: f32 = 0.2;
/// Posts closer than this are one post.
const POST_SAME: f32 = 0.3;
/// A low cast that hit nothing is taken to have reached at least this far.
const MISS_REACH: f32 = 4.0;
/// Crossings stay this fraction of an edge away from its samples: closer,
/// the slivers beside them are too thin to keep and would leave holes.
const CROSSING_MARGIN: f32 = 0.1;
pub const MAX_SIZE: usize = 161;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ground {
    pub height: f32,
    pub normal: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallHit {
    pub position: Vec3,
    pub normal: Vec3,
}

/// A thin obstacle the low ring found: a square column standing on
/// `ground`, one face turned to `facing`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Post {
    pub centre: Vec2,
    pub facing: Vec2,
    pub ground: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    X = 0,
    Y = 1,
}

#[derive(Clone, Debug)]
pub struct Scan {
    /// World xy of sample (0, 0); sample (i, j) sits `spacing` apart along
    /// +x for i and +y for j.
    pub origin: Vec2,
    pub spacing: f32,
    pub size: usize,
    /// Row-major, `j * size + i`.
    pub ground: Vec<Option<Ground>>,
    /// Step position along a grid edge, from its lower-index sample, 0..1.
    pub crossings: HashMap<(usize, Axis), f32>,
    /// Where the ring casts start.
    pub eye: Vec3,
    /// Ground height under the skater.
    pub floor: f32,
    /// Ring casts in angular order; misses are `None`.
    pub ring: Vec<Option<WallHit>>,
    /// The low ring, cast at knee height for things too thin for the grid:
    /// poles, bollards, posts.
    pub low_ring: Vec<Option<WallHit>>,
}

/// Header of a scan as the mod sends it: origin x, origin y, spacing, size,
/// eye x, eye y, eye z, floor z.
pub const HEADER_LEN: usize = 8;

fn hit(value: f32) -> bool {
    value.is_finite() && value > -1.0e5
}

impl Scan {
    fn parse_grid(header: &[f32], ground: &[f32]) -> Result<Self, String> {
        let [ox, oy, spacing, size, ex, ey, ez, floor] = header
            .get(..HEADER_LEN)
            .and_then(|h| <[f32; HEADER_LEN]>::try_from(h).ok())
            .ok_or("scan header needs 8 values")?;
        if !(0.05..=8.).contains(&spacing) || !size.is_finite() {
            return Err(format!("scan spacing {spacing} is out of range"));
        }
        let size = size as usize;
        if !(2..=MAX_SIZE).contains(&size) {
            return Err(format!("scan size {size} is out of range"));
        }
        if [ox, oy, ex, ey, ez, floor].iter().any(|v| !v.is_finite()) {
            return Err("scan header is not finite".into());
        }
        if ground.len() != size * size * 4 {
            return Err(format!(
                "scan ground has {} values, expected {}",
                ground.len(),
                size * size * 4
            ));
        }
        let ground = ground
            .chunks_exact(4)
            .map(|s| {
                let normal = Vec3::new(s[1], s[2], s[3]);
                (hit(s[0]) && normal.is_finite()).then(|| Ground {
                    height: s[0],
                    normal: normal.normalize_or(Vec3::Z),
                })
            })
            .collect();
        Ok(Self {
            origin: Vec2::new(ox, oy),
            spacing,
            size,
            ground,
            crossings: HashMap::new(),
            eye: Vec3::new(ex, ey, ez),
            floor,
            ring: Vec::new(),
            low_ring: Vec::new(),
        })
    }

    pub fn parse(
        header: &[f32],
        ground: &[f32],
        edges: &[f32],
        walls: &[f32],
        posts: &[f32],
    ) -> Result<Self, String> {
        let mut scan = Self::parse_grid(header, ground)?;
        if edges.len() % 3 != 0 || walls.len() % 6 != 0 || posts.len() % 6 != 0 {
            return Err("scan edges come in threes, walls and posts in sixes".into());
        }
        for e in edges.chunks_exact(3) {
            let (index, axis, t) = (e[0], e[1], e[2]);
            if !(index.is_finite() && t.is_finite()) || index < 0. {
                continue;
            }
            let axis = if axis == 0. { Axis::X } else { Axis::Y };
            let index = index as usize;
            if index < scan.size * scan.size {
                scan.crossings.insert((index, axis), t);
            }
        }
        let hits = |values: &[f32]| -> Vec<Option<WallHit>> {
            values
                .chunks_exact(6)
                .map(|w| {
                    let position = Vec3::new(w[0], w[1], w[2]);
                    let normal = Vec3::new(w[3], w[4], w[5]);
                    (w.iter().all(|v| hit(*v)) && normal.length_squared() > 1e-6).then(|| WallHit {
                        position,
                        normal: normal.normalize(),
                    })
                })
                .collect()
        };
        scan.ring = hits(walls);
        scan.low_ring = hits(posts);
        Ok(scan)
    }

    /// Grid edges whose step positions the mod should bisect, as flat
    /// (sample index, axis) pairs.
    pub fn breaks(header: &[f32], ground: &[f32]) -> Result<Vec<f32>, String> {
        let scan = Self::parse_grid(header, ground)?;
        let mut out = Vec::new();
        for j in 0..scan.size {
            for i in 0..scan.size {
                let a = scan.index(i, j);
                for (axis, b) in [
                    (Axis::X, (i + 1 < scan.size).then(|| scan.index(i + 1, j))),
                    (Axis::Y, (j + 1 < scan.size).then(|| scan.index(i, j + 1))),
                ] {
                    let Some(b) = b else { continue };
                    let (ga, gb) = (scan.ground[a], scan.ground[b]);
                    // Only edges with ground on at least one side have a
                    // position worth finding.
                    if (ga.is_some() || gb.is_some()) && !scan.continuous(a, b) {
                        out.extend([a as f32, axis as u8 as f32]);
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn index(&self, i: usize, j: usize) -> usize {
        j * self.size + i
    }

    fn xy(&self, index: usize) -> Vec2 {
        let (i, j) = (index % self.size, index / self.size);
        self.origin + Vec2::new(i as f32, j as f32) * self.spacing
    }

    /// Height the plane through sample `index` has above `at`.
    fn extrapolate(&self, index: usize, at: Vec2) -> f32 {
        let g = self.ground[index].expect("extrapolating from a missing sample");
        if g.normal.z < MIN_SLOPE_NORMAL_Z {
            return g.height;
        }
        let d = at - self.xy(index);
        let rise = -(g.normal.x * d.x + g.normal.y * d.y) / g.normal.z;
        g.height + rise.clamp(-self.spacing, self.spacing)
    }

    /// One surface runs between the two samples: the height difference lies
    /// within what the two samples' slopes, meeting anywhere between them,
    /// can produce.
    fn continuous(&self, a: usize, b: usize) -> bool {
        let (Some(ga), Some(gb)) = (self.ground[a], self.ground[b]) else {
            return false;
        };
        let dz = gb.height - ga.height;
        let at_b = self.xy(b);
        let at_a = self.xy(a);
        let from_a = self.extrapolate(a, at_b) - ga.height;
        let from_b = gb.height - self.extrapolate(b, at_a);
        let (low, high) = (from_a.min(from_b), from_a.max(from_b));
        (low - STEP..=high + STEP).contains(&dz)
    }

    /// Where the step on the edge between `a` and its +axis neighbour `b`
    /// lies.
    fn crossing(&self, a: usize, axis: Axis, b: usize) -> Vec2 {
        let t = self
            .crossings
            .get(&(a, axis))
            .map_or(0.5, |t| t.clamp(CROSSING_MARGIN, 1. - CROSSING_MARGIN));
        self.xy(a).lerp(self.xy(b), t)
    }

    /// Triangles in Night City coordinates, counter-clockwise seen from their
    /// solid side's outside.
    pub fn triangles(&self) -> Vec<[Vec3; 3]> {
        self.triangles_with(&self.found_posts())
    }

    fn triangles_with(&self, posts: &[Post]) -> Vec<[Vec3; 3]> {
        let mut out = Triangles::default();
        for j in 0..self.size - 1 {
            for i in 0..self.size - 1 {
                self.cell(i, j, &mut out);
            }
        }
        self.walls(&mut out);
        for post in posts {
            out.post(
                post.centre,
                post.facing,
                post.ground - WALL_BELOW,
                post.ground + POST_ABOVE,
            );
        }
        out.0
    }

    /// Whether `at` lies within the scanned square.
    fn covers(&self, at: Vec2) -> bool {
        let far = self.origin + Vec2::splat((self.size - 1) as f32 * self.spacing);
        at.cmpge(self.origin).all() && at.cmple(far).all()
    }

    /// Whether a low cast went straight through where `post` stood.
    fn clears(&self, post: &Post) -> bool {
        let n = self.low_ring.len();
        let to = post.centre - self.eye.truncate();
        if n == 0 || to.length() < 0.3 {
            return false;
        }
        let tau = std::f32::consts::TAU;
        let k = ((to.y.atan2(to.x).rem_euclid(tau) / tau * n as f32).round() as usize) % n;
        let angle = k as f32 / n as f32 * tau;
        let dir = Vec2::new(angle.cos(), angle.sin());
        if dir.perp_dot(to).abs() > POST_HALF * 0.8 {
            return false;
        }
        let along = dir.dot(to);
        match self.low_ring[k] {
            Some(hit) => {
                (hit.position.truncate() - self.eye.truncate()).length() > along + POST_HALF * 2.
            }
            None => along < MISS_REACH,
        }
    }

    /// Height of the sample nearest `at`, if it found ground.
    fn ground_at(&self, at: Vec2) -> Option<f32> {
        let local = ((at - self.origin) / self.spacing).round();
        if local.x < 0.
            || local.y < 0.
            || local.x >= self.size as f32
            || local.y >= self.size as f32
        {
            return None;
        }
        self.ground[self.index(local.x as usize, local.y as usize)].map(|g| g.height)
    }

    /// The highest ground found within `radius` of `at`.
    fn highest_near(&self, at: Vec2, radius: f32) -> Option<f32> {
        let reach = (radius / self.spacing).ceil() as i64;
        let centre = ((at - self.origin) / self.spacing).round();
        let mut best: Option<f32> = None;
        for dj in -reach..=reach {
            for di in -reach..=reach {
                let (i, j) = (centre.x as i64 + di, centre.y as i64 + dj);
                if i < 0 || j < 0 || i >= self.size as i64 || j >= self.size as i64 {
                    continue;
                }
                let index = self.index(i as usize, j as usize);
                if self.xy(index).distance(at) > radius {
                    continue;
                }
                if let Some(g) = self.ground[index] {
                    best = Some(best.map_or(g.height, |b: f32| b.max(g.height)));
                }
            }
        }
        best
    }

    /// Low ring hits on things the grid stepped over become thin posts.
    /// Where the grid did see the obstacle (a bench, a ledge, a car) its
    /// raised ground already stands there and nothing is added; where the
    /// high ring hit the same wall, its face already does.
    fn found_posts(&self) -> Vec<Post> {
        let mut found = Vec::new();
        for (k, low) in self.low_ring.iter().enumerate() {
            let Some(low) = low.filter(|w| w.normal.z.abs() < WALL_NORMAL_Z) else {
                continue;
            };
            let reach = (low.position - self.eye).truncate().length();
            let same_wall = self.ring.len() == self.low_ring.len()
                && self.ring[k].is_some_and(|high| {
                    high.normal.z.abs() < WALL_NORMAL_Z
                        && ((high.position - self.eye).truncate().length() - reach).abs() < 0.5
                });
            if same_wall {
                continue;
            }
            let normal = low.normal.truncate().normalize_or_zero();
            if normal == Vec2::ZERO {
                continue;
            }
            let at = low.position.truncate();
            let front = self.ground_at(at + normal * 0.3).unwrap_or(self.floor);
            let behind = self.highest_near(at - normal * 0.3, 0.6);
            if behind.is_some_and(|h| h > front + POST_RAISED) {
                continue;
            }
            found.push(Post {
                centre: at - normal * POST_HALF,
                facing: normal,
                ground: front,
            });
        }
        found
    }

    fn cell(&self, i: usize, j: usize, out: &mut Triangles) {
        let corners = [
            self.index(i, j),
            self.index(i + 1, j),
            self.index(i + 1, j + 1),
            self.index(i, j + 1),
        ];
        // Cycle edge k runs from corner k to corner k + 1. The first two run
        // along +x and +y from their start, the last two against.
        let edge = |k: usize| -> Vec2 {
            match k {
                0 => self.crossing(corners[0], Axis::X, corners[1]),
                1 => self.crossing(corners[1], Axis::Y, corners[2]),
                2 => self.crossing(corners[3], Axis::X, corners[2]),
                _ => self.crossing(corners[0], Axis::Y, corners[3]),
            }
        };
        let breaks: Vec<usize> = (0..4)
            .filter(|&k| !self.continuous(corners[k], corners[(k + 1) % 4]))
            .collect();
        let point = |index: usize| {
            let xy = self.xy(index);
            xy.extend(self.ground[index].unwrap().height)
        };
        if breaks.len() <= 1 {
            if corners.iter().any(|&c| self.ground[c].is_none()) {
                return;
            }
            let p = corners.map(point);
            // Split along the diagonal whose ends agree best.
            if (p[0].z - p[2].z).abs() <= (p[1].z - p[3].z).abs() {
                out.push([p[0], p[1], p[2]]);
                out.push([p[0], p[2], p[3]]);
            } else {
                out.push([p[0], p[1], p[3]]);
                out.push([p[1], p[2], p[3]]);
            }
            return;
        }
        let center = self.xy(corners[0]) + Vec2::splat(self.spacing * 0.5);
        let star = breaks.len() >= 3;
        // Each arc: the corners between one break and the next.
        struct Arc {
            corners: Vec<usize>,
            start: usize,
            end: usize,
        }
        let arcs: Vec<Arc> = (0..breaks.len())
            .map(|m| {
                let start = breaks[m];
                let end = breaks[(m + 1) % breaks.len()];
                let mut list = Vec::new();
                let mut k = (start + 1) % 4;
                loop {
                    list.push(corners[k]);
                    if k == end {
                        break;
                    }
                    k = (k + 1) % 4;
                }
                Arc {
                    corners: list,
                    start,
                    end,
                }
            })
            .collect();
        let solid = |arc: &Arc| arc.corners.iter().all(|&c| self.ground[c].is_some());
        let center_height = |arc: &Arc| {
            arc.corners
                .iter()
                .map(|&c| self.extrapolate(c, center))
                .sum::<f32>()
                / arc.corners.len() as f32
        };
        for arc in arcs.iter().filter(|a| solid(a)) {
            let first = arc.corners[0];
            let last = *arc.corners.last().unwrap();
            let s = edge(arc.start);
            let e = edge(arc.end);
            let mut polygon = vec![s.extend(self.extrapolate(first, s))];
            polygon.extend(arc.corners.iter().map(|&c| point(c)));
            polygon.push(e.extend(self.extrapolate(last, e)));
            if star {
                let hub = center.extend(center_height(arc));
                for w in polygon.windows(2) {
                    out.push([hub, w[0], w[1]]);
                }
            } else {
                for k in 1..polygon.len() - 1 {
                    out.push([polygon[0], polygon[k], polygon[k + 1]]);
                }
            }
        }
        // Faces between neighbouring arcs, along the line they share.
        let shared: Vec<(usize, usize, Vec2, Vec2)> = if star {
            (0..arcs.len())
                .map(|m| {
                    let next = (m + 1) % arcs.len();
                    (m, next, edge(arcs[m].end), center)
                })
                .collect()
        } else {
            vec![(0, 1, edge(arcs[0].end), edge(arcs[1].end))]
        };
        for (x, y, p, q) in shared {
            let (ax, ay) = (&arcs[x], &arcs[y]);
            if !solid(ax) || !solid(ay) {
                continue;
            }
            let height = |arc: &Arc, at: Vec2| {
                if at == center && star {
                    return center_height(arc);
                }
                // The arc corner on the break edge `at` lies on.
                let near = if edge(arc.end) == at {
                    *arc.corners.last().unwrap()
                } else {
                    arc.corners[0]
                };
                self.extrapolate(near, at)
            };
            let toward_y = self.xy(ay.corners[0]);
            out.wall(
                p,
                q,
                [height(ax, p), height(ax, q)],
                [height(ay, p), height(ay, q)],
                toward_y,
            );
        }
    }

    /// Walls the ring found: neighbouring hits on one surface are joined,
    /// lone hits get a patch of their own.
    fn walls(&self, out: &mut Triangles) {
        let n = self.ring.len();
        if n == 0 {
            return;
        }
        let step = std::f32::consts::TAU / n as f32;
        let walls: Vec<Option<WallHit>> = self
            .ring
            .iter()
            .map(|w| w.filter(|w| w.normal.z.abs() < WALL_NORMAL_Z))
            .collect();
        let ground_near = |p: Vec3| {
            let local = (p.truncate() - self.origin) / self.spacing;
            let i = local.x.round();
            let j = local.y.round();
            if i < 0. || j < 0. || i >= self.size as f32 || j >= self.size as f32 {
                return self.floor;
            }
            self.ground[self.index(i as usize, j as usize)].map_or(self.floor, |g| g.height)
        };
        let extent = |p: Vec3| {
            let g = ground_near(p);
            (
                g.min(self.floor) - WALL_BELOW,
                g.max(self.floor) + WALL_ABOVE,
            )
        };
        let joined = |a: WallHit, b: WallHit| {
            let reach = (a.position - self.eye)
                .length()
                .max((b.position - self.eye).length());
            a.normal.dot(b.normal) > 0.5
                && a.position.distance(b.position) < (2.5 * reach * step).max(1.0)
        };
        let mut linked = vec![false; n];
        for k in 0..n {
            let next = (k + 1) % n;
            if next == k {
                break;
            }
            let (Some(a), Some(b)) = (walls[k], walls[next]) else {
                continue;
            };
            if !joined(a, b) {
                continue;
            }
            linked[k] = true;
            linked[next] = true;
            let (a_low, a_high) = extent(a.position);
            let (b_low, b_high) = extent(b.position);
            out.facing(
                [
                    a.position.truncate().extend(a_low),
                    b.position.truncate().extend(b_low),
                    b.position.truncate().extend(b_high),
                    a.position.truncate().extend(a_high),
                ],
                self.eye,
            );
        }
        for k in 0..n {
            let Some(w) = walls[k].filter(|_| !linked[k]) else {
                continue;
            };
            let reach = (w.position - self.eye).length();
            let half = (reach * step * 0.6).clamp(0.1, 0.35);
            let along = w.normal.cross(Vec3::Z).normalize_or_zero();
            if along == Vec3::ZERO {
                continue;
            }
            let (low, high) = extent(w.position);
            let a = (w.position - along * half).truncate();
            let b = (w.position + along * half).truncate();
            out.facing(
                [a.extend(low), b.extend(low), b.extend(high), a.extend(high)],
                self.eye,
            );
        }
    }
}

#[derive(Default)]
struct Triangles(Vec<[Vec3; 3]>);

impl Triangles {
    fn push(&mut self, t: [Vec3; 3]) {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let shortest = t[0]
            .distance(t[1])
            .min(t[1].distance(t[2]))
            .min(t[2].distance(t[0]));
        if t.iter().all(|p| p.is_finite()) && n.length() > 2.0e-4 && shortest > 0.005 {
            self.0.push(t);
        }
    }

    /// A quad split in two, each half wound to face `toward`.
    fn facing(&mut self, q: [Vec3; 4], toward: Vec3) {
        for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            let centroid = (t[0] + t[1] + t[2]) / 3.;
            if n.dot(toward - centroid) >= 0. {
                self.push(t);
            } else {
                self.push([t[0], t[2], t[1]]);
            }
        }
    }

    /// A square column centred on `centre`, one face turned to `facing`.
    fn post(&mut self, centre: Vec2, facing: Vec2, low: f32, high: f32) {
        let u = facing * POST_HALF;
        let v = facing.perp() * POST_HALF;
        let corners = [
            centre + u + v,
            centre - u + v,
            centre - u - v,
            centre + u - v,
        ];
        let middle = (low + high) * 0.5;
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let outward = ((a + b) * 0.5 - centre) * 4.;
            self.facing(
                [a.extend(low), b.extend(low), b.extend(high), a.extend(high)],
                (centre + outward).extend(middle),
            );
        }
        self.facing(corners.map(|c| c.extend(high)), centre.extend(high + 1.));
    }

    /// The vertical face along `p`–`q` between side x (heights `hx`) and
    /// side y (heights `hy`), facing whichever side is lower. `toward_y` is a
    /// point on side y.
    fn wall(&mut self, p: Vec2, q: Vec2, hx: [f32; 2], hy: [f32; 2], toward_y: Vec2) {
        let d = [hx[0] - hy[0], hx[1] - hy[1]];
        if d[0].abs() < 0.01 && d[1].abs() < 0.01 {
            return;
        }
        if d[0] * d[1] < 0. && d[0].abs().min(d[1].abs()) > 0.01 {
            // The sides swap heights along the line; no single face fits.
            return;
        }
        let x_high = d[0] + d[1] > 0.;
        let along = (q - p).normalize_or_zero();
        let mut side_y = along.perp();
        if side_y.dot(toward_y - p) < 0. {
            side_y = -side_y;
        }
        let low_side = if x_high { side_y } else { -side_y };
        let mid = (p + q) * 0.5;
        let toward = (mid + low_side).extend((hx[0] + hy[0]) * 0.5);
        self.facing(
            [
                p.extend(hx[0]),
                q.extend(hx[1]),
                q.extend(hy[1]),
                p.extend(hy[0]),
            ],
            toward,
        );
    }
}

/// Posts seen by earlier scans. A pole is only hit when a cast happens to
/// line up with it, so one scan alone would let it blink in and out of the
/// world; a remembered post stays until a later cast passes through where it
/// stood or it leaves the scanned square.
#[derive(Default)]
pub struct PostMemory {
    posts: Vec<Post>,
}

impl PostMemory {
    /// The posts `scan` should carry: what it found, and what earlier scans
    /// found that it neither found again nor ruled out.
    pub fn update(&mut self, scan: &Scan) -> Vec<Post> {
        let found = scan.found_posts();
        self.posts.retain(|p| {
            scan.covers(p.centre)
                && !scan.clears(p)
                && found
                    .iter()
                    .all(|f| f.centre.distance(p.centre) >= POST_SAME)
        });
        self.posts.extend(found);
        self.posts.clone()
    }
}

/// Collision ready for the skate engine: triangles and rails in skate space.
pub struct Collision {
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub rails: Vec<Vec<[f32; 3]>>,
    pub census: crate::rails::RailCensus,
}

impl Collision {
    pub fn from_scan(scan: &Scan) -> Result<Self, String> {
        Self::from_triangles(scan, scan.triangles())
    }

    /// As `from_scan`, keeping the posts earlier scans found.
    pub fn from_scan_remembering(scan: &Scan, memory: &mut PostMemory) -> Result<Self, String> {
        Self::from_triangles(scan, scan.triangles_with(&memory.update(scan)))
    }

    fn from_triangles(scan: &Scan, mut triangles: Vec<[Vec3; 3]>) -> Result<Self, String> {
        let mut seen = HashSet::new();
        triangles.retain(|t| {
            let mut k = t.map(|v| v.to_array().map(|x| (x * 200.).round() as i32));
            k.sort();
            seen.insert(k)
        });
        if triangles.is_empty() {
            return Err("the scan found no ground".into());
        }
        let low = scan.origin;
        let high = scan.origin + Vec2::splat((scan.size - 1) as f32 * scan.spacing);
        let on = |v: f32, edge: f32| (v - edge).abs() < 1e-3;
        let border = |a: Vec3, b: Vec3| {
            (on(a.x, low.x) && on(b.x, low.x))
                || (on(a.x, high.x) && on(b.x, high.x))
                || (on(a.y, low.y) && on(b.y, low.y))
                || (on(a.y, high.y) && on(b.y, high.y))
        };
        let (rails, census) = crate::rails::find_metres(&triangles, border);
        let skate = |p: Vec3| crate::coords::to_skate(p).to_array();
        Ok(Self {
            triangles: triangles.iter().map(|t| t.map(skate)).collect(),
            rails: rails
                .into_iter()
                .map(|r| r.into_iter().map(skate).collect())
                .collect(),
            census,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scan of `size`² samples `spacing` apart from the origin, ground from
    /// `height(x, y)` with straight-up normals, `None` for no ground.
    fn grid(size: usize, spacing: f32, height: impl Fn(f32, f32) -> Option<f32>) -> Scan {
        let mut ground = Vec::new();
        for j in 0..size {
            for i in 0..size {
                let (x, y) = (i as f32 * spacing, j as f32 * spacing);
                match height(x, y) {
                    Some(h) => ground.extend([h, 0., 0., 1.]),
                    None => ground.extend([f32::NEG_INFINITY, 0., 0., 0.]),
                }
            }
        }
        let header = [0., 0., spacing, size as f32, 0., 0., 1., 0.];
        Scan::parse(&header, &ground, &[], &[], &[]).unwrap()
    }

    fn up_area(tris: &[[Vec3; 3]]) -> f32 {
        tris.iter()
            .map(|t| (t[1] - t[0]).cross(t[2] - t[0]))
            .filter(|n| n.z > 0.)
            .map(|n| n.z * 0.5)
            .sum()
    }

    #[test]
    fn flat_ground_is_one_upward_sheet() {
        let scan = grid(5, 0.5, |_, _| Some(0.));
        let tris = scan.triangles();
        assert_eq!(tris.len(), 4 * 4 * 2);
        assert!(tris.iter().all(|t| (t[1] - t[0]).cross(t[2] - t[0]).z > 0.));
        assert!((up_area(&tris) - 4.).abs() < 1e-4);
    }

    #[test]
    fn a_slope_is_continuous_but_a_curb_is_a_step() {
        let mut ramp = Vec::new();
        let normal = Vec3::new(-0.5, 0., 1.).normalize();
        for _j in 0..3 {
            for i in 0..3 {
                ramp.extend([i as f32 * 0.25, normal.x, normal.y, normal.z]);
            }
        }
        let header = [0., 0., 0.5, 3., 0., 0., 1., 0.];
        assert!(Scan::breaks(&header, &ramp).unwrap().is_empty());

        let curb = grid(3, 0.5, |x, _| Some(if x > 0.6 { 0.15 } else { 0. }));
        let header = [0., 0., 0.5, 3., 0., 0., 1., 0.];
        let ground: Vec<f32> = curb
            .ground
            .iter()
            .flat_map(|g| {
                let g = g.unwrap();
                [g.height, 0., 0., 1.]
            })
            .collect();
        let breaks = Scan::breaks(&header, &ground).unwrap();
        // Three rows, each with one +x edge across the curb.
        assert_eq!(breaks.len(), 3 * 2);
        assert!(breaks.chunks(2).all(|b| b[1] == 0.));
    }

    #[test]
    fn a_curb_gets_a_vertical_face_toward_the_road_and_a_rail() {
        // Road at 0, sidewalk 0.15 up for x > 2.1, refined to the exact line.
        let size = 9;
        let spacing = 0.5;
        let mut scan = grid(size, spacing, |x, _| Some(if x > 2.1 { 0.15 } else { 0. }));
        for j in 0..size {
            scan.crossings.insert((scan.index(4, j), Axis::X), 0.2);
        }
        let collision = Collision::from_scan(&scan).unwrap();
        let tris = scan.triangles();
        let faces: Vec<_> = tris
            .iter()
            .map(|t| (t, (t[1] - t[0]).cross(t[2] - t[0]).normalize()))
            .filter(|(_, n)| n.z.abs() < 0.1)
            .collect();
        assert!(!faces.is_empty());
        for (t, n) in &faces {
            assert!(n.x < -0.99, "curb face should face the road, got {n}");
            assert!(t.iter().all(|p| (p.x - 2.1).abs() < 1e-4));
        }
        assert!((up_area(&tris) - 16.).abs() < 1e-3);
        assert_eq!(collision.rails.len(), 1, "{:?}", collision.census);
        let rail = &collision.rails[0];
        let ends = [rail[0], *rail.last().unwrap()]
            .map(|p| crate::coords::from_skate(Vec3::from_array(p)));
        for p in ends {
            assert!((p.x - 2.1).abs() < 0.01 && (p.z - 0.15).abs() < 0.01, "{p}");
        }
        assert!((ends[0].y - ends[1].y).abs() > 3.5);
    }

    #[test]
    fn a_diagonal_ledge_is_watertight_and_grindable() {
        diagonal_ledge(0.);
    }

    /// Bisection finds a step only to a few centimetres.
    #[test]
    fn a_roughly_bisected_ledge_is_still_one_rail() {
        diagonal_ledge(0.06);
    }

    fn diagonal_ledge(noise: f32) {
        let mut wobble = 0.37_f32;
        let mut jitter = move || {
            wobble = (wobble * 9301. + 49297.) % 233280.;
            (wobble / 233280. - 0.5) * noise
        };
        let size = 17;
        let spacing = 0.5;
        // Ledge 0.5 up on the side of x + 0.6 y > 4.
        let high = |x: f32, y: f32| x + 0.6 * y > 4.;
        let mut scan = grid(size, spacing, |x, y| {
            Some(if high(x, y) { 0.5 } else { 0. })
        });
        // Exact crossings, as bisection would find them.
        for j in 0..size {
            for i in 0..size {
                let (x, y) = (i as f32 * spacing, j as f32 * spacing);
                if i + 1 < size && high(x, y) != high(x + spacing, y) {
                    let t = (4. - 0.6 * y - x) / spacing + jitter();
                    scan.crossings
                        .insert((scan.index(i, j), Axis::X), t.clamp(0.02, 0.98));
                }
                if j + 1 < size && high(x, y) != high(x, y + spacing) {
                    let t = ((4. - x) / 0.6 - y) / spacing + jitter();
                    scan.crossings
                        .insert((scan.index(i, j), Axis::Y), t.clamp(0.02, 0.98));
                }
            }
        }
        let tris = scan.triangles();
        let side = (size - 1) as f32 * spacing;
        assert!((up_area(&tris) - side * side).abs() < 1e-2);
        // Every edge of every face is shared or on the patch border.
        let key = |v: Vec3| v.to_array().map(|x| (x * 1000.).round() as i64);
        let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
        for t in &tris {
            for k in 0..3 {
                let (a, b) = (key(t[k]), key(t[(k + 1) % 3]));
                *edges
                    .entry(if a < b { (a, b) } else { (b, a) })
                    .or_default() += 1;
            }
        }
        let border = |v: [i64; 3]| {
            let s = (side * 1000.) as i64;
            v[0] == 0 || v[1] == 0 || v[0] == s || v[1] == s
        };
        for ((a, b), count) in &edges {
            assert!(
                *count == 2 || (border(*a) && border(*b)),
                "open edge {a:?}-{b:?} used {count} times"
            );
        }
        let collision = Collision::from_scan(&scan).unwrap();
        let longest = collision
            .rails
            .iter()
            .map(|r| {
                r.windows(2)
                    .map(|w| Vec3::from_array(w[0]).distance(Vec3::from_array(w[1])))
                    .sum::<f32>()
            })
            .fold(0., f32::max);
        assert!(
            longest > 6.,
            "longest rail {longest} {:?}",
            collision.census
        );
    }

    #[test]
    fn missing_ground_leaves_a_hole_with_a_grindable_edge() {
        let scan = grid(9, 0.5, |x, _| (x < 2.).then_some(0.));
        let tris = scan.triangles();
        assert!(tris.iter().all(|t| t.iter().all(|p| p.x <= 2.0 + 1e-4)));
        let collision = Collision::from_scan(&scan).unwrap();
        assert!(!collision.rails.is_empty());
    }

    #[test]
    fn ring_walls_face_the_skater() {
        let mut scan = grid(9, 0.5, |_, _| Some(0.));
        scan.eye = Vec3::new(2., 2., 2.);
        let n = 64;
        scan.ring = (0..n)
            .map(|k| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                let dir = Vec3::new(a.cos(), a.sin(), 0.);
                // A wall along x = 5 only.
                (dir.x > 0.3).then(|| {
                    let t = 3. / dir.x;
                    WallHit {
                        position: scan.eye + dir * t,
                        normal: -Vec3::X,
                    }
                })
            })
            .collect();
        let tris = scan.triangles();
        let walls: Vec<_> = tris
            .iter()
            .filter(|t| (t[0].x - 5.).abs() < 1e-3 && (t[1].x - 5.).abs() < 1e-3)
            .collect();
        assert!(walls.len() >= 4);
        for t in walls {
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            assert!(n.x < 0.);
            assert!(t.iter().any(|p| p.z <= -1.4) && t.iter().any(|p| p.z >= 3.9));
        }
    }

    /// A ring of hits around a thin pole at (6, 4), `None` elsewhere.
    fn around_pole(scan: &Scan, count: usize) -> Vec<Option<WallHit>> {
        let pole = Vec2::new(6., 4.);
        (0..count)
            .map(|k| {
                let a = k as f32 / count as f32 * std::f32::consts::TAU;
                let dir = Vec2::new(a.cos(), a.sin());
                let to = pole - scan.eye.truncate();
                let along = to.dot(dir);
                (along > 0. && (to - dir * along).length() < 0.06).then(|| WallHit {
                    position: (scan.eye.truncate() + dir * (along - 0.05)).extend(scan.eye.z),
                    normal: (-dir).extend(0.),
                })
            })
            .collect()
    }

    #[test]
    fn a_pole_the_grid_missed_becomes_a_post() {
        let mut scan = grid(17, 0.5, |_, _| Some(0.));
        scan.eye = Vec3::new(4., 4., 0.5);
        scan.low_ring = around_pole(&scan, 256);
        assert!(scan.low_ring.iter().flatten().count() >= 1);
        let tris = scan.triangles();
        let post: Vec<_> = tris
            .iter()
            .filter(|t| {
                t.iter()
                    .all(|p| p.truncate().distance(Vec2::new(6., 4.)) < 0.5)
            })
            .collect();
        assert!(!post.is_empty(), "no post at the pole");
        // The face toward the skater faces the skater, and it reaches up.
        assert!(post.iter().any(|t| {
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            n.x < 0. && n.z.abs() < 1e-3 && t.iter().any(|p| p.z > 2.)
        }));
    }

    #[test]
    fn an_obstacle_the_grid_saw_gets_no_post() {
        // A 0.5 m bench from x = 5 to 7: knee-height casts hit its side,
        // and its top is already in the grid.
        let mut scan = grid(17, 0.5, |x, y| {
            Some(if (x - 6.).abs() < 1. && (y - 4.).abs() < 1. {
                0.5
            } else {
                0.
            })
        });
        scan.eye = Vec3::new(4., 4., 0.3);
        scan.low_ring = (0..256)
            .map(|k| {
                let a = k as f32 / 256. * std::f32::consts::TAU;
                let dir = Vec2::new(a.cos(), a.sin());
                let along = 1. / dir.x;
                let at = scan.eye.truncate() + dir * along;
                (dir.x > 0. && (at.y - 4.).abs() < 1.).then(|| WallHit {
                    position: at.extend(scan.eye.z),
                    normal: -Vec3::X,
                })
            })
            .collect();
        assert!(scan.low_ring.iter().flatten().count() > 10);
        let tall = scan
            .triangles()
            .iter()
            .filter(|t| t.iter().any(|p| p.z > 1.))
            .count();
        assert_eq!(tall, 0);
    }

    #[test]
    fn a_pole_is_remembered_until_a_cast_passes_through_it() {
        let mut memory = PostMemory::default();
        let at_pole = |p: &[Vec3; 3]| {
            p.iter()
                .all(|v| v.truncate().distance(Vec2::new(6., 4.)) < 0.5)
        };
        let mut scan = grid(17, 0.5, |_, _| Some(0.));
        scan.eye = Vec3::new(4., 4., 0.5);
        scan.low_ring = around_pole(&scan, 256);
        assert!(
            scan.triangles_with(&memory.update(&scan))
                .iter()
                .any(at_pole)
        );

        // Seen from elsewhere, no cast lines up with the pole: it stays.
        scan.eye = Vec3::new(2., 1., 0.5);
        scan.low_ring = vec![None; 16];
        assert!(
            scan.triangles_with(&memory.update(&scan))
                .iter()
                .any(at_pole)
        );

        // A cast runs straight through where it stood: it is gone.
        scan.eye = Vec3::new(4., 4., 0.5);
        scan.low_ring = vec![None; 256];
        assert!(
            !scan
                .triangles_with(&memory.update(&scan))
                .iter()
                .any(at_pole)
        );
    }

    /// The skate engine's own collision and grind-world builders accept
    /// what the mesher makes.
    #[test]
    fn the_skate_engine_accepts_scanned_worlds() {
        let builder = skate_host::bridge::CollisionBuilder::standalone();
        let mut street = grid(33, 0.5, |x, y| {
            if (x - 8.).powi(2) + (y - 8.).powi(2) < 4. {
                None
            } else if x + 0.6 * y > 9. {
                Some(0.15 + 0.1 * (x * 0.3).sin())
            } else {
                Some(0.02 * y)
            }
        });
        street.eye = Vec3::new(4., 4., 2.);
        street.ring = (0..64)
            .map(|k| {
                let a = k as f32 / 64. * std::f32::consts::TAU;
                let dir = Vec3::new(a.cos(), a.sin(), 0.);
                (dir.y < -0.5).then(|| WallHit {
                    position: street.eye + dir * (3. / -dir.y),
                    normal: Vec3::Y,
                })
            })
            .collect();
        street.low_ring = around_pole(&street, 256);
        assert!(street.triangles().iter().any(|t| {
            t.iter()
                .all(|p| p.truncate().distance(Vec2::new(6., 4.)) < 0.5)
        }));
        let collision = Collision::from_scan(&street).unwrap();
        assert!(collision.triangles.len() > 1500);
        assert!(!collision.rails.is_empty(), "{:?}", collision.census);
        builder
            .build(collision.triangles, collision.rails)
            .unwrap_or_else(|e| panic!("skate engine refused the scan: {e}"));
    }

    #[test]
    fn malformed_scans_are_refused() {
        assert!(Scan::parse(&[0.; 7], &[], &[], &[], &[]).is_err());
        assert!(Scan::parse(&[0., 0., 0.5, 3., 0., 0., 0., 0.], &[0.; 35], &[], &[], &[]).is_err());
        assert!(Scan::parse(&[0., 0., 0.5, 1e9, 0., 0., 0., 0.], &[], &[], &[], &[]).is_err());
        assert!(
            Scan::parse(
                &[f32::NAN, 0., 0.5, 2., 0., 0., 0., 0.],
                &[0.; 16],
                &[],
                &[],
                &[]
            )
            .is_err()
        );
    }
}
