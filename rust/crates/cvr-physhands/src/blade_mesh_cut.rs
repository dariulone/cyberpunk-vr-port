//! main's src/Anim/BladeMeshCut.cpp: the geometry of a blade's mark on a mesh -- the cut's volume (a box along the
//! stroke, on the surface's normal), a triangle split against it into the faces left outside and the faces it covers
//! (both in barycentric coordinates of the triangle), the sweep's sampled rays and where they meet a triangle, and the
//! stamp's surface point a confirmed contact lands on.
pub type Point = [f64; 3];
pub type Triangle = [Point; 3];
pub type Ray = [Point; 2];
pub type SweepSamples = [Ray; 3];

fn delta(a: &Point, b: &Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: &Point, b: &Point) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn normalize(v: &mut Point) -> bool {
    let l = dot(v, v).sqrt();
    if !(l > 1e-8) || !l.is_finite() {
        return false;
    }
    for e in v.iter_mut() {
        *e /= l;
    }
    true
}
fn cross(a: &Point, b: &Point) -> Point {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// A surface point a stamp lands on: the point, its face's normal, its squared distance from the seed; found, on an
/// exact intersection of the blade, on the finite segment.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceHit {
    pub point: Point,
    pub normal: Point,
    pub seed_distance_squared: f64,
    pub found: bool,
    pub exact: bool,
    pub on_segment: bool,
}
impl Default for SurfaceHit {
    fn default() -> Self {
        SurfaceHit { point: [0.0; 3], normal: [0.0; 3], seed_distance_squared: 0.16 * 0.16, found: false, exact: false, on_segment: false }
    }
}
/// A cut: from, to, the surface normal, its half width, its depth into the surface and how far it reaches outside.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cut {
    pub from: Point,
    pub to: Point,
    pub normal: Point,
    pub half_width: f64,
    pub depth: f64,
    pub outside: f64,
}
/// The cut's box: its centre, its axes (along, across, the normal) and half extents.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Volume {
    pub center: Point,
    pub axes: Triangle,
    pub extent: Point,
    pub outside: f64,
}
#[derive(Clone, Copy)]
struct Polygon {
    vertices: [Point; 16],
    count: usize,
}
impl Polygon {
    fn new() -> Polygon {
        Polygon { vertices: [[0.0; 3]; 16], count: 0 }
    }
    fn add(&mut self, p: Point) {
        if self.count < 16 {
            self.vertices[self.count] = p;
            self.count += 1;
        }
    }
}

/// SampleSweep: the blade's segment at its start, middle and end of the motion.
pub fn sample_sweep(endpoints: &[Point; 4]) -> SweepSamples {
    let mut rays = [[endpoints[0], endpoints[1]], [[0.0; 3]; 2], [endpoints[2], endpoints[3]]];
    for end in 0..2 {
        for axis in 0..3 {
            rays[1][end][axis] = (rays[0][end][axis] + rays[2][end][axis]) * 0.5;
        }
    }
    rays
}
/// ResolveSweep: the cut along the sampled rays' hits (the middle one required; an entry or exit joined to it only
/// from its confirmed half), its normal from the end hits' normals.
pub fn resolve_sweep(samples: &SweepSamples, fractions: &[f64; 3], normals: &Triangle, cut: &mut Cut) -> bool {
    let hit = |i: usize| (0.0..=1.0).contains(&fractions[i]);
    if !hit(1) {
        return false;
    }
    let first = if hit(0) { 0 } else { 1 };
    let last = if hit(2) { 2 } else { 1 };
    if first == last || dot(&normals[first], &normals[last]) < 0.4 || dot(&normals[first], &normals[1]) < 0.4 || dot(&normals[last], &normals[1]) < 0.4 {
        return false;
    }
    let point = |i: usize| {
        let mut p = samples[i][0];
        for axis in 0..3 {
            p[axis] += fractions[i] * (samples[i][1][axis] - p[axis]);
        }
        p
    };
    cut.from = point(first);
    cut.to = point(last);
    let span = delta(&cut.to, &cut.from);
    if !(dot(&span, &span) <= 0.15 * 0.15) {
        return false;
    }
    for axis in 0..3 {
        cut.normal[axis] = normals[first][axis] + normals[last][axis];
    }
    true
}
/// MakeVolume: the cut's box (1 mm to 1.5 m long, at most 10 cm half width, 1.5 m deep, 20 cm outside).
pub fn make_volume(cut: &Cut, volume: &mut Volume) -> bool {
    let mut tangent = delta(&cut.to, &cut.from);
    let mut normal = cut.normal;
    let length = dot(&tangent, &tangent).sqrt();
    if !(length > 0.001 && length <= 1.5 && cut.half_width > 0.0 && cut.half_width <= 0.1 && cut.depth > 0.0 && cut.depth <= 1.5 && cut.outside >= 0.0 && cut.outside <= 0.2)
        || !normalize(&mut tangent)
        || !normalize(&mut normal)
    {
        return false;
    }
    let mut side = cross(&normal, &tangent);
    if !normalize(&mut side) {
        return false;
    }
    normal = cross(&tangent, &side);
    for axis in 0..3 {
        volume.center[axis] = (cut.from[axis] + cut.to[axis]) * 0.5 + normal[axis] * (cut.outside - cut.depth) * 0.5;
        if !volume.center[axis].is_finite() {
            return false;
        }
    }
    volume.axes = [tangent, side, normal];
    volume.extent = [length * 0.5, cut.half_width, (cut.depth + cut.outside) * 0.5];
    volume.outside = cut.outside;
    true
}
fn area(a: &Point, b: &Point, c: &Point) -> f64 {
    (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1])
}
/// Subtract: the triangle against the box (its six planes; a diamond's four more in the box's own plane): the faces
/// left outside, and the ones it covers, barycentric on the triangle. False when it does not overlap.
pub fn subtract(positions: &Triangle, volume: &Volume, faces: &mut Vec<Triangle>, mut covered: Option<&mut Vec<Triangle>>, diamond: bool) -> bool {
    faces.clear();
    if let Some(c) = covered.as_deref_mut() {
        c.clear();
    }
    let mut distances = [[0f64; 3]; 10];
    let plane_count = if diamond { 10 } else { 6 };
    for plane in 0..6 {
        let axis = plane / 2;
        for v in 0..3 {
            let coord = dot(&delta(&positions[v], &volume.center), &volume.axes[axis]);
            if !coord.is_finite() {
                return false;
            }
            distances[plane][v] = volume.extent[axis] + if plane % 2 == 1 { coord } else { -coord };
        }
        if distances[plane].iter().cloned().fold(f64::NEG_INFINITY, f64::max) <= 0.0 {
            return false;
        }
    }
    for plane in 6..plane_count {
        for v in 0..3 {
            let rel = delta(&positions[v], &volume.center);
            let x = dot(&rel, &volume.axes[0]) / volume.extent[0];
            let y = dot(&rel, &volume.axes[1]) / volume.extent[1];
            let k = plane - 6;
            distances[plane][v] = 1.0 - (if k & 1 != 0 { x } else { -x }) - (if k & 2 != 0 { y } else { -y });
        }
    }
    let mut inside = Polygon::new();
    inside.add([1.0, 0.0, 0.0]);
    inside.add([0.0, 1.0, 0.0]);
    inside.add([0.0, 0.0, 1.0]);
    let mut outside = [Polygon::new(); 10];
    for plane in 0..plane_count {
        let mut next = Polygon::new();
        let d = &distances[plane];
        for index in 0..inside.count {
            let first = inside.vertices[index];
            let second = inside.vertices[(index + 1) % inside.count];
            let from = dot(&first, d);
            let to = dot(&second, d);
            if from >= 0.0 {
                next.add(first);
            }
            if from <= 0.0 {
                outside[plane].add(first);
            }
            if (from < 0.0 && to > 0.0) || (from > 0.0 && to < 0.0) {
                let ratio = from / (from - to);
                let mut edge = [0.0; 3];
                for axis in 0..3 {
                    edge[axis] = first[axis] + ratio * (second[axis] - first[axis]);
                }
                next.add(edge);
                outside[plane].add(edge);
            }
        }
        if next.count < 3 {
            return false;
        }
        inside = next;
    }
    let mut removed = 0.0;
    for i in 1..inside.count - 1 {
        removed += area(&inside.vertices[0], &inside.vertices[i], &inside.vertices[i + 1]);
    }
    if !(removed > 1e-9) {
        return false;
    }
    if let Some(c) = covered {
        for i in 1..inside.count - 1 {
            if area(&inside.vertices[0], &inside.vertices[i], &inside.vertices[i + 1]) > 1e-10 {
                c.push([inside.vertices[0], inside.vertices[i], inside.vertices[i + 1]]);
            }
        }
    }
    for polygon in &outside {
        if polygon.count < 3 {
            continue;
        }
        for i in 1..polygon.count - 1 {
            if area(&polygon.vertices[0], &polygon.vertices[i], &polygon.vertices[i + 1]) > 1e-10 {
                faces.push([polygon.vertices[0], polygon.vertices[i], polygon.vertices[i + 1]]);
            }
        }
    }
    true
}
fn interpolate(source: &Triangle, weights: &Point) -> Point {
    let mut p = [0.0; 3];
    for i in 0..3 {
        for j in 0..3 {
            p[j] += weights[i] * source[i][j];
        }
    }
    p
}
/// MarkFace: the faces outside the footprint and the ink it covers; an open seam leaves a slit (40 % of its width)
/// unpainted down the middle.
pub fn mark_face(face: &Triangle, footprint: &Volume, open_seam: bool, outside: &mut Vec<Triangle>, ink: &mut Vec<Triangle>, diamond: bool) -> bool {
    if !subtract(face, footprint, outside, Some(ink), diamond) {
        return false;
    }
    if !open_seam {
        return true;
    }
    let mut slit = *footprint;
    slit.extent[1] *= 0.40;
    let covered = std::mem::take(ink);
    let mut remaining = Vec::new();
    for polygon in &covered {
        let positions = [interpolate(face, &polygon[0]), interpolate(face, &polygon[1]), interpolate(face, &polygon[2])];
        if !subtract(&positions, &slit, &mut remaining, None, false) {
            ink.push(*polygon);
            continue;
        }
        for piece in &remaining {
            ink.push([interpolate(polygon, &piece[0]), interpolate(polygon, &piece[1]), interpolate(polygon, &piece[2])]);
        }
    }
    true
}
/// SegmentDistance: between two strokes' centres.
pub fn segment_distance(a: &Volume, b: &Volume) -> f64 {
    let d = delta(&a.center, &b.center);
    dot(&d, &d).sqrt()
}
/// ProjectSurface: the point moved along the normal onto the triangle (at most 12 cm in, 2 cm out, nearer than
/// `distance`): its travel and the barycentric weights.
pub fn project_surface(point: &Point, normal: &Point, face: &Triangle, distance: &mut f64, weights: &mut Point) -> bool {
    let first = delta(&face[1], &face[0]);
    let second = delta(&face[2], &face[0]);
    let perpendicular = cross(normal, &second);
    let determinant = dot(&first, &perpendicular);
    if determinant.abs() < 1e-11 {
        return false;
    }
    let d = delta(point, &face[0]);
    let c = cross(&d, &first);
    let u = dot(&d, &perpendicular) / determinant;
    let v = dot(normal, &c) / determinant;
    if !(u >= -1e-7 && v >= -1e-7 && u + v <= 1.000_000_1) {
        return false;
    }
    let travel = dot(&second, &c) / determinant;
    if !((-0.12..=0.02).contains(&travel)) || travel.abs() >= distance.abs() {
        return false;
    }
    *distance = travel;
    *weights = [1.0 - u - v, u, v];
    true
}
/// SegmentHit: the segment's first crossing of the triangle (nearer than `fraction`), the face's normal against it.
pub fn segment_hit(from: &Point, to: &Point, face: &Triangle, fraction: &mut f64, normal: &mut Point) -> bool {
    let ray = delta(to, from);
    let left = delta(&face[1], &face[0]);
    let right = delta(&face[2], &face[0]);
    let side = cross(&ray, &right);
    let det = dot(&left, &side);
    if det.abs() < 1e-12 {
        return false;
    }
    let offset = delta(from, &face[0]);
    let c = cross(&offset, &left);
    let u = dot(&offset, &side) / det;
    let v = dot(&ray, &c) / det;
    if !(u >= -1e-8 && v >= -1e-8 && u + v <= 1.000_000_01) {
        return false;
    }
    let t = dot(&right, &c) / det;
    if !((0.0..=1.0).contains(&t) && t < *fraction) {
        return false;
    }
    let mut candidate = cross(&left, &right);
    if !normalize(&mut candidate) {
        return false;
    }
    if dot(&candidate, &ray) > 0.0 {
        for v in candidate.iter_mut() {
            *v = -*v;
        }
    }
    *fraction = t;
    *normal = candidate;
    true
}
/// ClosestSurfacePoint: the nearest point of the closed triangle (a collapsed one refused).
pub fn closest_surface_point(face: &Triangle, seed: &Point, point: &mut Point, normal: &mut Point) -> bool {
    *normal = cross(&delta(&face[1], &face[0]), &delta(&face[2], &face[0]));
    if !normalize(normal) {
        return false;
    }
    let distance = dot(&delta(seed, &face[0]), normal);
    for k in 0..3 {
        point[k] = seed[k] - distance * normal[k];
    }
    let mut inside = true;
    let mut closest = [0.0; 3];
    let mut best = f64::INFINITY;
    for i in 0..3 {
        let edge = delta(&face[(i + 1) % 3], &face[i]);
        let relative = delta(point, &face[i]);
        if dot(&cross(&edge, &relative), normal) < 0.0 {
            inside = false;
        }
        let t = (dot(&relative, &edge) / dot(&edge, &edge)).clamp(0.0, 1.0);
        let mut candidate = face[i];
        for k in 0..3 {
            candidate[k] += t * edge[k];
        }
        let diff = delta(&candidate, point);
        let gap = dot(&diff, &diff);
        if gap < best {
            best = gap;
            closest = candidate;
        }
    }
    if !inside {
        *point = closest;
    }
    true
}
fn lerp(ray: &Ray, t: f64) -> Point {
    let mut p = [0.0; 3];
    for k in 0..3 {
        p[k] = ray[0][k] + t * (ray[1][k] - ray[0][k]);
    }
    p
}
/// LocateStamp: where on this face the confirmed contact lands -- an exact crossing of the blade's sampled rays or of
/// the sweep's surface, else the native entry projected along its normal onto a facing triangle (within the shell's
/// depth), else a point of the segment projected at most 15 mm; with skin weights only an exact crossing with a
/// representable influence of the region.
#[allow(clippy::too_many_arguments)]
pub fn locate_stamp(rays: &SweepSamples, seed: &Point, contact_normal: &Point, face: &Triangle, hit: &mut SurfaceHit, shell_depth: f64, shading_normal: Option<&Point>, blend_weights: Option<&Point>) {
    const MARGIN: f64 = 0.015;
    let mut near_sweep = true;
    for k in 0..3 {
        let low = face[0][k].min(face[1][k]).min(face[2][k]);
        let high = face[0][k].max(face[1][k]).max(face[2][k]);
        if low > seed[k] + 0.16 || high < seed[k] - 0.16 {
            return;
        }
        let rmax = rays[0][0][k].max(rays[0][1][k]).max(rays[2][0][k]).max(rays[2][1][k]);
        let rmin = rays[0][0][k].min(rays[0][1][k]).min(rays[2][0][k]).min(rays[2][1][k]);
        if low > rmax + MARGIN || high < rmin - MARGIN {
            near_sweep = false;
        }
    }
    if blend_weights.is_some() && !near_sweep {
        return;
    }
    let edge1 = delta(&face[1], &face[0]);
    let edge2 = delta(&face[2], &face[0]);
    let area = cross(&edge1, &edge2);
    let mut normal = area;
    if !normalize(&mut normal) {
        return;
    }
    let flip = match shading_normal {
        Some(sn) => dot(&normal, sn) < 0.0,
        None => dot(&normal, &delta(&rays[2][1], &rays[2][0])) > 0.0,
    };
    if flip {
        for n in normal.iter_mut() {
            *n = -*n;
        }
    }
    let mut consider = |p: Point, exact: bool, on_segment: bool, hit: &mut SurfaceHit| {
        let diff = delta(&p, seed);
        let d = dot(&diff, &diff);
        if d > 0.16 * 0.16 || (hit.exact && !exact) || (hit.on_segment && !on_segment) || (hit.found && hit.exact == exact && hit.on_segment == on_segment && d >= hit.seed_distance_squared) {
            return;
        }
        if let Some(bw) = blend_weights {
            // the skin influence at the intersection: a far vertex of the same triangle can be another bone's
            let rel = delta(&p, &face[0]);
            let inv = 1.0 / dot(&area, &area);
            let u = dot(&cross(&rel, &edge2), &area) * inv;
            let v = dot(&cross(&edge1, &rel), &area) * inv;
            if dot(&[1.0 - u - v, u, v], bw) < 1.0 {
                return;
            }
        }
        *hit = SurfaceHit { point: p, normal, seed_distance_squared: d, found: true, exact, on_segment };
    };
    let intersect = |ray: &Ray, tri: &Triangle, hit: &mut SurfaceHit, consider: &mut dyn FnMut(Point, bool, bool, &mut SurfaceHit)| {
        let mut t = 2.0;
        let mut ignored = [0.0; 3];
        if segment_hit(&ray[0], &ray[1], tri, &mut t, &mut ignored) {
            consider(lerp(ray, t), true, true, hit);
        }
    };
    // the native entry can lie on an inflated collision shell: its tangent coordinates kept on a facing triangle of
    // the same region; a confirmed blade intersection takes priority
    if blend_weights.is_none() && !hit.on_segment && dot(contact_normal, &normal) > 0.4 {
        let mut depth = 1.0;
        let mut weights = [0.0; 3];
        if project_surface(seed, contact_normal, face, &mut depth, &mut weights) && (shell_depth <= 0.0 || -depth < shell_depth) {
            let mut surface = *seed;
            for k in 0..3 {
                surface[k] += contact_normal[k] * depth;
            }
            consider(surface, false, false, hit);
        } else if shell_depth > 0.0 {
            // a seam or a mask can end the face beside the shell ray: only its near edge within the graze margin
            let (mut point, mut unused) = ([0.0; 3], [0.0; 3]);
            if closest_surface_point(face, seed, &mut point, &mut unused) {
                let d = delta(&point, seed);
                let along = dot(&d, contact_normal);
                if (-0.12..=0.02).contains(&along) && -along < shell_depth && dot(&d, &d) - along * along <= MARGIN * MARGIN {
                    consider(point, false, false, hit);
                }
            }
        }
    }
    if !near_sweep {
        return;
    }
    for ray in rays {
        intersect(ray, face, hit, &mut consider);
    }
    // mesh edges can cross the motion between the sampled segments
    let sweep = [[rays[0][0], rays[0][1], rays[2][1]], [rays[0][0], rays[2][1], rays[2][0]]];
    for tri in &sweep {
        for v in 0..3 {
            intersect(&[face[v], face[(v + 1) % 3]], tri, hit, &mut consider);
            intersect(&[tri[v], tri[(v + 1) % 3]], face, hit, &mut consider);
        }
    }
    if blend_weights.is_some() || hit.exact {
        return;
    }
    // a shallow hit on the collision shell can precede the render surface: a point of the segment moved 15 mm at most
    for ray in rays {
        let dir = delta(&ray[1], &ray[0]);
        let length2 = dot(&dir, &dir);
        if length2 < 1e-12 {
            continue;
        }
        let p = lerp(ray, (dot(&delta(seed, &ray[0]), &dir) / length2).clamp(0.0, 1.0));
        let mut gap = MARGIN;
        let mut weights = [0.0; 3];
        if project_surface(&p, &normal, face, &mut gap, &mut weights) {
            let mut surface = p;
            for k in 0..3 {
                surface[k] += gap * normal[k];
            }
            consider(surface, false, true, hit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn flat() -> Triangle {
        [[-0.1, -0.1, 0.0], [0.1, -0.1, 0.0], [0.0, 0.1, 0.0]]
    }
    #[test]
    fn volume_and_subtract() {
        let cut = Cut { from: [-0.02, 0.0, 0.0], to: [0.02, 0.0, 0.0], normal: [0.0, 0.0, 1.0], half_width: 0.005, depth: 0.004, outside: 0.004 };
        let mut v = Volume::default();
        assert!(make_volume(&cut, &mut v));
        assert!((v.extent[0] - 0.02).abs() < 1e-12);
        let (mut faces, mut ink) = (Vec::new(), Vec::new());
        assert!(subtract(&flat(), &v, &mut faces, Some(&mut ink), false));
        assert!(!faces.is_empty() && !ink.is_empty());
        // the covered area is the box's footprint (4 cm x 1 cm) of the triangle's 0.02 m^2
        let a = |t: &Triangle| {
            let p = [interpolate(&flat(), &t[0]), interpolate(&flat(), &t[1]), interpolate(&flat(), &t[2])];
            let c = cross(&delta(&p[1], &p[0]), &delta(&p[2], &p[0]));
            dot(&c, &c).sqrt() * 0.5
        };
        let covered: f64 = ink.iter().map(a).sum();
        assert!((covered - 0.04 * 0.01).abs() < 1e-9, "{covered}");
        let left: f64 = faces.iter().map(a).sum();
        assert!((left + covered - 0.02).abs() < 1e-9);
    }
    #[test]
    fn no_overlap() {
        let cut = Cut { from: [0.5, 0.0, 0.0], to: [0.54, 0.0, 0.0], normal: [0.0, 0.0, 1.0], half_width: 0.005, depth: 0.004, outside: 0.004 };
        let mut v = Volume::default();
        assert!(make_volume(&cut, &mut v));
        let mut faces = Vec::new();
        assert!(!subtract(&flat(), &v, &mut faces, None, false));
    }
    #[test]
    fn seam_and_hits() {
        let cut = Cut { from: [-0.02, 0.0, 0.0], to: [0.02, 0.0, 0.0], normal: [0.0, 0.0, 1.0], half_width: 0.005, depth: 0.004, outside: 0.004 };
        let mut v = Volume::default();
        make_volume(&cut, &mut v);
        let (mut out, mut ink) = (Vec::new(), Vec::new());
        assert!(mark_face(&flat(), &v, true, &mut out, &mut ink, false));
        assert!(ink.len() >= 2);
        let (mut t, mut n) = (2.0, [0.0; 3]);
        assert!(segment_hit(&[0.0, 0.0, 0.1], &[0.0, 0.0, -0.1], &flat(), &mut t, &mut n));
        assert!((t - 0.5).abs() < 1e-12 && n[2] > 0.99);
        let rays = sample_sweep(&[[-0.01, 0.0, 0.05], [-0.01, 0.0, -0.05], [0.01, 0.0, 0.05], [0.01, 0.0, -0.05]]);
        let mut hit = SurfaceHit::default();
        locate_stamp(&rays, &[0.0, 0.0, 0.0], &[0.0, 0.0, 1.0], &flat(), &mut hit, 0.0, None, None);
        assert!(hit.found && hit.exact && hit.point[2].abs() < 1e-9);
        let (mut p, mut nn) = ([0.0; 3], [0.0; 3]);
        assert!(closest_surface_point(&flat(), &[0.5, 0.0, 0.2], &mut p, &mut nn));
        assert!(p[2].abs() < 1e-12 && p[0] <= 0.1);
    }
}
