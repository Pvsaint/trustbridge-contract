/// Contract versioning and compatibility tracking.
///
/// This module provides version management utilities for tracking contract
/// upgrades, migrations, and maintaining backward compatibility.

/// Contract version information.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Create a new version.
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Version { major, minor, patch }
    }

    /// Parse a version from a tuple (used for storage).
    pub fn from_tuple(tuple: (u32, u32, u32)) -> Self {
        Version {
            major: tuple.0,
            minor: tuple.1,
            patch: tuple.2,
        }
    }

    /// Convert version to a tuple for storage.
    pub fn to_tuple(&self) -> (u32, u32, u32) {
        (self.major, self.minor, self.patch)
    }

    /// Create version 1.0.0 (initial release).
    pub fn v1_0_0() -> Self {
        Version::new(1, 0, 0)
    }

    /// Check if this version is compatible with a minimum required version.
    pub fn is_compatible_with(&self, minimum: Version) -> bool {
        // Same major version, minor and patch must be >= minimum
        if self.major != minimum.major {
            return self.major > minimum.major;
        }
        if self.minor != minimum.minor {
            return self.minor > minimum.minor;
        }
        self.patch >= minimum.patch
    }

    /// Check if a migration is needed from old version to new version.
    pub fn needs_migration(&self, target: Version) -> bool {
        self != &target && self.major <= target.major
    }

    /// Get the next patch version (for hot fixes).
    pub fn bump_patch(&self) -> Version {
        Version::new(self.major, self.minor, self.patch + 1)
    }

    /// Get the next minor version (for features).
    pub fn bump_minor(&self) -> Version {
        Version::new(self.major, self.minor + 1, 0)
    }

    /// Get the next major version (for breaking changes).
    pub fn bump_major(&self) -> Version {
        Version::new(self.major + 1, 0, 0)
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Contract migration state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationState {
    /// No migration needed
    NotRequired,
    /// Migration is pending
    Pending,
    /// Migration is in progress
    InProgress,
    /// Migration completed successfully
    Completed,
    /// Migration failed
    Failed,
}

/// Compatibility information between versions.
#[derive(Clone, Copy, Debug)]
pub struct CompatibilityInfo {
    pub current_version: Version,
    pub target_version: Version,
    pub migration_required: bool,
    pub breaking_changes: bool,
    pub data_migration_required: bool,
}

impl CompatibilityInfo {
    /// Create compatibility info for an upgrade.
    pub fn for_upgrade(current: Version, target: Version) -> Self {
        let breaking_changes = current.major != target.major;
        let data_migration_required = breaking_changes || current.minor != target.minor;

        CompatibilityInfo {
            current_version: current,
            target_version: target,
            migration_required: target.needs_migration(current),
            breaking_changes,
            data_migration_required,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        let v1_0_0 = Version::v1_0_0();
        let v1_0_1 = Version::new(1, 0, 1);
        let v1_1_0 = Version::new(1, 1, 0);
        let v2_0_0 = Version::new(2, 0, 0);

        assert!(v1_0_1 > v1_0_0);
        assert!(v1_1_0 > v1_0_1);
        assert!(v2_0_0 > v1_1_0);
    }

    #[test]
    fn test_version_compatibility() {
        let v1_2_0 = Version::new(1, 2, 0);
        let v1_1_5 = Version::new(1, 1, 5);

        assert!(v1_2_0.is_compatible_with(v1_1_5));
        assert!(!v1_1_5.is_compatible_with(v1_2_0));
    }

    #[test]
    fn test_version_bumps() {
        let v1_2_3 = Version::new(1, 2, 3);
        
        assert_eq!(v1_2_3.bump_patch(), Version::new(1, 2, 4));
        assert_eq!(v1_2_3.bump_minor(), Version::new(1, 3, 0));
        assert_eq!(v1_2_3.bump_major(), Version::new(2, 0, 0));
    }

    #[test]
    fn test_version_display() {
        let v = Version::new(1, 2, 3);
        assert_eq!(format!("{}", v), "1.2.3");
    }

    // ABI breaking-change CI gate (Contract Wave #38).
    //
    // These pin the rules a CI gate relies on to decide whether a proposed
    // version bump is allowed to accompany an ABI-breaking change, and
    // whether a migration must run before the new version is trusted.

    #[test]
    fn gate_major_bump_is_flagged_as_breaking() {
        let current = Version::new(1, 4, 2);
        let target = current.bump_major();

        let info = CompatibilityInfo::for_upgrade(current, target);
        assert!(info.breaking_changes);
        assert!(info.data_migration_required);
    }

    #[test]
    fn gate_minor_and_patch_bumps_are_not_breaking() {
        let current = Version::new(1, 4, 2);

        let minor = CompatibilityInfo::for_upgrade(current, current.bump_minor());
        assert!(!minor.breaking_changes);
        assert!(minor.data_migration_required);

        let patch = CompatibilityInfo::for_upgrade(current, current.bump_patch());
        assert!(!patch.breaking_changes);
        assert!(!patch.data_migration_required);
    }

    #[test]
    fn gate_downgrade_is_not_treated_as_a_forward_migration() {
        // A CI gate must not let a "downgrade" masquerade as a safe
        // no-migration bump: needs_migration() only trips for forward
        // moves within the same major line or ahead.
        let current = Version::new(2, 0, 0);
        let older = Version::new(1, 9, 9);

        assert!(!current.needs_migration(older));
    }

    #[test]
    fn gate_same_version_requires_no_migration_and_is_not_breaking() {
        let v = Version::new(3, 1, 4);
        let info = CompatibilityInfo::for_upgrade(v, v);

        assert!(!info.migration_required);
        assert!(!info.breaking_changes);
        assert!(!info.data_migration_required);
    }

    #[test]
    fn gate_compatibility_check_rejects_lower_major() {
        // is_compatible_with() is the check a caller uses to assert a
        // deployed contract is new enough; a lower major must never read
        // as compatible even if minor/patch look newer.
        let deployed = Version::new(1, 9, 9);
        let minimum_required = Version::new(2, 0, 0);

        assert!(!deployed.is_compatible_with(minimum_required));
    }
}
