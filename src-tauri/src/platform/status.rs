//! System status for `system.status` (spec §9.4): CPU, memory, battery and network.
//! One sample takes about 200 ms (CPU usage needs two readings), so call it from
//! `spawn_blocking`. Nothing here is personal data: no process names, no addresses.

use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemStatus {
    /// Average over all cores, 0 to 100.
    pub cpu_percent: f32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    /// `None` on machines without a battery, or when it cannot be read.
    pub battery: Option<Battery>,
    /// Some interface other than loopback has a routable address. A hint only:
    /// it does not prove the internet is reachable.
    pub network_up: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    /// 0 to 100.
    pub percent: f32,
    pub state: BatteryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryState {
    Charging,
    Discharging,
    Full,
    Unknown,
}

/// Takes one sample. Each part degrades on its own: an unreadable battery is `None`,
/// unreadable CPU or memory are 0, so the tool can still answer.
pub fn sample() -> SystemStatus {
    let (cpu_percent, memory_used_bytes, memory_total_bytes) = cpu_and_memory();
    SystemStatus {
        cpu_percent,
        memory_used_bytes,
        memory_total_bytes,
        battery: battery(),
        network_up: network_up(),
    }
}

fn cpu_and_memory() -> (f32, u64, u64) {
    let Ok(mut s) = sysinfo::System::new() else {
        return (0.0, 0, 0);
    };
    s.refresh_cpu_usage();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    s.refresh_cpu_usage();
    s.refresh_memory();
    let cpu = s.global_cpu_usage();
    let cpu = if cpu.is_finite() {
        cpu.clamp(0.0, 100.0)
    } else {
        0.0
    };
    (cpu, s.used_memory(), s.total_memory())
}

fn battery() -> Option<Battery> {
    use starship_battery::State;
    use starship_battery::units::ratio::percent;
    let manager = starship_battery::Manager::new().ok()?;
    let mut batteries = manager.batteries().ok()?;
    // Laptops have one; with more, the first is the system battery on every OS we target.
    let b = batteries.next()?.ok()?;
    let pct = b.state_of_charge().get::<percent>();
    Some(Battery {
        percent: if pct.is_finite() {
            pct.clamp(0.0, 100.0)
        } else {
            0.0
        },
        state: match b.state() {
            State::Charging => BatteryState::Charging,
            State::Discharging | State::Empty => BatteryState::Discharging,
            State::Full | State::Paused => BatteryState::Full,
            _ => BatteryState::Unknown,
        },
    })
}

fn network_up() -> bool {
    let Ok(nets) = sysinfo::Networks::new_with_refreshed_list() else {
        return false;
    };
    nets.values()
        .flat_map(|n| n.ip_networks())
        .any(|ip| routable(ip.addr))
}

/// Loopback, link-local (`169.254/16`, `fe80::/10`) and unspecified addresses do not
/// count: a machine with only those has no working network.
fn routable(a: IpAddr) -> bool {
    match a {
        IpAddr::V4(v) => !(v.is_loopback() || v.is_link_local() || v.is_unspecified()),
        IpAddr::V6(v) => {
            !(v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

impl SystemStatus {
    /// What the model reads. Plain numbers; no personal data.
    pub fn describe(&self) -> String {
        let gib = |b: u64| b as f64 / (1024.0 * 1024.0 * 1024.0);
        let mut out = format!(
            "CPU: {:.0}% in use\nMemory: {:.1} of {:.1} GB in use",
            self.cpu_percent,
            gib(self.memory_used_bytes),
            gib(self.memory_total_bytes)
        );
        match self.battery {
            Some(b) => out.push_str(&format!(
                "\nBattery: {:.0}%, {}",
                b.percent,
                match b.state {
                    BatteryState::Charging => "charging",
                    BatteryState::Discharging => "on battery",
                    BatteryState::Full => "full",
                    BatteryState::Unknown => "state unknown",
                }
            )),
            None => out.push_str("\nBattery: none found"),
        }
        out.push_str(if self.network_up {
            "\nNetwork: connected"
        } else {
            "\nNetwork: no connection"
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routable_excludes_loopback_link_local_and_unspecified() {
        for no in [
            "127.0.0.1",
            "169.254.3.4",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "::",
        ] {
            assert!(!routable(no.parse().unwrap()), "{no}");
        }
        for yes in ["192.168.1.20", "10.0.0.5", "2001:db8::1", "fd00::1"] {
            assert!(routable(yes.parse().unwrap()), "{yes}");
        }
    }

    #[test]
    fn describe_reads_well_with_and_without_battery() {
        let mut s = SystemStatus {
            cpu_percent: 12.4,
            memory_used_bytes: 6 * 1024 * 1024 * 1024,
            memory_total_bytes: 16 * 1024 * 1024 * 1024,
            battery: Some(Battery {
                percent: 81.6,
                state: BatteryState::Discharging,
            }),
            network_up: true,
        };
        assert_eq!(
            s.describe(),
            "CPU: 12% in use\nMemory: 6.0 of 16.0 GB in use\nBattery: 82%, on battery\nNetwork: connected"
        );
        s.battery = None;
        s.network_up = false;
        assert!(
            s.describe()
                .ends_with("Battery: none found\nNetwork: no connection")
        );
    }

    #[test]
    fn a_real_sample_is_sane() {
        let s = sample();
        assert!((0.0..=100.0).contains(&s.cpu_percent));
        assert!(s.memory_total_bytes > 0, "every CI machine has memory");
        assert!(s.memory_used_bytes <= s.memory_total_bytes);
        if let Some(b) = s.battery {
            assert!((0.0..=100.0).contains(&b.percent));
        }
    }
}
