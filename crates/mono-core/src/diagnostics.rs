#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Ok,
    Warning,
    Error,
    Unknown,
    Skipped,
}
impl Health {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
            Self::Unknown => "UNKNOWN",
            Self::Skipped => "SKIPPED",
        }
    }
    pub fn problem(self) -> bool {
        matches!(self, Self::Warning | Self::Error)
    }
}
#[derive(Debug)]
pub struct Check {
    pub name: &'static str,
    pub health: Health,
    pub details: Vec<String>,
}
impl Check {
    pub fn new(name: &'static str, health: Health, detail: impl Into<String>) -> Self {
        Self {
            name,
            health,
            details: vec![detail.into()],
        }
    }
}
pub const SECTIONS: &[&str] = &[
    "system",
    "cpu",
    "memory",
    "storage",
    "filesystems",
    "temperature",
    "network",
    "services",
    "boot",
    "hardware",
    "power",
    "logs",
];

pub fn percentage_health(percent: f64, warning: f64, error: f64) -> Health {
    if percent >= error {
        Health::Error
    } else if percent >= warning {
        Health::Warning
    } else {
        Health::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thresholds_are_inclusive() {
        assert_eq!(percentage_health(84.9, 85., 95.), Health::Ok);
        assert_eq!(percentage_health(85., 85., 95.), Health::Warning);
        assert_eq!(percentage_health(95., 85., 95.), Health::Error);
    }
    #[test]
    fn unavailable_is_not_a_problem_or_success() {
        assert!(!Health::Unknown.problem());
        assert!(!Health::Skipped.problem());
        assert!(Health::Warning.problem());
        assert!(Health::Error.problem());
    }
}
