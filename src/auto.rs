use clap::ValueEnum;

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum AutoProfile {
    Balanced,
    Aggressive,
    Conservative,
    CostEfficient,
}

impl Default for AutoProfile {
    fn default() -> Self {
        Self::Balanced
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum VerifyIntegrity {
    Off,
    Etag,
    Checksum,
}

impl Default for VerifyIntegrity {
    fn default() -> Self {
        Self::Etag
    }
}

#[derive(Copy, Clone, Debug)]
pub struct AutoPlan {
    pub initial_part_size: i64,
    pub initial_concurrency: usize,
    pub max_concurrency: usize,
    pub probe_parts: usize,
}

#[derive(Copy, Clone, Debug)]
pub struct WindowMetrics {
    pub avg_part_seconds: f64,
    pub throughput_mib_s: f64,
    pub had_retryable_pressure: bool,
}

const MIB: i64 = 1024 * 1024;
const GIB: i64 = 1024 * 1024 * 1024;
const S3_MIN_PART_SIZE: i64 = 5 * MIB;
const S3_MAX_PART_SIZE: i64 = 5 * GIB;

pub fn build_auto_plan(
    profile: AutoProfile,
    file_size_bytes: i64,
    same_region: bool,
    concurrency_cap: usize,
) -> AutoPlan {
    let base_part_size = select_initial_part_size(file_size_bytes, profile);
    let initial_part_size =
        optimize_part_size_for_cost(file_size_bytes, base_part_size, profile, same_region);
    let region_start = recommended_initial_concurrency(profile, same_region);
    let region_max = recommended_max_concurrency(profile, same_region);

    let hard_cap = concurrency_cap.max(1);
    let max_concurrency = region_max.min(hard_cap).max(1);
    let initial_concurrency = region_start.min(max_concurrency).max(1);

    AutoPlan {
        initial_part_size,
        initial_concurrency,
        max_concurrency,
        probe_parts: probe_part_count(profile),
    }
}

pub fn select_initial_part_size(file_size_bytes: i64, profile: AutoProfile) -> i64 {
    let hundred_gb: i64 = 100 * 1024 * 1024 * 1024;
    let one_tb: i64 = 1024 * 1024 * 1024 * 1024;
    let ten_tb: i64 = 10 * 1024 * 1024 * 1024 * 1024;

    match profile {
        AutoProfile::Aggressive => {
            if file_size_bytes < hundred_gb {
                64 * 1024 * 1024
            } else if file_size_bytes < one_tb {
                128 * 1024 * 1024
            } else if file_size_bytes < ten_tb {
                256 * 1024 * 1024
            } else {
                512 * 1024 * 1024
            }
        }
        AutoProfile::Balanced => {
            if file_size_bytes < hundred_gb {
                128 * 1024 * 1024
            } else if file_size_bytes < one_tb {
                256 * 1024 * 1024
            } else if file_size_bytes < ten_tb {
                512 * 1024 * 1024
            } else {
                1024 * 1024 * 1024
            }
        }
        AutoProfile::Conservative => {
            if file_size_bytes < hundred_gb {
                256 * 1024 * 1024
            } else if file_size_bytes < one_tb {
                512 * 1024 * 1024
            } else {
                1024 * 1024 * 1024
            }
        }
        AutoProfile::CostEfficient => {
            if file_size_bytes < hundred_gb {
                1024 * 1024 * 1024
            } else if file_size_bytes < one_tb {
                2 * GIB
            } else if file_size_bytes < ten_tb {
                3 * GIB
            } else {
                4 * GIB
            }
        }
    }
}

pub fn clamp_part_size_for_limit(
    file_size_bytes: i64,
    desired_part_size: i64,
    max_parts: i64,
) -> i64 {
    if file_size_bytes <= 0 {
        return desired_part_size;
    }

    let min_size = S3_MIN_PART_SIZE;
    let desired_part_size = desired_part_size.max(min_size);
    let required_size = ((file_size_bytes + max_parts - 1) / max_parts).max(min_size);
    let required_size_mib = ((required_size + MIB - 1) / MIB) * MIB;
    desired_part_size
        .max(required_size_mib)
        .min(S3_MAX_PART_SIZE)
}

pub fn tune_part_size_from_probe(
    profile: AutoProfile,
    remaining_bytes: i64,
    current_part_size: i64,
    measured_mib_s: f64,
) -> i64 {
    if remaining_bytes <= 0 {
        return current_part_size;
    }

    let tuned = if measured_mib_s >= 1200.0 {
        (current_part_size * 2).min(1024 * 1024 * 1024)
    } else if matches!(profile, AutoProfile::CostEfficient) && measured_mib_s <= 120.0 {
        // Keep large parts for cost efficiency unless speed degradation is extreme.
        current_part_size.max(1024 * 1024 * 1024)
    } else if measured_mib_s <= 120.0 {
        (current_part_size / 2).max(64 * 1024 * 1024)
    } else {
        current_part_size
    };

    clamp_part_size_for_limit(remaining_bytes, tuned, 9500)
}

pub fn optimize_part_size_for_cost(
    file_size_bytes: i64,
    candidate_part_size: i64,
    profile: AutoProfile,
    same_region: bool,
) -> i64 {
    if file_size_bytes <= 0 {
        return candidate_part_size;
    }

    // Request-cost heuristic:
    // fewer parts => fewer UploadPartCopy API calls.
    // Cross-region copies and non-aggressive profiles prioritize lower part count.
    let target_max_parts = match (profile, same_region) {
        (AutoProfile::Aggressive, true) => 3500,
        (AutoProfile::Aggressive, false) => 2800,
        (AutoProfile::Balanced, true) => 2200,
        (AutoProfile::Balanced, false) => 1500,
        (AutoProfile::Conservative, true) => 1200,
        (AutoProfile::Conservative, false) => 800,
        (AutoProfile::CostEfficient, true) => 500,
        (AutoProfile::CostEfficient, false) => 350,
    };

    let cost_floor = ((file_size_bytes + target_max_parts - 1) / target_max_parts)
        .max(S3_MIN_PART_SIZE)
        .min(S3_MAX_PART_SIZE);
    let cost_floor_mib = ((cost_floor + MIB - 1) / MIB) * MIB;

    candidate_part_size
        .max(cost_floor_mib)
        .min(S3_MAX_PART_SIZE)
}

pub fn adapt_concurrency(
    profile: AutoProfile,
    current: usize,
    min_concurrency: usize,
    max_concurrency: usize,
    metrics: WindowMetrics,
) -> usize {
    let step = match profile {
        AutoProfile::Aggressive => 8,
        AutoProfile::Balanced => 4,
        AutoProfile::Conservative => 2,
        AutoProfile::CostEfficient => 1,
    };

    if metrics.had_retryable_pressure {
        return current.saturating_sub(step).max(min_concurrency);
    }

    if metrics.avg_part_seconds < 8.0 && metrics.throughput_mib_s > 0.0 {
        return (current + step).min(max_concurrency);
    }

    if metrics.avg_part_seconds > 25.0 {
        return current.saturating_sub(step).max(min_concurrency);
    }

    current
}

pub fn is_instant_copy(auto: bool, file_size_bytes: i64) -> bool {
    auto && file_size_bytes < 5 * 1024 * 1024 * 1024
}

fn recommended_initial_concurrency(profile: AutoProfile, same_region: bool) -> usize {
    match (profile, same_region) {
        (AutoProfile::Aggressive, true) => 48,
        (AutoProfile::Aggressive, false) => 28,
        (AutoProfile::Balanced, true) => 24,
        (AutoProfile::Balanced, false) => 16,
        (AutoProfile::Conservative, true) => 12,
        (AutoProfile::Conservative, false) => 8,
        (AutoProfile::CostEfficient, true) => 8,
        (AutoProfile::CostEfficient, false) => 6,
    }
}

fn recommended_max_concurrency(profile: AutoProfile, same_region: bool) -> usize {
    match (profile, same_region) {
        (AutoProfile::Aggressive, true) => 96,
        (AutoProfile::Aggressive, false) => 64,
        (AutoProfile::Balanced, true) => 64,
        (AutoProfile::Balanced, false) => 40,
        (AutoProfile::Conservative, true) => 32,
        (AutoProfile::Conservative, false) => 20,
        (AutoProfile::CostEfficient, true) => 16,
        (AutoProfile::CostEfficient, false) => 12,
    }
}

fn probe_part_count(profile: AutoProfile) -> usize {
    match profile {
        AutoProfile::Aggressive => 5,
        AutoProfile::Balanced => 4,
        AutoProfile::Conservative => 3,
        AutoProfile::CostEfficient => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures instant-copy mode is only selected when auto mode is enabled and size is below 5 GiB.
    #[test]
    fn instant_copy_threshold() {
        assert!(is_instant_copy(true, 1024));
        assert!(!is_instant_copy(false, 1024));
        assert!(!is_instant_copy(true, 6 * 1024 * 1024 * 1024));
    }

    /// Verifies part-size clamping enforces the S3 10,000-part ceiling.
    #[test]
    fn clamp_part_size_respects_limit() {
        let size = 20_i64 * 1024 * 1024 * 1024 * 1024;
        let part = clamp_part_size_for_limit(size, 64 * 1024 * 1024, 10000);
        let parts = (size + part - 1) / part;
        assert!(parts <= 10000);
    }

    /// Confirms adaptive concurrency can scale up on healthy windows and down on slow windows.
    #[test]
    fn adaptive_concurrency_moves_up_and_down() {
        let up = adapt_concurrency(
            AutoProfile::Balanced,
            20,
            4,
            64,
            WindowMetrics {
                avg_part_seconds: 6.0,
                throughput_mib_s: 400.0,
                had_retryable_pressure: false,
            },
        );
        assert!(up > 20);

        let down = adapt_concurrency(
            AutoProfile::Balanced,
            20,
            4,
            64,
            WindowMetrics {
                avg_part_seconds: 30.0,
                throughput_mib_s: 100.0,
                had_retryable_pressure: false,
            },
        );
        assert!(down < 20);
    }

    /// Validates that cost optimization increases part size for very large cross-region copies.
    #[test]
    fn cost_optimization_raises_part_size_for_large_cross_region_copy() {
        let ten_tb = 10_i64 * 1024 * 1024 * 1024 * 1024;
        let candidate = 128 * 1024 * 1024;
        let optimized =
            optimize_part_size_for_cost(ten_tb, candidate, AutoProfile::Balanced, false);
        assert!(optimized > candidate);
    }

    /// Ensures the cost-efficient profile starts with larger parts than balanced for large objects.
    #[test]
    fn cost_efficient_targets_larger_parts_than_balanced() {
        let one_tb = 1024_i64 * 1024 * 1024 * 1024;
        let balanced = select_initial_part_size(one_tb, AutoProfile::Balanced);
        let cost = select_initial_part_size(one_tb, AutoProfile::CostEfficient);
        assert!(cost > balanced);
    }
}
