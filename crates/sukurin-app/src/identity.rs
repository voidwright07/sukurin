//! The desktop portal knows a host app by the systemd scope its process runs
//! in, `app-<launcher>-<app>-<n>.scope`, and GNOME grants screenshot
//! permission to that app. Started by a launcher or a terminal, Sukurin
//! inherits their scope and GNOME sees Albert or the terminal asking, so at
//! startup Sukurin moves itself into a scope of its own, as GNOME does for the
//! apps it launches.

use anyhow::{Context as _, Result, anyhow};
use ashpd::zbus;
use ashpd::zbus::zvariant::Value;

const APP: &str = "sukurin";

/// Makes this process the app `sukurin` in the eyes of the portal, unless it
/// already is. Returns the scope it now runs in.
pub fn adopt() -> Result<String> {
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").context("reading /proc/self/cgroup")?;
    if let Some(scope) = own_scope(&cgroup) {
        return Ok(scope.to_string());
    }
    let name = format!("app-{APP}-{}.scope", std::process::id());
    smol::block_on(start_scope(&name)).with_context(|| format!("moving into {name}"))?;
    Ok(name)
}

/// The scope of the cgroup listing when it is already Sukurin's own, however
/// the launcher spelled it (`app-sukurin-7.scope`, `app-gnome-sukurin-7.scope`).
fn own_scope(cgroup: &str) -> Option<&str> {
    let unit = cgroup.lines().find_map(|l| l.strip_prefix("0::"))?.trim().rsplit('/').next()?;
    let app = unit.strip_prefix("app-")?.strip_suffix(".scope")?;
    let (stem, _instance) = app.rsplit_once('-')?;
    (stem == APP || stem.ends_with(&format!("-{APP}"))).then_some(unit)
}

async fn start_scope(name: &str) -> Result<()> {
    let conn = zbus::Connection::session().await?;
    let pids = vec![std::process::id()];
    let properties: Vec<(&str, Value)> = vec![("PIDs", Value::from(pids))];
    let aux: Vec<(&str, Vec<(&str, Value)>)> = Vec::new();
    conn.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        "StartTransientUnit",
        &(name, "fail", properties, aux),
    )
    .await
    .map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_its_own_scope_under_any_launcher() {
        let gnome = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-sukurin-4242.scope\n";
        assert_eq!(own_scope(gnome), Some("app-gnome-sukurin-4242.scope"));
        let own = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-sukurin-77.scope\n";
        assert_eq!(own_scope(own), Some("app-sukurin-77.scope"));
    }

    #[test]
    fn another_apps_scope_or_none_needs_adopting() {
        assert_eq!(own_scope("0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-albert-9.scope\n"), None);
        assert_eq!(own_scope("0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.gnome.Terminal-9.scope\n"), None);
        assert_eq!(own_scope("0::/init.scope\n"), None);
        assert_eq!(own_scope("0::/user.slice/user-1000.slice/session-2.scope\n"), None);
    }
}
