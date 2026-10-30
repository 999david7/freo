//! Orbit/pan/zoom camera with Creo-style standard orientations.

use crate::kernel::math::*;
use glam::{DQuat, Mat4, Vec3};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StdView { Standard, Default, Front, Back, Top, Bottom, Left, Right }

impl StdView {
    /// The fixed orientations; the part's own saved views (GOODVIEW, …) are listed with them.
    pub const ALL: [StdView; 8] = [StdView::Standard, StdView::Default, StdView::Back, StdView::Left, StdView::Top, StdView::Right, StdView::Bottom, StdView::Front];
    pub fn name(&self) -> &'static str {
        match self {
            StdView::Standard => "Standardorientierung", StdView::Default => "Vorgabeorientierung",
            StdView::Front => "VORNE - FRONT", StdView::Back => "HINTEN - REAR", StdView::Top => "OBEN - TOP",
            StdView::Bottom => "UNTEN - BOTTOM", StdView::Left => "LINKS - LEFT", StdView::Right => "RECHTS - RIGHT",
        }
    }
    pub fn icon(&self) -> super::icons::Icon {
        use super::icons::Icon;
        match self {
            StdView::Standard | StdView::Default => Icon::ViewStandard,
            StdView::Front => Icon::ViewFront, StdView::Back => Icon::ViewRear, StdView::Top => Icon::ViewTop,
            StdView::Bottom => Icon::ViewBottom, StdView::Left => Icon::ViewLeft, StdView::Right => Icon::ViewRight,
        }
    }
    pub fn rotation(&self) -> DQuat {
        use std::f64::consts::FRAC_PI_2;
        match self {
            StdView::Standard | StdView::Default => DQuat::from_axis_angle(V3::Y, 45f64.to_radians()) * DQuat::from_axis_angle(V3::X, -(35.264f64.to_radians())),
            StdView::Front => DQuat::IDENTITY,
            StdView::Back => DQuat::from_axis_angle(V3::Y, std::f64::consts::PI),
            StdView::Top => DQuat::from_axis_angle(V3::X, -FRAC_PI_2),
            StdView::Bottom => DQuat::from_axis_angle(V3::X, FRAC_PI_2),
            StdView::Left => DQuat::from_axis_angle(V3::Y, -FRAC_PI_2),
            StdView::Right => DQuat::from_axis_angle(V3::Y, FRAC_PI_2),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub target: V3,
    pub rot: DQuat,
    /// half of the visible height in model units
    pub scale: f64,
    pub perspective: bool,
    pub spin_center: V3,
    pub depth: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { target: V3::ZERO, rot: StdView::Standard.rotation(), scale: 120.0, perspective: false, spin_center: V3::ZERO, depth: 2000.0 }
    }
}

impl Camera {
    pub fn right(&self) -> V3 { self.rot * V3::X }
    pub fn up(&self) -> V3 { self.rot * V3::Y }
    pub fn back(&self) -> V3 { self.rot * V3::Z }
    pub fn fov_tan(&self) -> f64 { 15f64.to_radians().tan() }

    pub fn eye(&self) -> V3 {
        let dist = if self.perspective { self.scale / self.fov_tan() } else { self.depth };
        self.target + self.back() * dist
    }

    pub fn view(&self) -> Mat4 {
        let e = self.eye();
        Mat4::look_at_rh(e.as_vec3(), self.target.as_vec3(), self.up().as_vec3())
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        let dist = (self.eye() - self.target).length() as f32;
        let range = (self.depth as f32).max(dist * 2.0);
        if self.perspective {
            Mat4::perspective_rh_gl(30f32.to_radians(), aspect, (dist * 0.01).max(0.01), dist + range)
        } else {
            let h = self.scale as f32;
            Mat4::orthographic_rh_gl(-h * aspect, h * aspect, -h, h, 0.0, dist + range)
        }
    }

    /// Screen pixel -> world ray (origin, direction). `px` relative to viewport top-left.
    pub fn ray(&self, px: (f32, f32), size: (f32, f32)) -> (V3, V3) {
        let aspect = (size.0 / size.1) as f64;
        let nx = (px.0 / size.0) as f64 * 2.0 - 1.0;
        let ny = 1.0 - (px.1 / size.1) as f64 * 2.0;
        if self.perspective {
            let t = self.fov_tan();
            let d = safe_norm(-self.back() + self.right() * (nx * t * aspect) + self.up() * (ny * t), -self.back());
            (self.eye(), d)
        } else {
            let o = self.eye() + self.right() * (nx * self.scale * aspect) + self.up() * (ny * self.scale);
            (o, -self.back())
        }
    }

    /// World -> screen pixel (relative to viewport) and view depth.
    pub fn project(&self, p: V3, size: (f32, f32)) -> Option<(f32, f32, f64)> {
        let m = self.proj(size.0 / size.1) * self.view();
        let c = m * glam::Vec4::new(p.x as f32, p.y as f32, p.z as f32, 1.0);
        if c.w <= 1e-6 { return None; }
        let ndc = Vec3::new(c.x / c.w, c.y / c.w, c.z / c.w);
        let depth = (p - self.eye()).dot(-self.back());
        Some(((ndc.x + 1.0) * 0.5 * size.0, (1.0 - ndc.y) * 0.5 * size.1, depth))
    }

    pub fn pixel_size(&self, h: f32) -> f64 { self.scale * 2.0 / h as f64 }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        let k = 0.008;
        let q = self.rot * DQuat::from_axis_angle(V3::Y, -dx as f64 * k) * DQuat::from_axis_angle(V3::X, -dy as f64 * k);
        let delta = q * self.rot.inverse();
        self.target = self.spin_center + delta * (self.target - self.spin_center);
        self.rot = q.normalize();
    }

    pub fn pan(&mut self, dx: f32, dy: f32, h: f32) {
        let ps = self.pixel_size(h);
        self.target -= self.right() * (dx as f64 * ps) - self.up() * (dy as f64 * ps);
    }

    /// Zoom keeping the point under the cursor fixed.
    pub fn zoom_at(&mut self, factor: f64, px: (f32, f32), size: (f32, f32)) {
        let aspect = (size.0 / size.1) as f64;
        let nx = (px.0 / size.0) as f64 * 2.0 - 1.0;
        let ny = 1.0 - (px.1 / size.1) as f64 * 2.0;
        let before = self.right() * (nx * self.scale * aspect) + self.up() * (ny * self.scale);
        self.scale = (self.scale * factor).clamp(1e-3, 1e6);
        let after = self.right() * (nx * self.scale * aspect) + self.up() * (ny * self.scale);
        self.target += before - after;
    }

    pub fn fit(&mut self, b: &BBox, aspect: f32) {
        let (c, d) = if b.is_empty() { (V3::ZERO, 200.0) } else { (b.center(), b.diagonal().max(1.0)) };
        self.target = c;
        self.spin_center = c;
        self.scale = d * 0.5 * 1.15 / (aspect as f64).min(1.0).max(0.4);
        self.depth = d * 10.0 + 1000.0;
    }
}
