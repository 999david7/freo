//! Normal (grid) support: overhangs steeper than the threshold angle are projected down to
//! the bed (or the model), kept `xy` away from the part and `z` below it, with dense
//! interface layers right under the part.

use super::config::Process;
use super::poly::{self, Shapes};

pub struct SupportLayers {
    /// sparse support area per layer (line centres)
    pub base: Vec<Shapes>,
    /// dense interface area per layer
    pub interface: Vec<Shapes>,
}

/// `slices`: model region per layer, `h`: layer heights, `w`: support line width.
pub fn generate(slices: &[Shapes], heights: &[f64], q: &Process, w: f64) -> SupportLayers {
    let n = slices.len();
    let mut base = vec![vec![]; n];
    let mut interface = vec![vec![]; n];
    if !q.support || n < 2 {
        return SupportLayers { base, interface };
    }
    let h = q.layer_height.max(1e-3);
    let gap_top = if q.support_top_z <= 1e-6 { 0 } else { ((q.support_top_z / h).round() as usize).max(1) };
    let gap_bottom = if q.support_bottom_z <= 1e-6 { 0 } else { ((q.support_bottom_z / h).round() as usize).max(1) };
    let tan = q.support_threshold.clamp(1.0, 89.0).to_radians().tan();
    // overhang of layer i: what sticks out further than the threshold angle allows
    let mut overhang: Vec<Shapes> = vec![vec![]; n];
    for i in 1..n {
        let allowed = heights[i] / tan;
        let lower = poly::offset(&slices[i - 1], allowed);
        // thin slivers matter: under a shallow slope every layer adds one and together they
        // cover the whole surface, so only numerical crumbs are dropped here
        overhang[i] = poly::drop_small(poly::diff(&slices[i], &lower), 1e-3);
    }
    // part grown by the xy distance, per layer
    let keep_out: Vec<Shapes> = slices.iter().map(|s| poly::offset_join(s, q.support_xy, true)).collect();
    // bed-only: everything above the part's shadow is removed
    let mut shadow: Vec<Shapes> = vec![vec![]; n];
    if q.support_build_plate_only {
        let mut acc: Shapes = vec![];
        for i in 0..n {
            acc = poly::union(&acc, &keep_out[i]);
            shadow[i] = acc.clone();
        }
    }
    let mut carry: Shapes = vec![];
    for j in (0..n.saturating_sub(1)).rev() {
        // overhangs that start supporting at this layer (gap_top layers below them)
        let src = j + 1 + gap_top;
        if src < n && !overhang[src].is_empty() {
            carry = poly::union(&carry, &overhang[src]);
        }
        if carry.is_empty() {
            continue;
        }
        // not inside the part on this layer or in the top gap above it, not in the bottom
        // gap over the part it stands on, nowhere over the part for bed-only support
        let mut blocked: Vec<&Shapes> = vec![];
        for k in j..=(j + gap_top).min(n - 1) {
            blocked.push(&keep_out[k]);
        }
        for k in 1..=gap_bottom {
            if j >= k {
                blocked.push(&keep_out[j - k]);
            }
        }
        if q.support_build_plate_only && j > 0 {
            blocked.push(&shadow[j - 1]);
        }
        let blocked = poly::union_all(&blocked);
        let s = poly::diff(&carry, &blocked);
        // what hits the part stops there: below, the part is in the way
        carry = poly::diff(&carry, &slices[j]);
        let min_area = if q.support_remove_small { (w * 2.5).powi(2) } else { w * w * 0.5 };
        // closing merges crumbs into printable islands; it must not refill the gaps
        let s = poly::drop_small(poly::diff(&poly::close(&s, w), &blocked), min_area);
        if s.is_empty() {
            continue;
        }
        // interface: right under overhangs (the next `interface_layers` layers above the gap)
        let mut near: Shapes = vec![];
        for k in 0..q.interface_layers as usize {
            let src = j + 1 + gap_top + k;
            if src < n {
                near = poly::union(&near, &overhang[src]);
            }
        }
        let iface = poly::inter(&s, &poly::offset_join(&near, w, true));
        base[j] = poly::diff(&s, &iface);
        interface[j] = iface;
    }
    SupportLayers { base, interface }
}
