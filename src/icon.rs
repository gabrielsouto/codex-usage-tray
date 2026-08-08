use crate::config::Config;

pub const SIZE: u32 = 32;

/// Ring color for a given utilization (percent used); gray on error.
fn color(util: Option<f64>, cfg: &Config) -> [u8; 4] {
    match util {
        None => [128, 128, 128, 255],
        Some(u) if u >= cfg.icon_red_at as f64 => [239, 68, 68, 255],
        Some(u) if u >= cfg.icon_yellow_at as f64 => [245, 158, 11, 255],
        Some(_) => [16, 185, 129, 255],
    }
}

/// 32×32 RGBA donut chart, filled clockwise from 12 o'clock by `util`
/// percent. `None` renders a full gray ring (error state).
pub fn rgba(util: Option<f64>, cfg: &Config) -> Vec<u8> {
    let mut buf = vec![0u8; (SIZE * SIZE * 4) as usize];
    let c = (SIZE as f64 - 1.0) / 2.0;
    let (r_out, r_in) = (15.0, 6.5);
    let frac = util.unwrap_or(100.0).clamp(0.0, 100.0) / 100.0;
    let fill = color(util, cfg);
    let track = [128u8, 128, 128, 170];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f64 - c;
            let dy = y as f64 - c;
            let d = (dx * dx + dy * dy).sqrt();
            if d < r_in || d > r_out {
                continue;
            }
            let mut ang = dx.atan2(-dy);
            if ang < 0.0 {
                ang += std::f64::consts::TAU;
            }
            let px = if frac > 0.0 && ang <= frac * std::f64::consts::TAU {
                fill
            } else {
                track
            };
            let i = ((y * SIZE + x) * 4) as usize;
            buf[i..i + 4].copy_from_slice(&px);
        }
    }
    buf
}

#[cfg(windows)]
pub fn make(util: Option<f64>, cfg: &Config) -> tray_icon::Icon {
    tray_icon::Icon::from_rgba(rgba(util, cfg), SIZE, SIZE).expect("failed to build tray icon")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colored_pixels(util: Option<f64>) -> (usize, usize) {
        let cfg = Config::default();
        let buf = rgba(util, &cfg);
        let mut fill = 0;
        let mut track = 0;
        for px in buf.chunks(4) {
            match px[3] {
                255 => fill += 1,
                170 => track += 1,
                _ => {}
            }
        }
        (fill, track)
    }

    #[test]
    fn buffer_has_expected_size() {
        let cfg = Config::default();
        assert_eq!(rgba(Some(50.0), &cfg).len(), (SIZE * SIZE * 4) as usize);
    }

    #[test]
    fn fill_grows_with_utilization() {
        let (f0, t0) = colored_pixels(Some(0.0));
        let (f50, _) = colored_pixels(Some(50.0));
        let (f100, t100) = colored_pixels(Some(100.0));
        assert_eq!(f0, 0);
        assert!(t0 > 0);
        assert!(f50 > 0 && f50 < f100);
        assert_eq!(t100, 0);
        let ratio = f50 as f64 / f100 as f64;
        assert!((0.4..=0.6).contains(&ratio), "ratio was {ratio}");
    }

    #[test]
    fn error_state_is_full_gray_ring() {
        let cfg = Config::default();
        let buf = rgba(None, &cfg);
        assert!(buf.chunks(4).any(|p| p == [128, 128, 128, 255]));
    }
}
