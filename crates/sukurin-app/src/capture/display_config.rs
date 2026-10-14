//! The monitor layout from GNOME's `org.gnome.Mutter.DisplayConfig`, in the
//! pixels the portal screenshot comes in. GNOME renders a whole-desktop
//! screenshot at the largest monitor scale, so a 1920x1080 monitor beside a
//! 2x one occupies 3840x2160 of the image. Xwayland's RandR rounds that scale
//! up to an integer and GPUI's `wl_output` geometry rounds it too, so neither
//! places a 1.5x monitor where the image has it.

use anyhow::{Context as _, Result, anyhow};
use std::collections::HashMap;
use sukurin_core::{Display, RectPx};
use zbus::zvariant::OwnedValue;

type Props = HashMap<String, OwnedValue>;
type MonitorId = (String, String, String, String);
type Mode = (String, i32, i32, f64, f64, Vec<f64>, Props);
type Monitor = (MonitorId, Vec<Mode>, Props);
type Logical = (i32, i32, f64, u32, bool, Vec<MonitorId>, Props);
type State = (u32, Vec<Monitor>, Vec<Logical>, Props);

/// Every logical monitor, in whole-desktop screenshot pixels.
pub async fn displays() -> Result<Vec<Display>> {
    let conn = zbus::Connection::session().await?;
    let reply = conn
        .call_method(
            Some("org.gnome.Mutter.DisplayConfig"),
            "/org/gnome/Mutter/DisplayConfig",
            Some("org.gnome.Mutter.DisplayConfig"),
            "GetCurrentState",
            &(),
        )
        .await
        .context("org.gnome.Mutter.DisplayConfig.GetCurrentState")?;
    let (_, monitors, logical, _): State = reply.body().deserialize().context("DisplayConfig state")?;
    let sizes = monitors
        .into_iter()
        .filter_map(|(id, modes, _)| {
            let current = modes.iter().find(|m| m.6.get("is-current").is_some_and(|v| bool::try_from(v).unwrap_or(false)));
            current.or(modes.first()).map(|m| (id.0.clone(), (m.1 as u32, m.2 as u32)))
        })
        .collect();
    let logical: Vec<(i32, i32, f64, Vec<String>)> =
        logical.into_iter().map(|(x, y, scale, _, _, ids, _)| (x, y, scale, ids.into_iter().map(|id| id.0).collect())).collect();
    layout(&logical, &sizes)
}

/// Logical monitors (position, scale, connectors) and each connector's mode in
/// physical pixels, laid out at the largest scale: the geometry of the portal's
/// screenshot.
fn layout(logical: &[(i32, i32, f64, Vec<String>)], sizes: &HashMap<String, (u32, u32)>) -> Result<Vec<Display>> {
    let stage = logical.iter().map(|l| l.2).fold(1.0f64, f64::max);
    logical
        .iter()
        .map(|(x, y, scale, connectors)| {
            let name = connectors.first().ok_or_else(|| anyhow!("logical monitor without a connector"))?;
            let (w, h) = sizes.get(name).ok_or_else(|| anyhow!("no mode for {name}"))?;
            let to_stage = |v: f64| (v * stage).round() as i32;
            let rect = RectPx::new(
                to_stage(*x as f64),
                to_stage(*y as f64),
                to_stage(*w as f64 / scale) as u32,
                to_stage(*h as f64 / scale) as u32,
            );
            Ok(Display { rect, scale: stage as f32, name: name.clone() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes() -> HashMap<String, (u32, u32)> {
        HashMap::from([("DP-1".to_string(), (2560, 1440)), ("HDMI-1".to_string(), (1920, 1080))])
    }

    #[test]
    fn unscaled_monitors_keep_their_pixels() {
        let d = layout(&[(0, 0, 1.0, vec!["DP-1".into()]), (2560, 0, 1.0, vec!["HDMI-1".into()])], &sizes()).unwrap();
        assert_eq!(d[0].rect, RectPx::new(0, 0, 2560, 1440));
        assert_eq!(d[1].rect, RectPx::new(2560, 0, 1920, 1080));
        assert_eq!((d[0].scale, d[1].scale), (1.0, 1.0));
    }

    #[test]
    fn a_2x_monitor_doubles_its_unscaled_neighbour() {
        let d = layout(&[(0, 0, 2.0, vec!["DP-1".into()]), (1280, 0, 1.0, vec!["HDMI-1".into()])], &sizes()).unwrap();
        assert_eq!(d[0].rect, RectPx::new(0, 0, 2560, 1440));
        assert_eq!(d[1].rect, RectPx::new(2560, 0, 3840, 2160));
        assert_eq!(d[1].scale, 2.0);
    }

    #[test]
    fn a_fractional_monitor_sets_the_stage_scale() {
        let d = layout(&[(0, 0, 1.0, vec!["DP-1".into()]), (2560, 0, 1.5, vec!["HDMI-1".into()])], &sizes()).unwrap();
        assert_eq!(d[0].rect, RectPx::new(0, 0, 3840, 2160));
        assert_eq!(d[1].rect, RectPx::new(3840, 0, 1920, 1080));
        assert_eq!(d[0].scale, 1.5);
    }
}
