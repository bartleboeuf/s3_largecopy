use crate::auto::{AutoProfile, build_auto_plan, clamp_part_size_for_limit, is_instant_copy};


/// Cost estimation module for S3 copy operations.
///
/// Pricing data is based on publicly available AWS S3 pricing as of 2026-02.
/// See: https://aws.amazon.com/s3/pricing/
///
/// Key pricing facts used:
/// - PUT/COPY/POST/LIST requests (Class A): varies by region & storage class
/// - GET/SELECT requests (Class B): varies by region & storage class
/// - Data Transfer: Free within same region, $0.02/GB cross-region (most regions)
/// - Storage: varies by region & storage class
/// - DELETE and CANCEL requests are free.
/// - UploadPartCopy is billed as a PUT request on the destination bucket.

/// Regional pricing data for S3 Standard storage class.
/// Covers the most common AWS regions.
/// Source: https://aws.amazon.com/s3/pricing/
#[derive(Debug, Clone)]
pub struct RegionPricing {
    /// Region identifier (e.g. "us-east-1")
    pub region: &'static str,
    /// Human-friendly name
    pub name: &'static str,
    /// PUT/COPY/POST/LIST per 1,000 requests (Class A) - S3 Standard
    pub put_per_1k: f64,
    /// GET/SELECT per 1,000 requests (Class B) - S3 Standard
    pub get_per_1k: f64,
    /// Storage per GB/month - S3 Standard (first 50 TB tier)
    pub storage_per_gb: f64,
    /// Cross-region data transfer OUT per GB
    pub transfer_out_per_gb: f64,
}

/// S3 pricing table for common regions.
/// Prices from https://aws.amazon.com/s3/pricing/ (as of 2026-02)
const REGION_PRICING: &[RegionPricing] = &[
    // US Regions
    RegionPricing {
        region: "us-east-1",
        name: "US East (N. Virginia)",
        put_per_1k: 0.005,
        get_per_1k: 0.0004,
        storage_per_gb: 0.023,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "us-east-2",
        name: "US East (Ohio)",
        put_per_1k: 0.005,
        get_per_1k: 0.0004,
        storage_per_gb: 0.023,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "us-west-1",
        name: "US West (N. California)",
        put_per_1k: 0.0055,
        get_per_1k: 0.00044,
        storage_per_gb: 0.026,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "us-west-2",
        name: "US West (Oregon)",
        put_per_1k: 0.005,
        get_per_1k: 0.0004,
        storage_per_gb: 0.023,
        transfer_out_per_gb: 0.02,
    },
    // Europe
    RegionPricing {
        region: "eu-west-1",
        name: "EU (Ireland)",
        put_per_1k: 0.005,
        get_per_1k: 0.0004,
        storage_per_gb: 0.023,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "eu-west-2",
        name: "EU (London)",
        put_per_1k: 0.0053,
        get_per_1k: 0.00042,
        storage_per_gb: 0.024,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "eu-west-3",
        name: "EU (Paris)",
        put_per_1k: 0.0053,
        get_per_1k: 0.00042,
        storage_per_gb: 0.024,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "eu-central-1",
        name: "EU (Frankfurt)",
        put_per_1k: 0.0054,
        get_per_1k: 0.00043,
        storage_per_gb: 0.0245,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "eu-north-1",
        name: "EU (Stockholm)",
        put_per_1k: 0.005,
        get_per_1k: 0.0004,
        storage_per_gb: 0.023,
        transfer_out_per_gb: 0.02,
    },
    // Asia Pacific
    RegionPricing {
        region: "ap-southeast-1",
        name: "Asia Pacific (Singapore)",
        put_per_1k: 0.005,
        get_per_1k: 0.0004,
        storage_per_gb: 0.025,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "ap-southeast-2",
        name: "Asia Pacific (Sydney)",
        put_per_1k: 0.0055,
        get_per_1k: 0.00044,
        storage_per_gb: 0.025,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "ap-northeast-1",
        name: "Asia Pacific (Tokyo)",
        put_per_1k: 0.0047,
        get_per_1k: 0.00037,
        storage_per_gb: 0.025,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "ap-northeast-2",
        name: "Asia Pacific (Seoul)",
        put_per_1k: 0.005,
        get_per_1k: 0.0004,
        storage_per_gb: 0.025,
        transfer_out_per_gb: 0.02,
    },
    RegionPricing {
        region: "ap-south-1",
        name: "Asia Pacific (Mumbai)",
        put_per_1k: 0.005,
        get_per_1k: 0.0004,
        storage_per_gb: 0.025,
        transfer_out_per_gb: 0.02,
    },
    // South America
    RegionPricing {
        region: "sa-east-1",
        name: "South America (São Paulo)",
        put_per_1k: 0.007,
        get_per_1k: 0.00056,
        storage_per_gb: 0.0405,
        transfer_out_per_gb: 0.02,
    },
    // Canada
    RegionPricing {
        region: "ca-central-1",
        name: "Canada (Central)",
        put_per_1k: 0.005,
        get_per_1k: 0.0004,
        storage_per_gb: 0.023,
        transfer_out_per_gb: 0.02,
    },
    // Middle East
    RegionPricing {
        region: "me-south-1",
        name: "Middle East (Bahrain)",
        put_per_1k: 0.006,
        get_per_1k: 0.00048,
        storage_per_gb: 0.025,
        transfer_out_per_gb: 0.02,
    },
    // Africa
    RegionPricing {
        region: "af-south-1",
        name: "Africa (Cape Town)",
        put_per_1k: 0.0065,
        get_per_1k: 0.00052,
        storage_per_gb: 0.0274,
        transfer_out_per_gb: 0.02,
    },
];

/// Storage class pricing multipliers relative to S3 Standard.
/// These are approximate and region-independent for simplicity.
fn storage_class_multiplier(storage_class: &str) -> f64 {
    match storage_class {
        "STANDARD" => 1.0,
        "INTELLIGENT_TIERING" | "STANDARD_IA" => 0.54, // ~$0.0125/$0.023
        "ONEZONE_IA" => 0.43,                          // ~$0.01/$0.023
        "GLACIER_IR" | "GLACIER_INSTANT_RETRIEVAL" => 0.17, // ~$0.004/$0.023
        "GLACIER" | "GLACIER_FLEXIBLE_RETRIEVAL" => 0.15, // ~$0.0036/$0.023
        "DEEP_ARCHIVE" => 0.04,                        // ~$0.00099/$0.023
        _ => 1.0,
    }
}

/// Get pricing for a region, falling back to us-east-1 defaults.
pub fn get_region_pricing(region: &str) -> &'static RegionPricing {
    REGION_PRICING
        .iter()
        .find(|r| r.region == region)
        .unwrap_or(&REGION_PRICING[0]) // Fallback to us-east-1
}

/// Result of a cost estimation
#[derive(Debug)]
pub struct CostEstimate {
    /// Source region
    pub source_region: String,
    /// Destination region
    pub dest_region: String,
    /// File size in bytes
    pub file_size_bytes: i64,
    /// Part size in bytes
    pub part_size_bytes: i64,
    /// Number of parts
    pub num_parts: i64,
    /// Storage class
    pub storage_class: String,
    /// Whether same-region copy
    pub same_region: bool,
    /// Individual cost items
    pub api_request_cost: f64,
    pub data_transfer_cost: f64,
    pub monthly_storage_cost: f64,
    /// Total one-time cost (API + transfer)
    pub total_one_time_cost: f64,
    /// Detailed breakdown lines
    pub breakdown: Vec<String>,
}

/// Orchestrate and run a cost estimate.
pub async fn run_estimate(
    args: &crate::args::Args,
    source_region: &str,
    dest_region: &str,
    part_size_mb: i64,
    concurrency: usize,
    auto_profile: crate::auto::AutoProfile,
    verify_integrity: crate::auto::VerifyIntegrity,
) -> anyhow::Result<()> {
    // We still need the app to get the source object size
    let app = crate::app::S3CopyApp::new(
        args.source_bucket.clone().unwrap(),
        args.source_key.clone().unwrap(),
        args.dest_bucket.clone().unwrap(),
        args.dest_key.clone().unwrap(),
        args.dest_region.clone().or(args.region.clone()).or_else(|| Some(dest_region.to_string())),
        Some(source_region.to_string()),
        args.profile.clone(),
        part_size_mb * 1024 * 1024,
        concurrency,
        args.storage_class.clone(),
        args.full_control,
        args.auto,
        auto_profile,
        args.no_metadata,
        args.no_tags,
        args.no_storage_class,
        args.no_acl,
        true, // quiet = true, we only want the estimate output
        true, // dry_run = true, don't modify anything
        args.force_copy,
        verify_integrity,
        args.checksum_algorithm.clone(),
        args.sse.clone(),
        args.sse_kms_key_id.clone(),
    )
    .await?;

    // Get the source object size
    let file_size = app.get_source_size().await?;

    // Attempt to load pricing client for accurate estimates, but fallback to static if it fails
    let pricing = crate::pricing::S3PricingClient::new(args.profile.as_deref()).await.ok();

    let est = estimate_cost(
        file_size,
        part_size_mb * 1024 * 1024,
        args.auto,
        auto_profile,
        source_region,
        Some(dest_region),
        args.storage_class.as_deref(),
        args.no_tags,
        pricing.as_ref(),
    ).await;

    println!("{}", format_estimate(&est));
    Ok(())
}

/// Estimate the cost of a copy operation.
///
/// # Arguments
/// * `file_size_bytes` - Size of the file in bytes
/// * `part_size_bytes` - Part size in bytes
/// * `auto` - Whether auto-tuning is enabled
/// * `source_region` - Source bucket region
/// * `dest_region` - Destination bucket region (if different)
/// * `storage_class` - Target storage class (defaults to STANDARD)
pub async fn estimate_cost(
    file_size_bytes: i64,
    part_size_bytes: i64,
    auto: bool,
    auto_profile: AutoProfile,
    source_region: &str,
    dest_region: Option<&str>,
    storage_class: Option<&str>,
    no_tags: bool,
    pricing_client: Option<&crate::pricing::S3PricingClient>,
) -> CostEstimate {
    let dest_region = dest_region.unwrap_or(source_region);
    let storage_class_str = storage_class.unwrap_or("STANDARD");
    let same_region = source_region == dest_region;

    let is_instant_copy = is_instant_copy(auto, file_size_bytes);
    let effective_part_size = if is_instant_copy {
        0
    } else if auto {
        let auto_plan = build_auto_plan(auto_profile, file_size_bytes, same_region, 64);
        clamp_part_size_for_limit(file_size_bytes, auto_plan.initial_part_size, 10000)
    } else {
        clamp_part_size_for_limit(file_size_bytes, part_size_bytes, 10000)
    };

    // Calculate number of parts
    let num_parts = if is_instant_copy || effective_part_size == 0 {
        0
    } else {
        (file_size_bytes + effective_part_size - 1) / effective_part_size
    };

    // Get falling back destination region pricing (costs are billed to the destination)
    let fallback_pricing = get_region_pricing(dest_region);

    let mut put_per_1k = fallback_pricing.put_per_1k;
    let mut get_per_1k = fallback_pricing.get_per_1k;
    let mut storage_per_gb = fallback_pricing.storage_per_gb * storage_class_multiplier(storage_class_str);
    let mut transfer_out_per_gb = fallback_pricing.transfer_out_per_gb;

    if let Some(client) = pricing_client {
        if let Ok(p) = client.get_class_a_request_price(dest_region, storage_class_str).await {
            put_per_1k = p * 1000.0;
        }
        if let Ok(p) = client.get_class_b_request_price(dest_region, storage_class_str).await {
            get_per_1k = p * 1000.0;
        }
        if let Ok(p) = client.get_storage_price(dest_region, storage_class_str).await {
            storage_per_gb = p;
        }
        if same_region {
            transfer_out_per_gb = 0.0;
        } else {
            if let Ok(p) = client.get_cross_region_transfer_price(source_region, dest_region).await {
                transfer_out_per_gb = p;
            } else if let Ok(p) = client.get_data_transfer_price(source_region).await {
                transfer_out_per_gb = p;
            }
        }
    }

    let mut breakdown = Vec::new();
    let mut api_request_cost = 0.0;

    // --- API Request Costs ---
    // HeadObject on source and destination: 2x GET-class requests
    let head_requests = 2;
    let head_cost = (head_requests as f64) / 1000.0 * get_per_1k;
    api_request_cost += head_cost;
    breakdown.push(format!(
        "  HeadObject              {:>6} req × ${:.4}/1k = ${:.6}",
        head_requests, get_per_1k, head_cost
    ));

    if is_instant_copy {
        if !no_tags {
            let tag_requests = 1;
            let tag_cost = (tag_requests as f64) / 1000.0 * get_per_1k;
            api_request_cost += tag_cost;
            breakdown.push(format!(
                "  GetObjectTagging        {:>6} req × ${:.4}/1k = ${:.6}",
                tag_requests, get_per_1k, tag_cost
            ));
        }

        // Single CopyObject (PUT-class)
        let copy_cost = 1.0 / 1000.0 * put_per_1k;
        api_request_cost += copy_cost;
        breakdown.push(format!(
            "  CopyObject (Instant)    {:>6} req × ${:.4}/1k = ${:.6}",
            1, put_per_1k, copy_cost
        ));
    } else {
        if !no_tags {
            // GetObjectTagging on source: GET-class
            let tag_requests = 1;
            let tag_cost = (tag_requests as f64) / 1000.0 * get_per_1k;
            api_request_cost += tag_cost;
            breakdown.push(format!(
                "  GetObjectTagging        {:>6} req × ${:.4}/1k = ${:.6}",
                tag_requests, get_per_1k, tag_cost
            ));
        }

        // CreateMultipartUpload: 1x PUT-class
        let create_cost = 1.0 / 1000.0 * put_per_1k;
        api_request_cost += create_cost;
        breakdown.push(format!(
            "  CreateMultipartUpload   {:>6} req × ${:.4}/1k = ${:.6}",
            1, put_per_1k, create_cost
        ));

        // UploadPartCopy: num_parts × PUT-class
        let parts_cost = (num_parts as f64) / 1000.0 * put_per_1k;
        api_request_cost += parts_cost;
        breakdown.push(format!(
            "  UploadPartCopy          {:>6} req × ${:.4}/1k = ${:.6}",
            num_parts, put_per_1k, parts_cost
        ));

        // CompleteMultipartUpload: 1x PUT-class
        let complete_cost = 1.0 / 1000.0 * put_per_1k;
        api_request_cost += complete_cost;
        breakdown.push(format!(
            "  CompleteMultipartUpload {:>6} req × ${:.4}/1k = ${:.6}",
            1, put_per_1k, complete_cost
        ));

        // HeadObject verification: 1x GET-class
        let verify_cost = 1.0 / 1000.0 * get_per_1k;
        api_request_cost += verify_cost;
        breakdown.push(format!(
            "  HeadObject (verify)     {:>6} req × ${:.4}/1k = ${:.6}",
            1, get_per_1k, verify_cost
        ));
    }

    // --- Data Transfer Costs ---
    // S3-to-S3 within same region = FREE
    // S3 cross-region via UploadPartCopy = billed as inter-region data transfer
    let file_size_gb = file_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let data_transfer_cost = if same_region {
        0.0
    } else {
        file_size_gb * transfer_out_per_gb
    };

    // --- Storage Costs ---
    let monthly_storage_cost = file_size_gb * storage_per_gb;

    let total_one_time_cost = api_request_cost + data_transfer_cost;

    CostEstimate {
        source_region: source_region.to_string(),
        dest_region: dest_region.to_string(),
        file_size_bytes,
        part_size_bytes: effective_part_size,
        num_parts,
        storage_class: storage_class_str.to_string(),
        same_region,
        api_request_cost,
        data_transfer_cost,
        monthly_storage_cost,
        total_one_time_cost,
        breakdown,
    }
}

/// Format the cost estimate as a pretty-printed report.
pub fn format_estimate(est: &CostEstimate) -> String {
    let file_size_gb = est.file_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let file_size_tb = file_size_gb / 1024.0;

    let size_display = if file_size_tb >= 1.0 {
        format!("{:.2} TB", file_size_tb)
    } else if file_size_gb >= 1.0 {
        format!("{:.2} GB", file_size_gb)
    } else {
        format!("{:.2} MB", est.file_size_bytes as f64 / (1024.0 * 1024.0))
    };

    let strategy = if est.num_parts == 0 {
        "Instant Copy (CopyObject)".to_string()
    } else {
        format!(
            "Multipart Upload ({} parts × {} MB)",
            est.num_parts,
            est.part_size_bytes / 1024 / 1024
        )
    };

    let transfer_note = if est.same_region {
        "Same-region (FREE)".to_string()
    } else {
        format!("Cross-region ({} → {})", est.source_region, est.dest_region)
    };

    let dest_pricing = get_region_pricing(&est.dest_region);

    let mut output = String::new();

    output.push_str("\n╔══════════════════════════════════════════════════════════════╗\n");
    output.push_str("║              💰 S3 COPY COST ESTIMATE                       ║\n");
    output.push_str("╚══════════════════════════════════════════════════════════════╝\n\n");

    output.push_str(&format!("  File size:       {}\n", size_display));
    output.push_str(&format!("  Strategy:        {}\n", strategy));
    output.push_str(&format!("  Data transfer:   {}\n", transfer_note));
    output.push_str(&format!("  Storage class:   {}\n", est.storage_class));
    output.push_str(&format!(
        "  Dest region:     {} ({})\n\n",
        est.dest_region, dest_pricing.name
    ));

    output.push_str("┌──────────────────────────────────────────────────────────────┐\n");
    output.push_str("│ 1. API Request Charges                                      │\n");
    output.push_str("├──────────────────────────────────────────────────────────────┤\n");
    for line in &est.breakdown {
        output.push_str(&format!("│ {}│\n", format!("{:<60}", line)));
    }
    output.push_str("├──────────────────────────────────────────────────────────────┤\n");
    output.push_str(&format!(
        "│ {:>60} │\n",
        format!("Subtotal: ${:.6}", est.api_request_cost)
    ));
    output.push_str("└──────────────────────────────────────────────────────────────┘\n\n");

    output.push_str("┌──────────────────────────────────────────────────────────────┐\n");
    output.push_str("│ 2. Data Transfer Charges                                    │\n");
    output.push_str("├──────────────────────────────────────────────────────────────┤\n");
    if est.same_region {
        output.push_str("│   S3-to-S3 within the same region is FREE.                  │\n");
        output.push_str(&format!("│ {:>60} │\n", "Subtotal: $0.000000"));
    } else {
        let line = format!(
            "  {:.2} GB × ${:.4}/GB = ${:.4}",
            est.file_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            dest_pricing.transfer_out_per_gb,
            est.data_transfer_cost
        );
        output.push_str(&format!("│ {:<60}│\n", line));
        output.push_str("│   (UploadPartCopy = inter-region data transfer)             │\n");
        output.push_str(&format!(
            "│ {:>60} │\n",
            format!("Subtotal: ${:.4}", est.data_transfer_cost)
        ));
    }
    output.push_str("└──────────────────────────────────────────────────────────────┘\n\n");

    output.push_str("┌──────────────────────────────────────────────────────────────┐\n");
    output.push_str("│ 3. Monthly Storage Cost (at destination)                     │\n");
    output.push_str("├──────────────────────────────────────────────────────────────┤\n");
    let storage_line = format!(
        "  {:.2} GB × ${:.4}/GB ({}) = ${:.4}/mo",
        est.file_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        dest_pricing.storage_per_gb * storage_class_multiplier(&est.storage_class),
        est.storage_class,
        est.monthly_storage_cost
    );
    output.push_str(&format!("│ {:<60}│\n", storage_line));
    output.push_str(&format!(
        "│ {:>60} │\n",
        format!("Monthly: ${:.4}", est.monthly_storage_cost)
    ));
    output.push_str("└──────────────────────────────────────────────────────────────┘\n\n");

    output.push_str("══════════════════════════════════════════════════════════════\n");
    output.push_str(&format!(
        "  ONE-TIME COST (API + Transfer):   ${:.6}\n",
        est.total_one_time_cost
    ));
    output.push_str(&format!(
        "  MONTHLY STORAGE COST:             ${:.4}/mo\n",
        est.monthly_storage_cost
    ));
    output.push_str("══════════════════════════════════════════════════════════════\n\n");

    output.push_str("  ℹ️  Prices are based on published AWS S3 pricing (2026-02).\n");
    output.push_str("     Actual costs may vary. Use the AWS Pricing Calculator\n");
    output.push_str("     for authoritative estimates: https://calculator.aws/\n");

    // Add cost saving tip
    if !est.same_region && est.data_transfer_cost > est.api_request_cost * 10.0 {
        output.push_str(&format!(
            "\n  💡 Tip: Cross-region transfer is ${:.2}. Consider copying\n     within the same region if possible.\n",
            est.data_transfer_cost
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gib(n: i64) -> i64 {
        n * 1024 * 1024 * 1024
    }

    /// Validates that auto mode uses Instant Copy for objects smaller than 5 GiB.
    #[tokio::test]
    async fn auto_small_file_uses_instant_copy_strategy() {
        let est = estimate_cost(
            gib(1),
            256 * 1024 * 1024,
            true,
            AutoProfile::Balanced,
            "us-east-1",
            Some("us-east-1"),
            Some("STANDARD"),
            false,
            None,
        ).await;

        assert_eq!(est.num_parts, 0);
        assert_eq!(est.part_size_bytes, 0);
        assert!(est
            .breakdown
            .iter()
            .any(|line| line.contains("CopyObject (Instant)")));
    }

    /// Ensures cross-region estimates include non-zero transfer charges.
    #[tokio::test]
    async fn cross_region_copy_has_transfer_cost() {
        let est = estimate_cost(
            gib(10),
            256 * 1024 * 1024,
            false,
            AutoProfile::Balanced,
            "us-east-1",
            Some("eu-west-1"),
            Some("STANDARD"),
            false,
            None,
        ).await;

        assert!(!est.same_region);
        assert!(est.data_transfer_cost > 0.0);
    }

    /// Ensures same-region estimates keep transfer charges at zero.
    #[tokio::test]
    async fn same_region_copy_has_zero_transfer_cost() {
        let est = estimate_cost(
            gib(10),
            256 * 1024 * 1024,
            false,
            AutoProfile::Balanced,
            "us-east-1",
            Some("us-east-1"),
            Some("STANDARD"),
            false,
            None,
        ).await;

        assert!(est.same_region);
        assert_eq!(est.data_transfer_cost, 0.0);
    }

    /// Verifies that disabling tags removes GetObjectTagging request cost from the breakdown.
    #[tokio::test]
    async fn no_tags_removes_get_object_tagging_from_breakdown() {
        let with_tags = estimate_cost(
            gib(10),
            256 * 1024 * 1024,
            false,
            AutoProfile::Balanced,
            "us-east-1",
            Some("us-east-1"),
            Some("STANDARD"),
            false,
            None,
        ).await;
        let without_tags = estimate_cost(
            gib(10),
            256 * 1024 * 1024,
            false,
            AutoProfile::Balanced,
            "us-east-1",
            Some("us-east-1"),
            Some("STANDARD"),
            true,
            None,
        ).await;

        assert!(with_tags
            .breakdown
            .iter()
            .any(|line| line.contains("GetObjectTagging")));
        assert!(!without_tags
            .breakdown
            .iter()
            .any(|line| line.contains("GetObjectTagging")));
        assert!(without_tags.api_request_cost < with_tags.api_request_cost);
    }

    /// Confirms the cost-efficient profile favors larger parts and fewer multipart requests.
    #[tokio::test]
    async fn cost_efficient_profile_yields_fewer_parts_than_balanced() {
        let size = gib(5 * 1024); // 5 TiB

        let balanced = estimate_cost(
            size,
            256 * 1024 * 1024,
            true,
            AutoProfile::Balanced,
            "us-east-1",
            Some("eu-west-1"),
            Some("STANDARD"),
            false,
            None,
        ).await;
        let cost = estimate_cost(
            size,
            256 * 1024 * 1024,
            true,
            AutoProfile::CostEfficient,
            "us-east-1",
            Some("eu-west-1"),
            Some("STANDARD"),
            false,
            None,
        ).await;

        assert!(cost.part_size_bytes >= balanced.part_size_bytes);
        assert!(cost.num_parts <= balanced.num_parts);
    }
}
