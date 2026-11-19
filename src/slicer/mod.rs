//! 3D-Druck: a complete FDM slicer for Freo parts, written in Rust. The part is placed on
//! the printer's bed, cut into layers, filled with walls, shells, infill and support, and
//! written as G-code for Klipper, Marlin, RepRapFirmware or Bambu printers.

pub mod config;
pub mod gcode;
pub mod infill;
pub mod mesh;
pub mod plan;
pub mod poly;
pub mod printers;
pub mod slice;
pub mod support;
pub mod template;

pub use config::Settings;
pub use plan::Progress;

/// A finished slice: the plan, the G-code and what the preview draws.
pub struct Sliced {
    pub plan: plan::Plan,
    pub out: gcode::Output,
    /// the placed part the G-code was made for
    pub mesh: mesh::Mesh,
}

/// Local date and time for the G-code header.
pub fn now_text() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let t = t + local_offset();
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    // civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02} at {:02}:{:02}:{:02}", y, m, d, secs / 3600, (secs / 60) % 60, secs % 60)
}

#[cfg(unix)]
fn local_offset() -> i64 {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            0
        } else {
            tm.tm_gmtoff as i64
        }
    }
}

#[cfg(not(unix))]
fn local_offset() -> i64 {
    0
}

/// Slices a placed mesh (printer coordinates) with the settings.
pub fn slice(mesh: mesh::Mesh, s: &Settings, prog: &Progress) -> Result<Sliced, String> {
    let plan = plan::plan(&mesh, s, prog)?;
    let thumb = (s.printer.thumbnail > 0).then(|| {
        let n = s.printer.thumbnail.min(512) as usize;
        (n, gcode::thumbnail(&mesh, n, s.filament.color))
    });
    prog.done.store(950, std::sync::atomic::Ordering::Relaxed);
    let mut out = gcode::write(&plan, s, thumb, &now_text());
    let mut w = plan.warnings.clone();
    w.append(&mut out.warnings);
    out.warnings = w;
    prog.done.store(1000, std::sync::atomic::Ordering::Relaxed);
    Ok(Sliced { plan, out, mesh })
}
