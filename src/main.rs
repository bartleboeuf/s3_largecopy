use anyhow::Result;
use clap::Parser;

mod app;
mod args;
mod estimate;
mod progress;

use app::S3CopyApp;
use args::{
    Args, DEFAULT_CONCURRENCY, DEFAULT_PART_SIZE_MB, MAX_CONCURRENT_PARTS, MAX_PART_SIZE_MB,
    MIN_PART_SIZE_MB,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Validate and set defaults
    let part_size_mb = args.part_size.unwrap_or(DEFAULT_PART_SIZE_MB);
    let concurrency = args.concurrency.unwrap_or(DEFAULT_CONCURRENCY);

    // Validate part size
    if part_size_mb < MIN_PART_SIZE_MB {
        return Err(anyhow::anyhow!(
            "Part size must be at least {} MB",
            MIN_PART_SIZE_MB
        ));
    }
    if part_size_mb > MAX_PART_SIZE_MB {
        return Err(anyhow::anyhow!(
            "Part size cannot exceed {} MB (5GB)",
            MAX_PART_SIZE_MB
        ));
    }

    // Validate concurrency
    if concurrency == 0 || concurrency > MAX_CONCURRENT_PARTS {
        return Err(anyhow::anyhow!(
            "Concurrency must be between 1 and {}",
            MAX_CONCURRENT_PARTS
        ));
    }

    // Handle --estimate mode: fetch source metadata, compute costs, print, and exit
    if args.estimate {
        let source_region = args.region.clone().unwrap_or_else(|| "us-east-1".to_string());
        let dest_region_str = args
            .dest_region
            .clone()
            .unwrap_or_else(|| source_region.clone());

        // We still need the client to get the source object size
        let app = S3CopyApp::new(
            args.source_bucket,
            args.source_key,
            args.dest_bucket,
            args.dest_key,
            args.region,
            part_size_mb * 1024 * 1024,
            concurrency,
            args.storage_class.clone(),
            args.full_control,
            args.auto,
            args.no_metadata,
            args.no_tags,
            args.no_storage_class,
            args.no_acl,
            true, // quiet = true, we only want the estimate output
            true, // dry_run = true, don't modify anything
            args.checksum_algorithm,
            args.sse,
            args.sse_kms_key_id,
        )
        .await?;

        // Get the source object size
        let file_size = app.get_source_size().await?;

        let est = estimate::estimate_cost(
            file_size,
            part_size_mb * 1024 * 1024,
            args.auto,
            &source_region,
            Some(dest_region_str.as_str()),
            args.storage_class.as_deref(),
        );

        println!("{}", estimate::format_estimate(&est));
        return Ok(());
    }

    // Create and run the application (normal mode)
    let app = S3CopyApp::new(
        args.source_bucket,
        args.source_key,
        args.dest_bucket,
        args.dest_key,
        args.region,
        part_size_mb * 1024 * 1024, // Convert MB to bytes
        concurrency,
        args.storage_class,
        args.full_control,
        args.auto,
        args.no_metadata,
        args.no_tags,
        args.no_storage_class,
        args.no_acl,
        args.quiet,
        args.dry_run,
        args.checksum_algorithm,
        args.sse,
        args.sse_kms_key_id,
    )
    .await?;

    match app.copy_file().await {
        Ok(_) => {
            if !app.quiet {
                println!("\n🎉 File copy completed successfully!");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("\n❌ Error: {}", e);
            Err(e)
        }
    }
}
