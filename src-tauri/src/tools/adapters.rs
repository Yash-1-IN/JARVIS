//! Connects the tool traits (`ports.rs`) to the real platform code in `platform/`. No
//! OS-specific code lives here: it only forwards and converts types.

use super::ports::{AppLauncher, Ports, StatusSource, SystemStatus};
use crate::platform::status::{self, BatteryState};
use std::io;
use std::sync::Arc;

pub struct PlatformLauncher;

impl AppLauncher for PlatformLauncher {
    fn launch(&self, app_id: &str) -> io::Result<()> {
        crate::platform::launch::launch(app_id)
    }
}

pub struct PlatformStatus;

impl StatusSource for PlatformStatus {
    fn sample(&self) -> Option<SystemStatus> {
        Some(convert(&status::sample()))
    }
}

/// Bytes become whole megabytes; a battery that cannot report charging leaves it unknown.
pub fn convert(s: &status::SystemStatus) -> SystemStatus {
    const MB: u64 = 1024 * 1024;
    SystemStatus {
        cpu_percent: Some(s.cpu_percent),
        memory_used_mb: Some(s.memory_used_bytes / MB),
        memory_total_mb: Some(s.memory_total_bytes / MB),
        battery_percent: s.battery.map(|b| b.percent.round().clamp(0.0, 100.0) as u8),
        charging: s.battery.and_then(|b| match b.state {
            BatteryState::Charging => Some(true),
            BatteryState::Discharging => Some(false),
            BatteryState::Full | BatteryState::Unknown => None,
        }),
        online: Some(s.network_up),
    }
}

/// The real adapters, with proposals kept in memory.
pub fn platform_ports() -> Ports {
    Ports {
        launcher: Arc::new(PlatformLauncher),
        status: Arc::new(PlatformStatus),
        ..Ports::placeholders()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::status::Battery;

    #[test]
    fn conversion_maps_units_and_battery_states() {
        let s = status::SystemStatus {
            cpu_percent: 12.0,
            memory_used_bytes: 6 * 1024 * 1024 * 1024,
            memory_total_bytes: 16 * 1024 * 1024 * 1024,
            battery: Some(Battery {
                percent: 80.6,
                state: BatteryState::Charging,
            }),
            network_up: true,
        };
        let c = convert(&s);
        assert_eq!(
            (c.memory_used_mb, c.memory_total_mb),
            (Some(6144), Some(16384))
        );
        assert_eq!((c.battery_percent, c.charging), (Some(81), Some(true)));
        assert_eq!(c.online, Some(true));
        let none = convert(&status::SystemStatus { battery: None, ..s });
        assert_eq!((none.battery_percent, none.charging), (None, None));
    }

    #[test]
    fn the_real_launcher_refuses_a_malformed_id_without_starting_anything() {
        let e = PlatformLauncher.launch("; rm -rf ~").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn the_real_status_source_answers() {
        assert!(PlatformStatus.sample().is_some());
    }
}
