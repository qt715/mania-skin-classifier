//! Small binary-image kernels replacing the OpenCV calls used by the Python reference:
//! area/bilinear resize, connected components, morphological closing, hole filling and
//! scanline polygon filling. Images are row-major `u8` with 0 = background, 255 = foreground.

#[derive(Clone)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Gray {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            data: vec![0; w * h],
        }
    }
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.w + x]
    }
    pub fn count(&self) -> usize {
        self.data.iter().filter(|&&v| v != 0).count()
    }
    /// Bounding box (x0, y0, x1, y1) inclusive of non-zero pixels.
    pub fn bbox(&self) -> Option<(usize, usize, usize, usize)> {
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for y in 0..self.h {
            let row = &self.data[y * self.w..(y + 1) * self.w];
            if let Some(first) = row.iter().position(|&v| v != 0) {
                let last = row.iter().rposition(|&v| v != 0).unwrap();
                x0 = x0.min(first);
                x1 = x1.max(last);
                y0 = y0.min(y);
                y1 = y;
            }
        }
        (x0 != usize::MAX).then_some((x0, y0, x1, y1))
    }
    pub fn crop(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Gray {
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut out = Gray::new(w, h);
        for y in 0..h {
            out.data[y * w..(y + 1) * w]
                .copy_from_slice(&self.data[(y + y0) * self.w + x0..(y + y0) * self.w + x0 + w]);
        }
        out
    }
    pub fn threshold(&mut self, t: u8) {
        self.data
            .iter_mut()
            .for_each(|v| *v = if *v > t { 255 } else { 0 });
    }
    pub fn pad(&self, k: usize) -> Gray {
        let mut out = Gray::new(self.w + 2 * k, self.h + 2 * k);
        for y in 0..self.h {
            let d = (y + k) * out.w + k;
            out.data[d..d + self.w].copy_from_slice(&self.data[y * self.w..(y + 1) * self.w]);
        }
        out
    }
}

/// OpenCV INTER_AREA weights for one axis (`computeResizeAreaTab`), as (dst, src, weight).
fn area_tab(ssize: usize, dsize: usize) -> Vec<(usize, usize, f64)> {
    let scale = ssize as f64 / dsize as f64;
    let mut tab = Vec::new();
    for dx in 0..dsize {
        let fsx1 = dx as f64 * scale;
        let fsx2 = fsx1 + scale;
        let cell = scale.min(ssize as f64 - fsx1);
        let mut sx1 = fsx1.ceil() as isize;
        let sx2 = (fsx2.floor() as isize).min(ssize as isize - 1);
        sx1 = sx1.min(sx2);
        if sx1 as f64 - fsx1 > 1e-3 {
            tab.push((dx, (sx1 - 1) as usize, (sx1 as f64 - fsx1) / cell));
        }
        for sx in sx1..sx2 {
            tab.push((dx, sx as usize, 1.0 / cell));
        }
        if fsx2 - sx2 as f64 > 1e-3 {
            tab.push((
                dx,
                sx2 as usize,
                (fsx2 - sx2 as f64).min(1.0).min(cell) / cell,
            ));
        }
    }
    tab
}

/// cv2.resize(INTER_AREA) for downscaling (both factors >= 1).
pub fn resize_area(src: &Gray, dw: usize, dh: usize) -> Gray {
    let xt = area_tab(src.w, dw);
    let yt = area_tab(src.h, dh);
    // Horizontal pass into f64 rows, then vertical accumulation.
    let mut hrows = vec![0f64; dw * src.h];
    for y in 0..src.h {
        let s = &src.data[y * src.w..(y + 1) * src.w];
        let d = &mut hrows[y * dw..(y + 1) * dw];
        for &(dx, sx, a) in &xt {
            d[dx] += s[sx] as f64 * a;
        }
    }
    let mut acc = vec![0f64; dw * dh];
    for &(dy, sy, b) in &yt {
        let s = &hrows[sy * dw..(sy + 1) * dw];
        let d = &mut acc[dy * dw..(dy + 1) * dw];
        for x in 0..dw {
            d[x] += s[x] * b;
        }
    }
    Gray {
        w: dw,
        h: dh,
        data: acc
            .into_iter()
            .map(|v| v.round_ties_even().clamp(0.0, 255.0) as u8)
            .collect(),
    }
}

/// cv2.resize(INTER_LINEAR) with OpenCV's 11-bit fixed-point coefficients.
pub fn resize_linear(src: &Gray, dw: usize, dh: usize) -> Gray {
    fn coefs(ssize: usize, dsize: usize) -> Vec<(usize, usize, i64, i64)> {
        let scale = ssize as f64 / dsize as f64;
        (0..dsize)
            .map(|d| {
                let f = (d as f64 + 0.5) * scale - 0.5;
                let mut s = f.floor() as isize;
                let mut fx = f - s as f64;
                if s < 0 {
                    s = 0;
                    fx = 0.0;
                }
                if s as usize >= ssize - 1 {
                    s = ssize as isize - 1;
                    fx = 0.0;
                }
                let c0 = ((1.0 - fx) * 2048.0).round() as i64;
                let s0 = s as usize;
                (s0, (s0 + 1).min(ssize - 1), c0, 2048 - c0)
            })
            .collect()
    }
    let xc = coefs(src.w, dw);
    let yc = coefs(src.h, dh);
    let mut out = Gray::new(dw, dh);
    let hrow = |y: usize| -> Vec<i64> {
        let s = &src.data[y * src.w..(y + 1) * src.w];
        xc.iter()
            .map(|&(a, b, ca, cb)| s[a] as i64 * ca + s[b] as i64 * cb)
            .collect()
    };
    for (dy, &(y0, y1, b0, b1)) in yc.iter().enumerate() {
        let (r0, r1) = (hrow(y0), hrow(y1));
        for x in 0..dw {
            let v = (r0[x] * b0 + r1[x] * b1 + (1 << 21)) >> 22;
            out.data[dy * dw + x] = v.clamp(0, 255) as u8;
        }
    }
    out
}

/// 8-connected component labels (0 = background) and per-label areas (index 0 unused).
pub fn components(img: &Gray) -> (Vec<u32>, Vec<usize>) {
    let (w, h) = (img.w, img.h);
    let mut labels = vec![0u32; w * h];
    let mut areas = vec![0usize];
    let mut stack = Vec::new();
    for start in 0..w * h {
        if img.data[start] == 0 || labels[start] != 0 {
            continue;
        }
        let id = areas.len() as u32;
        let mut area = 0;
        labels[start] = id;
        stack.push(start);
        while let Some(p) = stack.pop() {
            area += 1;
            let (x, y) = ((p % w) as isize, (p / w) as isize);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                        continue;
                    }
                    let q = ny as usize * w + nx as usize;
                    if img.data[q] != 0 && labels[q] == 0 {
                        labels[q] = id;
                        stack.push(q);
                    }
                }
            }
        }
        areas.push(area);
    }
    (labels, areas)
}

/// cv2.getStructuringElement(MORPH_ELLIPSE, (2k+1, 2k+1)) as per-row half-widths.
fn ellipse_kernel(k: usize) -> Vec<(isize, isize)> {
    let r = k as f64;
    (0..=2 * k)
        .map(|i| {
            let dy = i as f64 - r;
            let dx = if dy.abs() <= r {
                (r * ((r * r - dy * dy) / (r * r)).sqrt()).round_ties_even() as isize
            } else {
                -1
            };
            (i as isize - k as isize, dx)
        })
        .collect()
}

fn morph(img: &Gray, k: usize, dilate: bool) -> Gray {
    let kernel = ellipse_kernel(k);
    let (w, h) = (img.w as isize, img.h as isize);
    let mut out = Gray::new(img.w, img.h);
    for y in 0..h {
        for x in 0..w {
            // Out-of-image pixels are ignored (OpenCV's default constant border for morphology).
            let mut v = if dilate { 0u8 } else { 255u8 };
            'k: for &(dy, half) in &kernel {
                let yy = y + dy;
                if half < 0 || yy < 0 || yy >= h {
                    continue;
                }
                for xx in (x - half).max(0)..=(x + half).min(w - 1) {
                    let p = img.data[(yy * w + xx) as usize];
                    if dilate && p > v {
                        v = p;
                        if v == 255 {
                            break 'k;
                        }
                    } else if !dilate && p < v {
                        v = p;
                        if v == 0 {
                            break 'k;
                        }
                    }
                }
            }
            out.data[(y * w + x) as usize] = v;
        }
    }
    out
}

/// cv2.morphologyEx(MORPH_CLOSE) with an elliptical (2k+1) kernel.
pub fn close(img: &Gray, k: usize) -> Gray {
    morph(&morph(img, k, true), k, false)
}

/// Fill holes: everything not 4-connected to the border through background becomes
/// foreground (equivalent to drawing filled external contours).
pub fn fill_holes(img: &Gray) -> Gray {
    let (w, h) = (img.w, img.h);
    let mut outside = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    let seed = |p: usize, stack: &mut Vec<usize>, outside: &mut Vec<bool>| {
        if img.data[p] == 0 && !outside[p] {
            outside[p] = true;
            stack.push(p);
        }
    };
    for x in 0..w {
        seed(x, &mut stack, &mut outside);
        seed((h - 1) * w + x, &mut stack, &mut outside);
    }
    for y in 0..h {
        seed(y * w, &mut stack, &mut outside);
        seed(y * w + w - 1, &mut stack, &mut outside);
    }
    while let Some(p) = stack.pop() {
        let (x, y) = (p % w, p / w);
        if x > 0 {
            seed(p - 1, &mut stack, &mut outside);
        }
        if x + 1 < w {
            seed(p + 1, &mut stack, &mut outside);
        }
        if y > 0 {
            seed(p - w, &mut stack, &mut outside);
        }
        if y + 1 < h {
            seed(p + w, &mut stack, &mut outside);
        }
    }
    Gray {
        w,
        h,
        data: outside
            .into_iter()
            .map(|o| if o { 0 } else { 255 })
            .collect(),
    }
}

/// Fill a polygon (pixel coordinates, pixel centres at integers) with the even-odd rule,
/// sampling `ss`x`ss` points per pixel and keeping pixels with at least half coverage.
pub fn fill_polygon(w: usize, h: usize, pts: &[(f64, f64)], ss: usize) -> Gray {
    let mut cover = vec![0u16; w * h];
    let n = pts.len();
    let mut xs: Vec<f64> = Vec::with_capacity(n);
    for sy in 0..h * ss {
        let y = (sy as f64 + 0.5) / ss as f64 - 0.5;
        xs.clear();
        for i in 0..n {
            let (x0, y0) = pts[i];
            let (x1, y1) = pts[(i + 1) % n];
            if (y0 <= y && y < y1) || (y1 <= y && y < y0) {
                xs.push(x0 + (y - y0) * (x1 - x0) / (y1 - y0));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let row = sy / ss;
        for pair in xs.chunks_exact(2) {
            // sample x positions: (sx + 0.5)/ss - 0.5 within [pair0, pair1]
            let lo = ((pair[0] + 0.5) * ss as f64 - 0.5).ceil().max(0.0) as isize;
            let hi = ((pair[1] + 0.5) * ss as f64 - 0.5)
                .floor()
                .min((w * ss) as f64 - 1.0) as isize;
            for sx in lo..=hi {
                cover[row * w + sx as usize / ss] += 1;
            }
        }
    }
    let need = (ss * ss).div_ceil(2) as u16;
    Gray {
        w,
        h,
        data: cover
            .into_iter()
            .map(|c| if c >= need { 255 } else { 0 })
            .collect(),
    }
}

/// Area of the convex hull of a set of points (Andrew's monotone chain + shoelace).
pub fn hull_area(mut pts: Vec<(i64, i64)>) -> f64 {
    pts.sort_unstable();
    pts.dedup();
    if pts.len() < 3 {
        return 0.0;
    }
    let cross = |o: (i64, i64), a: (i64, i64), b: (i64, i64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut hull: Vec<(i64, i64)> = Vec::with_capacity(pts.len() * 2);
    for &p in &pts {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0 {
            hull.pop();
        }
        hull.push(p);
    }
    // Upper hull must not pop back into the lower one.
    let lower = hull.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0 {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    let mut a = 0i64;
    for i in 0..hull.len() {
        let (p, q) = (hull[i], hull[(i + 1) % hull.len()]);
        a += p.0 * q.1 - q.0 * p.1;
    }
    a.abs() as f64 / 2.0
}

/// Pixels whose centre is inside the polygon or within `d` of its outline. This matches
/// OpenCV's anti-aliased fill followed by a >127 threshold, where outline pixels closer than
/// half a pixel to the edge come out above mid-grey.
pub fn fill_polygon_dilated(w: usize, h: usize, pts: &[(f64, f64)], d: f64) -> Gray {
    let mut out = fill_polygon(w, h, pts, 1);
    let n = pts.len();
    let d2 = d * d;
    for y in 0..h {
        for x in 0..w {
            if out.data[y * w + x] != 0 {
                continue;
            }
            let (px, py) = (x as f64, y as f64);
            let near = (0..n).any(|i| {
                let (ax, ay) = pts[i];
                let (bx, by) = pts[(i + 1) % n];
                let (vx, vy) = (bx - ax, by - ay);
                let len2 = vx * vx + vy * vy;
                let t = if len2 > 0.0 {
                    (((px - ax) * vx + (py - ay) * vy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (qx, qy) = (ax + t * vx - px, ay + t * vy - py);
                qx * qx + qy * qy <= d2
            });
            if near {
                out.data[y * w + x] = 255;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_resize_averages_blocks() {
        let src = Gray {
            w: 4,
            h: 2,
            data: vec![255, 255, 0, 0, 255, 0, 0, 0],
        };
        let d = resize_area(&src, 2, 1);
        assert_eq!(d.data, vec![191, 0]);
    }

    #[test]
    fn closing_bridges_gap_and_holes_fill() {
        let mut g = Gray::new(9, 9);
        for y in 2..7 {
            for x in 2..7 {
                g.data[y * 9 + x] = 255;
            }
        }
        g.data[4 * 9 + 4] = 0;
        let f = fill_holes(&g);
        assert_eq!(f.count(), 25);
        assert_eq!(close(&g, 1).at(4, 4), 255);
    }

    #[test]
    fn polygon_square_and_hull() {
        let g = fill_polygon(10, 10, &[(2.0, 2.0), (7.0, 2.0), (7.0, 7.0), (2.0, 7.0)], 1);
        assert_eq!(g.count(), 30); // half-open on the bottom edge
        assert_eq!(
            fill_polygon_dilated(
                10,
                10,
                &[(2.0, 2.0), (7.0, 2.0), (7.0, 7.0), (2.0, 7.0)],
                0.5
            )
            .count(),
            36
        );
        assert_eq!(
            hull_area(vec![(0, 0), (4, 0), (4, 4), (0, 4), (2, 2)]),
            16.0
        );
    }
}
