use anyhow::Result;
use clap::Parser;

mod app;
mod args;
mod auto;
mod estimate;
mod progress;
mod s3_utils;

use app::S3CopyApp;
use args::{
    Args, DEFAULT_CONCURRENCY, DEFAULT_PART_SIZE_MB, MAX_CONCURRENT_PARTS, MAX_PART_SIZE_MB,
    MIN_PART_SIZE_MB,
};
use auto::{AutoProfile, VerifyIntegrity};
use s3_pricing::s3_pricing_client::S3PricingClient;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    run(args).await
}

async fn run(args: Args) -> Result<()> {
    let part_size_mb = args.part_size.unwrap_or(DEFAULT_PART_SIZE_MB);
    let concurrency = args.concurrency.unwrap_or(DEFAULT_CONCURRENCY);
    let auto_profile = args.auto_profile.unwrap_or(AutoProfile::Balanced);
    let verify_integrity = args.verify_integrity.unwrap_or(VerifyIntegrity::Etag);
    let prefix_mode = args.source_prefix.is_some() || args.dest_prefix.is_some();

    if args.get_price {
        let region = args
            .region
            .clone()
            .unwrap_or_else(|| "us-east-1".to_string());
        let storage_class = args
            .storage_class
            .clone()
            .unwrap_or_else(|| "STANDARD".to_string());
        let pricing = S3PricingClient::new(args.profile.as_deref()).await?;
        return pricing
            .display_pricing(&region, &storage_class, args.dest_region.as_ref())
            .await;
    }

    let source_bucket = args
        .source_bucket
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--source-bucket is required"))?;
    let dest_bucket = args
        .dest_bucket
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--dest-bucket is required"))?;

    if prefix_mode && (args.source_prefix.is_none() || args.dest_prefix.is_none()) {
        anyhow::bail!("Both --source-prefix and --dest-prefix are required for directory mode");
    }

    if !prefix_mode && (args.source_key.is_none() || args.dest_key.is_none()) {
        anyhow::bail!(
            "--source-key and --dest-key are required for single object copy. Use --source-prefix/--dest-prefix for directory mode."
        );
    }

    let source_key = args.source_key.clone().unwrap_or_default();
    let dest_key = args.dest_key.clone().unwrap_or_default();

    if !(MIN_PART_SIZE_MB..=MAX_PART_SIZE_MB).contains(&part_size_mb) {
        anyhow::bail!(
            "Part size must be between {} and {} MB",
            MIN_PART_SIZE_MB,
            MAX_PART_SIZE_MB
        );
    }
    if concurrency == 0 || concurrency > MAX_CONCURRENT_PARTS {
        anyhow::bail!("Concurrency must be between 1 and {}", MAX_CONCURRENT_PARTS);
    }

    let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
    if let Some(p) = &args.profile {
        config_loader = config_loader.profile_name(p);
    }
    let detection_client = aws_sdk_s3::Client::new(&config_loader.load().await);

    let source_region =
        s3_utils::get_bucket_region(&detection_client, &source_bucket, args.region.as_ref())
            .await?;
    let dest_region = s3_utils::get_bucket_region(
        &detection_client,
        &dest_bucket,
        args.dest_region.as_ref().or(args.region.as_ref()),
    )
    .await?;

    if args.estimate {
        return estimate::run_estimate(
            &args,
            &source_region,
            &dest_region,
            part_size_mb,
            concurrency,
            auto_profile,
            verify_integrity,
        )
        .await;
    }

    let app = S3CopyApp::new(
        source_bucket.clone(),
        source_key.clone(),
        dest_bucket.clone(),
        dest_key.clone(),
        args.region.clone().or_else(|| Some(dest_region.clone())),
        Some(source_region),
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
        args.quiet,
        args.dry_run,
        args.force_copy,
        verify_integrity,
        args.checksum_algorithm.clone(),
        args.sse.clone(),
        args.sse_kms_key_id.clone(),
        args.include.clone(),
        args.exclude.clone(),
    )
    .await?;

    // Check if directory mode is enabled (source_prefix provided)
    if let Some(ref source_prefix) = args.source_prefix {
        let dest_prefix = args.dest_prefix.clone().unwrap_or_default();
        println!("\n=== S3 Directory Copy ===");
        let sb = source_bucket.clone();
        let db = dest_bucket.clone();
        println!("Source prefix: s3://{}/{}", sb, source_prefix);
        println!("Destination:   s3://{}/{}", db, dest_prefix);

        app.copy_from_prefix(source_prefix, &dest_prefix)
            .await
            .map_err(|e| {
                eprintln!("\n❌ Error: {}", e);
                e
            })?;
    } else {
        app.copy_file().await.map_err(|e| {
            eprintln!("\n❌ Error: {}", e);
            e
        })?;
    }

    if !app.quiet {
        println!("\n🎉 Copy completed successfully!");
    }
    Ok(())
}
