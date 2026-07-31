use std::{fmt, io, mem};

use sysinfo::Pid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PriorityPreset {
    VeryHigh,
    High,
    Normal,
    Low,
    VeryLow,
}

impl PriorityPreset {
    pub const ALL: [Self; 5] = [
        Self::VeryHigh,
        Self::High,
        Self::Normal,
        Self::Low,
        Self::VeryLow,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::VeryHigh => "Very High",
            Self::High => "High",
            Self::Normal => "Normal",
            Self::Low => "Low",
            Self::VeryLow => "Very Low",
        }
    }

    pub const fn nice(self) -> i32 {
        match self {
            Self::VeryHigh => -15,
            Self::High => -5,
            Self::Normal => 0,
            Self::Low => 5,
            Self::VeryLow => 15,
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::VeryHigh => Self::High,
            Self::High => Self::Normal,
            Self::Normal => Self::Low,
            Self::Low => Self::VeryLow,
            Self::VeryLow => Self::VeryHigh,
        }
    }

    pub const fn from_nice(nice: i64) -> Self {
        match nice {
            i64::MIN..=-8 => Self::VeryHigh,
            -7..=-3 => Self::High,
            -2..=2 => Self::Normal,
            3..=6 => Self::Low,
            _ => Self::VeryLow,
        }
    }
}

#[derive(Debug)]
pub struct ControlError {
    operation: &'static str,
    pid: Pid,
    source: io::Error,
}

impl ControlError {
    fn last(operation: &'static str, pid: Pid) -> Self {
        Self {
            operation,
            pid,
            source: io::Error::last_os_error(),
        }
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} failed for PID {}: {}",
            self.operation, self.pid, self.source
        )
    }
}

impl std::error::Error for ControlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub fn set_priority(pid: Pid, preset: PriorityPreset) -> Result<(), ControlError> {
    let result = unsafe {
        libc::setpriority(
            libc::PRIO_PROCESS,
            pid.as_u32() as libc::id_t,
            preset.nice(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(ControlError::last("set priority", pid))
    }
}

pub fn available_cpus() -> Vec<usize> {
    let configured = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(libc::CPU_SETSIZE as usize);
    (0..configured).collect()
}

pub fn affinity(pid: Pid) -> Result<Vec<usize>, ControlError> {
    let mut set = unsafe { mem::zeroed::<libc::cpu_set_t>() };
    unsafe {
        libc::CPU_ZERO(&mut set);
    }
    let result = unsafe {
        libc::sched_getaffinity(
            pid.as_u32() as libc::pid_t,
            mem::size_of::<libc::cpu_set_t>(),
            &mut set,
        )
    };
    if result != 0 {
        return Err(ControlError::last("read CPU affinity", pid));
    }

    Ok((0..libc::CPU_SETSIZE as usize)
        .filter(|cpu| unsafe { libc::CPU_ISSET(*cpu, &set) })
        .collect())
}

pub fn set_affinity(pid: Pid, cpus: &[usize]) -> Result<(), ControlError> {
    if cpus.is_empty() {
        return Err(ControlError {
            operation: "set CPU affinity",
            pid,
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "at least one logical CPU must remain selected",
            ),
        });
    }

    let mut set = unsafe { mem::zeroed::<libc::cpu_set_t>() };
    unsafe {
        libc::CPU_ZERO(&mut set);
    }
    for cpu in cpus.iter().copied() {
        if cpu >= libc::CPU_SETSIZE as usize {
            return Err(ControlError {
                operation: "set CPU affinity",
                pid,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("logical CPU {cpu} exceeds CPU_SETSIZE"),
                ),
            });
        }
        unsafe {
            libc::CPU_SET(cpu, &mut set);
        }
    }

    let result = unsafe {
        libc::sched_setaffinity(
            pid.as_u32() as libc::pid_t,
            mem::size_of::<libc::cpu_set_t>(),
            &set,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(ControlError::last("set CPU affinity", pid))
    }
}

pub fn format_affinity(cpus: &[usize]) -> String {
    if cpus.is_empty() {
        return "None".to_string();
    }

    let mut ranges = Vec::new();
    let mut start = cpus[0];
    let mut previous = cpus[0];
    for cpu in cpus.iter().copied().skip(1) {
        if cpu == previous + 1 {
            previous = cpu;
            continue;
        }
        ranges.push(format_range(start, previous));
        start = cpu;
        previous = cpu;
    }
    ranges.push(format_range(start, previous));
    ranges.join(", ")
}

fn format_range(start: usize, end: usize) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_presets_are_ordered() {
        assert_eq!(PriorityPreset::VeryHigh.next(), PriorityPreset::High);
        assert_eq!(PriorityPreset::VeryLow.next(), PriorityPreset::VeryHigh);
        assert!(PriorityPreset::VeryHigh.nice() < PriorityPreset::Normal.nice());
        assert!(PriorityPreset::Normal.nice() < PriorityPreset::VeryLow.nice());
    }

    #[test]
    fn affinity_ranges_are_compact() {
        assert_eq!(format_affinity(&[0, 1, 2, 4, 6, 7]), "0-2, 4, 6-7");
        assert_eq!(format_affinity(&[3]), "3");
        assert_eq!(format_affinity(&[]), "None");
    }
}
