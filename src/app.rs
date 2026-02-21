use crate::auto::{
    AutoProfile, VerifyIntegrity, WindowMetrics, adapt_concurrency, build_auto_plan,
    clamp_part_size_for_limit, is_instant_copy, optimize_part_size_for_cost,
    tune_part_size_from_probe,
};
use crate::progress::CopyProgress;
use anyhow::{Context, Result};
use aws_sdk_s3::operation::head_object::HeadObjectOutput;
use aws_sdk_s3::types::{
    ChecksumAlgorithm, CompletedPart, ObjectCannedAcl, ServerSideEncryption, StorageClass, Tag,
    Tagging,
};
use aws_sdk_s3::{Client, config::Region};
use aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder;
use aws_smithy_types::retry::RetryConfig;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::{Arc, atomic::Ordering};
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio::task;

/// Main application structure
#[derive(Clone)]
pub struct S3CopyApp {
    client: Client,
    source_client: Client,
    source_bucket: String,
    source_key: String,
    dest_bucket: String,
    dest_key: String,
    part_size: i64,
    concurrency: usize,
    storage_class: Option<StorageClass>,
    full_control: bool,
    auto: bool,
    auto_profile: AutoProfile,
    no_metadata: bool,
    no_tags: bool,
    no_storage_class: bool,
    no_acl: bool,
    pub quiet: bool,
    pub dry_run: bool,
    force_copy: bool,
    verify_integrity: VerifyIntegrity,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    pub sse: Option<ServerSideEncryption>,
    pub sse_kms_key_id: Option<String>,
}

#[cfg_attr(test, mockall::automock)]
trait ChecksumProvider {
    fn extract_checksum_value(&self, meta: &HeadObjectOutput) -> Option<String>;
}

struct HeadObjectChecksumProvider;

impl ChecksumProvider for HeadObjectChecksumProvider {
    fn extract_checksum_value(&self, meta: &HeadObjectOutput) -> Option<String> {
        S3CopyApp::extract_checksum_value(meta)
    }
}

impl S3CopyApp {
    /// Create a new S3CopyApp instance
    pub async fn new(
        source_bucket: String,
        source_key: String,
        dest_bucket: String,
        dest_key: String,
        region: Option<String>,
        source_region: Option<String>,
        profile: Option<String>,
        part_size: i64,
        concurrency: usize,
        storage_class: Option<String>,
        full_control: bool,
        auto: bool,
        auto_profile: AutoProfile,
        no_metadata: bool,
        no_tags: bool,
        no_storage_class: bool,
        no_acl: bool,
        quiet: bool,
        dry_run: bool,
        force_copy: bool,
        verify_integrity: VerifyIntegrity,
        checksum_algorithm: Option<String>,
        sse: Option<String>,
        sse_kms_key_id: Option<String>,
    ) -> Result<Self> {
        // Convert storage class string to StorageClass enum
        let storage_class = storage_class.map(|s| StorageClass::from(s.as_str()));

        // Convert checksum algorithm string to ChecksumAlgorithm enum
        let checksum_algorithm = checksum_algorithm.map(|s| ChecksumAlgorithm::from(s.as_str()));

        // Convert SSE string to ServerSideEncryption enum
        let sse = sse.map(|s| ServerSideEncryption::from(s.as_str()));

        // Concurrency is a hard cap; auto mode derives dynamic runtime target within this cap.
        let final_concurrency = concurrency.max(1);

        // Configure a custom Hyper client with increased connection pool limits
        let mut hyper_builder = hyper::Client::builder();
        hyper_builder.pool_max_idle_per_host(final_concurrency);
        hyper_builder.retry_canceled_requests(true);
        hyper_builder.http2_only(false); // Allow fallback to HTTP/1.1

        // Match max connections to concurrency to avoid pool bottlenecks
        hyper_builder.pool_idle_timeout(std::time::Duration::from_secs(90));

        let http_client = HyperClientBuilder::new()
            .hyper_builder(hyper_builder)
            .build_https();

        // Tune retries: More aggressive for large transfers
        let max_attempts = if auto {
            match auto_profile {
                AutoProfile::Aggressive => 10,
                AutoProfile::Balanced => 8,
                AutoProfile::Conservative => 6,
                AutoProfile::CostEfficient => 6,
            }
        } else {
            5
        };
        let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .http_client(http_client.clone())
            .retry_config(RetryConfig::standard().with_max_attempts(max_attempts));

        if let Some(r) = region {
            config_loader = config_loader.region(Region::new(r));
        }

        if let Some(p) = profile.clone() {
            config_loader = config_loader.profile_name(p);
        }

        let config = config_loader.load().await;
        let client = Client::new(&config);

        let mut source_config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .http_client(http_client.clone())
            .retry_config(RetryConfig::standard().with_max_attempts(max_attempts));

        if let Some(r) = source_region {
            source_config_loader = source_config_loader.region(Region::new(r));
        }

        if let Some(p) = profile {
            source_config_loader = source_config_loader.profile_name(p);
        }

        let source_config = source_config_loader.load().await;
        let source_client = Client::new(&source_config);

        Ok(Self {
            client,
            source_client,
            source_bucket,
            source_key,
            dest_bucket,
            dest_key,
            part_size,
            concurrency: final_concurrency,
            storage_class,
            full_control,
            auto,
            auto_profile,
            no_metadata,
            no_tags,
            no_storage_class,
            no_acl,
            quiet,
            dry_run,
            force_copy,
            verify_integrity,
            checksum_algorithm,
            sse,
            sse_kms_key_id,
        })
    }

    /// Get the source object's size in bytes.
    /// Used by the cost estimation flow.
    pub async fn get_source_size(&self) -> Result<i64> {
        let metadata = self
            .get_object_metadata(&self.source_bucket, &self.source_key)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Source object not found: s3://{}/{}",
                    self.source_bucket,
                    self.source_key
                )
            })?;
        Ok(metadata.content_length.unwrap_or(0))
    }

    /// Get object metadata
    async fn get_object_metadata(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Option<HeadObjectOutput>> {
        let client_to_use = if bucket == self.source_bucket {
            &self.source_client
        } else {
            &self.client
        };
        match client_to_use
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
        {
            Ok(output) => Ok(Some(output)),
            Err(e) => {
                let service_error = e.into_service_error();
                if service_error.is_not_found() {
                    return Ok(None);
                }
                Err(anyhow::anyhow!(service_error).context(format!(
                    "Failed to get metadata for s3://{}/{}",
                    bucket, key
                )))
            }
        }
    }

    async fn get_bucket_region(&self, bucket: &str) -> Result<String> {
        let out = self
            .client
            .get_bucket_location()
            .bucket(bucket)
            .send()
            .await
            .with_context(|| format!("Failed to get region for bucket {}", bucket))?;

        let region = out
            .location_constraint()
            .map(|v| v.as_str().to_string())
            .unwrap_or_else(|| "us-east-1".to_string());

        if region.is_empty() {
            Ok("us-east-1".to_string())
        } else {
            Ok(region)
        }
    }

    /// Get object tagging
    async fn get_object_tagging(&self, bucket: &str, key: &str) -> Result<Option<Vec<Tag>>> {
        let client_to_use = if bucket == self.source_bucket {
            &self.source_client
        } else {
            &self.client
        };
        match client_to_use
            .get_object_tagging()
            .bucket(bucket)
            .key(key)
            .send()
            .await
        {
            Ok(output) => Ok(Some(output.tag_set)),
            Err(e) => {
                let service_error = e.into_service_error();
                if format!("{:?}", service_error).contains("NoSuchKey") {
                    return Ok(None);
                }
                Err(anyhow::anyhow!(service_error)
                    .context(format!("Failed to get tagging for s3://{}/{}", bucket, key)))
            }
        }
    }

    /// Initiate multipart upload
    async fn initiate_multipart_upload(
        &self,
        source_etag: &str,
        source_metadata: &HeadObjectOutput,
        source_tags: Option<Vec<Tag>>,
    ) -> Result<String> {
        let mut builder = self
            .client
            .create_multipart_upload()
            .bucket(&self.dest_bucket)
            .key(&self.dest_key)
            .metadata("source-etag", source_etag);

        // Copy high-level metadata unless disabled
        if !self.no_metadata {
            if let Some(cc) = source_metadata.cache_control() {
                builder = builder.cache_control(cc);
            }
            if let Some(cd) = source_metadata.content_disposition() {
                builder = builder.content_disposition(cd);
            }
            if let Some(ce) = source_metadata.content_encoding() {
                builder = builder.content_encoding(ce);
            }
            if let Some(cl) = source_metadata.content_language() {
                builder = builder.content_language(cl);
            }
            if let Some(ct) = source_metadata.content_type() {
                builder = builder.content_type(ct);
            }
            if let Some(wr) = source_metadata.website_redirect_location() {
                builder = builder.website_redirect_location(wr);
            }
            if let Some(ex) = source_metadata.expires_string() {
                if let Ok(dt) = aws_smithy_types::date_time::DateTime::from_str(
                    ex,
                    aws_smithy_types::date_time::Format::HttpDate,
                ) {
                    builder = builder.set_expires(Some(dt));
                }
            }

            // Copy custom metadata
            if let Some(metadata) = source_metadata.metadata() {
                for (key, value) in metadata {
                    if key != "source-etag" {
                        builder = builder.metadata(key, value);
                    }
                }
            }
        }

        // Copy tags unless disabled
        if !self.no_tags {
            if let Some(tags) = source_tags {
                if !tags.is_empty() {
                    let tagging = tags
                        .into_iter()
                        .map(|t| format!("{}={}", t.key(), t.value()))
                        .collect::<Vec<_>>()
                        .join("&");
                    builder = builder.tagging(tagging);
                }
            }
        }

        // Set storage class
        if let Some(sc) = &self.storage_class {
            builder = builder.storage_class(sc.clone());
        } else if !self.no_storage_class {
            if let Some(sc) = source_metadata.storage_class() {
                builder = builder.storage_class(sc.clone());
            }
        }

        // Set ACL unless disabled
        if self.full_control && !self.no_acl {
            builder = builder.acl(ObjectCannedAcl::BucketOwnerFullControl);
        }

        if let Some(algo) = &self.checksum_algorithm {
            builder = builder.checksum_algorithm(algo.clone());
        }

        // Set Encryption
        if let Some(sse) = &self.sse {
            builder = builder.server_side_encryption(sse.clone());
        }
        if let Some(key_id) = &self.sse_kms_key_id {
            builder = builder.ssekms_key_id(key_id);
        }

        if self.dry_run {
            if !self.quiet {
                println!(
                    "   [Dry Run] Would initiate multipart upload (dest: s3://{}/{})",
                    self.dest_bucket, self.dest_key
                );
            }
            return Ok("DRY-RUN-UPLOAD-ID".to_string());
        }

        let response = builder.send().await.with_context(|| {
            format!(
                "Failed to initiate multipart upload to s3://{}/{}",
                self.dest_bucket, self.dest_key
            )
        })?;

        Ok(response.upload_id.unwrap_or_default())
    }

    /// Upload a single part using copy
    async fn upload_part_copy(
        &self,
        upload_id: &str,
        part_number: i32,
        source_range: &str,
    ) -> Result<CompletedPart> {
        if self.dry_run {
            // Emulate delay for dry run
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            return Ok(CompletedPart::builder()
                .part_number(part_number)
                .e_tag("dry-run-etag")
                .build());
        }

        let response = self
            .client
            .upload_part_copy()
            .bucket(&self.dest_bucket)
            .key(&self.dest_key)
            .upload_id(upload_id)
            .part_number(part_number)
            .copy_source(format!("{}/{}", self.source_bucket, self.source_key))
            .copy_source_range(source_range.to_string())
            .send()
            .await
            .with_context(|| {
                format!(
                    "Failed to upload part {} (range: {})",
                    part_number, source_range
                )
            })?;

        let etag = response.copy_part_result.unwrap().e_tag.unwrap_or_default();

        Ok(CompletedPart::builder()
            .part_number(part_number)
            .e_tag(etag)
            .build())
    }

    /// Complete the multipart upload
    async fn complete_multipart_upload(
        &self,
        upload_id: &str,
        parts: Vec<CompletedPart>,
    ) -> Result<()> {
        if self.dry_run {
            if !self.quiet {
                println!(
                    "   [Dry Run] Would complete multipart upload (upload_id: {})",
                    upload_id
                );
            }
            return Ok(());
        }

        self.client
            .complete_multipart_upload()
            .bucket(&self.dest_bucket)
            .key(&self.dest_key)
            .upload_id(upload_id)
            .multipart_upload(
                aws_sdk_s3::types::CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .send()
            .await
            .with_context(|| {
                format!(
                    "Failed to complete multipart upload for s3://{}/{}",
                    self.dest_bucket, self.dest_key
                )
            })?;

        Ok(())
    }

    /// Abort multipart upload on failure
    async fn abort_multipart_upload(&self, upload_id: &str) -> Result<()> {
        if self.dry_run {
            if !self.quiet {
                println!(
                    "   [Dry Run] Would abort multipart upload (upload_id: {})",
                    upload_id
                );
            }
            return Ok(());
        }

        self.client
            .abort_multipart_upload()
            .bucket(&self.dest_bucket)
            .key(&self.dest_key)
            .upload_id(upload_id)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Failed to abort multipart upload for s3://{}/{}",
                    self.dest_bucket, self.dest_key
                )
            })?;

        Ok(())
    }

    async fn run_copy_window(
        &self,
        upload_id: &str,
        batch: Vec<(i32, String, u64)>,
        progress: &CopyProgress,
        progress_bar: &ProgressBar,
    ) -> Result<(Vec<CompletedPart>, WindowMetrics)> {
        let started = Instant::now();
        let window_bytes: u64 = batch.iter().map(|(_, _, bytes)| *bytes).sum();
        let semaphore = Arc::new(Semaphore::new(batch.len()));
        let mut handles = Vec::with_capacity(batch.len());
        let mut total_part_seconds = 0.0_f64;

        for (part_number, range, part_size_bytes) in batch {
            let app = self.clone();
            let upload_id = upload_id.to_string();
            let semaphore = semaphore.clone();
            let progress = progress.clone();
            let progress_bar = progress_bar.clone();

            let handle = task::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                let part_started = Instant::now();
                let completed_part = app
                    .upload_part_copy(&upload_id, part_number, &range)
                    .await?;
                let elapsed = part_started.elapsed().as_secs_f64();

                progress.add_completed(part_size_bytes);
                progress_bar.set_position(progress.copied_bytes.load(Ordering::SeqCst));
                let completed = progress.completed_parts.load(Ordering::SeqCst);
                let total = progress.total_parts;
                progress_bar.set_message(format!("{}/{} parts completed", completed, total));

                Ok::<_, anyhow::Error>((completed_part, elapsed))
            });
            handles.push(handle);
        }

        let mut completed_parts = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(Ok((part, elapsed))) => {
                    total_part_seconds += elapsed;
                    completed_parts.push(part);
                }
                Ok(Err(e)) => {
                    return Err(e);
                }
                Err(join_err) => {
                    return Err(anyhow::anyhow!(join_err).context("Part task join error"));
                }
            }
        }

        let elapsed = started.elapsed().as_secs_f64().max(0.001);
        let bytes = window_bytes as f64;
        let throughput_mib_s = (bytes / (1024.0 * 1024.0)) / elapsed;
        let avg_part_seconds = total_part_seconds / completed_parts.len().max(1) as f64;

        Ok((
            completed_parts,
            WindowMetrics {
                avg_part_seconds,
                throughput_mib_s,
                had_retryable_pressure: false,
            },
        ))
    }

    fn extract_checksum_value(meta: &HeadObjectOutput) -> Option<String> {
        if let Some(v) = meta.checksum_sha256() {
            return Some(format!("SHA256:{}", v));
        }
        if let Some(v) = meta.checksum_sha1() {
            return Some(format!("SHA1:{}", v));
        }
        if let Some(v) = meta.checksum_crc32_c() {
            return Some(format!("CRC32C:{}", v));
        }
        if let Some(v) = meta.checksum_crc32() {
            return Some(format!("CRC32:{}", v));
        }
        None
    }

    fn verify_checksum_with_provider<P: ChecksumProvider>(
        provider: &P,
        source_metadata: &HeadObjectOutput,
        dest_metadata: &HeadObjectOutput,
    ) -> Result<()> {
        let src = provider.extract_checksum_value(source_metadata);
        let dst = provider.extract_checksum_value(dest_metadata);
        match (src, dst) {
            (Some(a), Some(b)) if a == b => Ok(()),
            (Some(a), Some(b)) => Err(anyhow::anyhow!(
                "Checksum mismatch: source={} destination={}",
                a,
                b
            )),
            _ => Err(anyhow::anyhow!(
                "Checksum verification requested but checksum headers are not available. Use --checksum-algorithm during copy."
            )),
        }
    }

    /// Copy the file using multipart upload
    pub async fn copy_file(&self) -> Result<()> {
        if !self.quiet {
            println!("\n=== S3 Large File Copy ===");
            println!(
                "Source:      s3://{}/{}",
                self.source_bucket, self.source_key
            );
            println!("Destination: s3://{}/{}", self.dest_bucket, self.dest_key);
            println!("Part size:   {} MB", self.part_size / 1024 / 1024);
            println!("Concurrency: {} parts", self.concurrency);
            println!("=========================\n");
        }

        if self.dry_run && !self.quiet {
            println!("🚨 DRY RUN MODE: No data will be modified.");
        }

        // Get source object metadata
        let metadata = self
            .get_object_metadata(&self.source_bucket, &self.source_key)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Source object not found: s3://{}/{}",
                    self.source_bucket,
                    self.source_key
                )
            })?;
        let content_length = metadata.content_length.unwrap_or(0);

        // Check if destination exists and is identical unless forced.
        if self.force_copy {
            if !self.quiet {
                println!(
                    "⚠️  Force copy enabled: destination pre-check optimizations are disabled. Object will be overwritten."
                );
            }
        } else if let Some(dest_metadata) = self
            .get_object_metadata(&self.dest_bucket, &self.dest_key)
            .await?
        {
            let dest_size = dest_metadata.content_length.unwrap_or(0);
            let dest_etag = dest_metadata.e_tag.as_deref().unwrap_or_default();
            let src_etag = metadata.e_tag.as_deref().unwrap_or_default();

            // Check for persistent source ETag in metadata
            let dest_stored_src_etag = dest_metadata
                .metadata()
                .and_then(|m| m.get("source-etag"))
                .map(|s| format!("\"{}\"", s.trim_matches('"'))) // Standardize quotes
                .unwrap_or_default();

            let standardized_src_etag = format!("\"{}\"", src_etag.trim_matches('"'));

            if dest_size == content_length
                && (dest_etag == src_etag || dest_stored_src_etag == standardized_src_etag)
            {
                // Data matches. Now check if properties need syncing.
                if !self.quiet {
                    println!("✅ Data identity verified (Size & ETag). Checking properties...");
                }

                let source_tags = if self.no_tags {
                    None
                } else {
                    self.get_object_tagging(&self.source_bucket, &self.source_key)
                        .await?
                };
                let dest_tags = if self.no_tags {
                    None
                } else {
                    self.get_object_tagging(&self.dest_bucket, &self.dest_key)
                        .await?
                };

                let tags_match = self.no_tags || source_tags == dest_tags;
                let storage_class_match = self.no_storage_class
                    || (dest_metadata.storage_class() == self.storage_class.as_ref());

                // Compare basic metadata headers if not disabled
                let metadata_match = self.no_metadata
                    || (dest_metadata.cache_control() == metadata.cache_control()
                        && dest_metadata.content_disposition() == metadata.content_disposition()
                        && dest_metadata.content_encoding() == metadata.content_encoding()
                        && dest_metadata.content_language() == metadata.content_language()
                        && dest_metadata.content_type() == metadata.content_type()
                        && dest_metadata.website_redirect_location()
                            == metadata.website_redirect_location()
                        && dest_metadata.expires_string() == metadata.expires_string());

                if tags_match && storage_class_match && metadata_match {
                    if !self.quiet {
                        println!(
                            "⏭️  Skipping copy: Destination s3://{}/{} is already identical in data and properties.",
                            self.dest_bucket, self.dest_key
                        );
                    }
                    return Ok(());
                } else if content_length <= 5 * 1024 * 1024 * 1024 {
                    if !self.quiet {
                        println!(
                            "🔄 Data matches but properties differ. Performing property-only sync via CopyObject..."
                        );
                    }
                    // Property-only sync: Use CopyObject with MetadataDirective=REPLACE
                    let mut builder = self
                        .client
                        .copy_object()
                        .bucket(&self.dest_bucket)
                        .key(&self.dest_key)
                        .copy_source(format!("{}/{}", self.source_bucket, self.source_key))
                        .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace);

                    // Apply ACL unless disabled
                    if self.full_control && !self.no_acl {
                        builder = builder.acl(ObjectCannedAcl::BucketOwnerFullControl);
                    }

                    // Set checksum algorithm if provided
                    if let Some(algo) = &self.checksum_algorithm {
                        builder = builder.checksum_algorithm(algo.clone());
                    }

                    // Set Encryption
                    if let Some(sse) = &self.sse {
                        builder = builder.server_side_encryption(sse.clone());
                    }
                    if let Some(key_id) = &self.sse_kms_key_id {
                        builder = builder.ssekms_key_id(key_id);
                    }

                    // Re-apply metadata unless disabled
                    if !self.no_metadata {
                        if let Some(ct) = metadata.content_type() {
                            builder = builder.content_type(ct);
                        }
                        if let Some(cc) = metadata.cache_control() {
                            builder = builder.cache_control(cc);
                        }
                        if let Some(cd) = metadata.content_disposition() {
                            builder = builder.content_disposition(cd);
                        }
                        if let Some(ce) = metadata.content_encoding() {
                            builder = builder.content_encoding(ce);
                        }
                        if let Some(cl) = metadata.content_language() {
                            builder = builder.content_language(cl);
                        }
                        if let Some(wr) = metadata.website_redirect_location() {
                            builder = builder.website_redirect_location(wr);
                        }
                        if let Some(ex) = metadata.expires_string() {
                            if let Ok(dt) = aws_smithy_types::date_time::DateTime::from_str(
                                ex,
                                aws_smithy_types::date_time::Format::HttpDate,
                            ) {
                                builder = builder.set_expires(Some(dt));
                            }
                        }
                    }

                    // Re-apply custom metadata unless disabled (preserving our source-etag)
                    if !self.no_metadata {
                        if let Some(m) = metadata.metadata() {
                            for (k, v) in m {
                                if k != "source-etag" {
                                    builder = builder.metadata(k, v);
                                }
                            }
                        }
                    }
                    // Always maintain our source-etag tracking metadata
                    builder = builder.metadata("source-etag", src_etag);

                    // Re-apply storage class unless disabled
                    if let Some(sc) = &self.storage_class {
                        builder = builder.storage_class(sc.clone());
                    } else if !self.no_storage_class {
                        if let Some(sc) = metadata.storage_class() {
                            builder = builder.storage_class(sc.clone());
                        }
                    }

                    // Sync tags if needed and not disabled
                    if !self.no_tags && !tags_match {
                        if let Some(tags) = &source_tags {
                            let tagging = tags
                                .iter()
                                .map(|t| format!("{}={}", t.key(), t.value()))
                                .collect::<Vec<_>>()
                                .join("&");
                            builder = builder.tagging(tagging);
                            builder = builder
                                .tagging_directive(aws_sdk_s3::types::TaggingDirective::Replace);
                        }
                    }

                    if self.dry_run {
                        if !self.quiet {
                            println!(
                                "   [Dry Run] Would sync properties via CopyObject (REPLACE directive)"
                            );
                        }
                    } else {
                        builder
                            .send()
                            .await
                            .with_context(|| "Failed to sync properties via CopyObject")?;
                    }

                    if !self.quiet {
                        println!("✨ Property sync completed successfully.");
                    }
                    return Ok(());
                } else if !tags_match && storage_class_match && metadata_match {
                    // Object > 5GB, but only tags changed. We can use PutObjectTagging.
                    if !self.quiet {
                        println!(
                            "🔄 Data matches, object > 5GB, but ONLY tags differ. Syncing tags..."
                        );
                    }
                    if let Some(tags) = source_tags {
                        let tagging = Tagging::builder()
                            .set_tag_set(Some(tags))
                            .build()
                            .context("Failed to build tagging")?;
                        if self.dry_run {
                            if !self.quiet {
                                println!("   [Dry Run] Would update object tags");
                            }
                        } else {
                            self.client
                                .put_object_tagging()
                                .bucket(&self.dest_bucket)
                                .key(&self.dest_key)
                                .tagging(tagging)
                                .send()
                                .await
                                .with_context(|| "Failed to sync tags")?;
                        }
                        if !self.quiet {
                            println!("✨ Tags updated successfully.");
                        }
                        return Ok(());
                    }
                } else {
                    if !self.quiet {
                        println!(
                            "🔄 Data matches, but object > 5GB and metadata/storage-class differ."
                        );
                        println!(
                            "   S3 requires a full copy for metadata updates > 5GB. Proceeding with Multipart Copy..."
                        );
                    }
                    // Fall through to regular multipart copy loop
                }
            }
        }

        if content_length == 0 {
            return Err(anyhow::anyhow!("Source object is empty"));
        }

        // Fetch source tags (needed for both Instant Copy and Multipart Initiate)
        let source_tags = if self.no_tags {
            None
        } else {
            self.get_object_tagging(&self.source_bucket, &self.source_key)
                .await?
        };

        // Instant copy path for small objects when auto mode is enabled.
        if is_instant_copy(self.auto, content_length) {
            if !self.quiet {
                println!(
                    "🤖 Auto Mode: Small file detected ({:.2} MB). Using Instant Copy (CopyObject)...",
                    content_length as f64 / (1024.0 * 1024.0)
                );
            }

            let src_etag = metadata.e_tag.as_deref().unwrap_or_default();
            let mut builder = self
                .client
                .copy_object()
                .bucket(&self.dest_bucket)
                .key(&self.dest_key)
                .copy_source(format!("{}/{}", self.source_bucket, self.source_key))
                .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace);

            // Apply ACL
            if self.full_control && !self.no_acl {
                builder = builder.acl(ObjectCannedAcl::BucketOwnerFullControl);
            }

            // Set checksum algorithm if provided
            if let Some(algo) = &self.checksum_algorithm {
                builder = builder.checksum_algorithm(algo.clone());
            }

            // Set Encryption
            if let Some(sse) = &self.sse {
                builder = builder.server_side_encryption(sse.clone());
            }
            if let Some(key_id) = &self.sse_kms_key_id {
                builder = builder.ssekms_key_id(key_id);
            }

            // Apply metadata
            if !self.no_metadata {
                if let Some(ct) = metadata.content_type() {
                    builder = builder.content_type(ct);
                }
                if let Some(cc) = metadata.cache_control() {
                    builder = builder.cache_control(cc);
                }
                if let Some(cd) = metadata.content_disposition() {
                    builder = builder.content_disposition(cd);
                }
                if let Some(ce) = metadata.content_encoding() {
                    builder = builder.content_encoding(ce);
                }
                if let Some(cl) = metadata.content_language() {
                    builder = builder.content_language(cl);
                }
                if let Some(wr) = metadata.website_redirect_location() {
                    builder = builder.website_redirect_location(wr);
                }
                if let Some(ex) = metadata.expires_string() {
                    if let Ok(dt) = aws_smithy_types::date_time::DateTime::from_str(
                        ex,
                        aws_smithy_types::date_time::Format::HttpDate,
                    ) {
                        builder = builder.set_expires(Some(dt));
                    }
                }

                // Re-apply custom metadata (preserving our source-etag)
                if let Some(m) = metadata.metadata() {
                    for (k, v) in m {
                        if k != "source-etag" {
                            builder = builder.metadata(k, v);
                        }
                    }
                }
            }
            // Always maintain our source-etag tracking metadata
            builder = builder.metadata("source-etag", src_etag);

            // Apply storage class
            if let Some(sc) = &self.storage_class {
                builder = builder.storage_class(sc.clone());
            } else if !self.no_storage_class {
                if let Some(sc) = metadata.storage_class() {
                    builder = builder.storage_class(sc.clone());
                }
            }

            // Apply tags
            if !self.no_tags {
                if let Some(tags) = &source_tags {
                    if !tags.is_empty() {
                        let tagging = tags
                            .iter()
                            .map(|t| format!("{}={}", t.key(), t.value()))
                            .collect::<Vec<_>>()
                            .join("&");
                        builder = builder.tagging(tagging);
                        builder =
                            builder.tagging_directive(aws_sdk_s3::types::TaggingDirective::Replace);
                    }
                }
            }

            if self.dry_run {
                if !self.quiet {
                    println!("   [Dry Run] Would perform Instant Copy (CopyObject)");
                }
            } else {
                builder
                    .send()
                    .await
                    .with_context(|| "Failed to perform Instant Copy")?;
            }

            if !self.quiet {
                println!("✨ Instant Copy completed successfully.");
            }
            return Ok(());
        }

        let mut part_size = self.part_size;
        let mut target_concurrency = self.concurrency.max(1);
        let mut max_auto_concurrency = target_concurrency;
        let mut probe_parts = 0usize;
        let mut same_region_for_auto = false;

        if self.auto {
            let same_region = match (
                self.get_bucket_region(&self.source_bucket).await,
                self.get_bucket_region(&self.dest_bucket).await,
            ) {
                (Ok(src), Ok(dst)) => src == dst,
                _ => {
                    if !self.quiet {
                        println!(
                            "⚠️  Auto Mode: Could not determine both bucket regions. Assuming cross-region defaults."
                        );
                    }
                    false
                }
            };
            same_region_for_auto = same_region;
            let auto_plan = build_auto_plan(
                self.auto_profile,
                content_length,
                same_region,
                self.concurrency,
            );
            part_size = auto_plan.initial_part_size;
            target_concurrency = auto_plan.initial_concurrency;
            max_auto_concurrency = auto_plan.max_concurrency;
            probe_parts = auto_plan.probe_parts;
            if !self.quiet {
                println!(
                    "🤖 Auto Mode: profile={:?}, initial part size={} MB, concurrency start={} (max {})",
                    self.auto_profile,
                    part_size / 1024 / 1024,
                    target_concurrency,
                    max_auto_concurrency
                );
            }
        }

        part_size = clamp_part_size_for_limit(content_length, part_size, 10000);

        // Initiate multipart upload
        if !self.quiet {
            println!("\n📤 Initiating multipart upload...");
        }
        let src_etag = metadata.e_tag.as_deref().unwrap_or_default();
        let upload_id = self
            .initiate_multipart_upload(src_etag, &metadata, source_tags)
            .await?;
        if !self.quiet {
            println!("   Upload ID: {}", upload_id);
        }

        // Wrap the upload logic to ensure cleanup on failure
        let upload_result: Result<()> = async {
            let mut completed_parts: Vec<CompletedPart> = Vec::new();
            let mut next_part_number: i32 = 1;
            let mut next_start_byte: i64 = 0;

            if self.auto && probe_parts > 0 {
                let probe_start = Instant::now();
                let mut probe_measured_mib_s = 0.0_f64;
                let mut probe_done = 0usize;
                let max_probe = std::cmp::min(
                    probe_parts,
                    ((content_length + part_size - 1) / part_size) as usize,
                );

                if !self.quiet {
                    println!("🧪 Auto Mode: running warm-up probe ({} parts)...", max_probe);
                }

                for _ in 0..max_probe {
                    if next_start_byte >= content_length {
                        break;
                    }
                    let end_byte = std::cmp::min(next_start_byte + part_size, content_length) - 1;
                    let range = format!("bytes={}-{}", next_start_byte, end_byte);
                    let part_bytes = (end_byte - next_start_byte + 1) as u64;
                    let started = Instant::now();
                    let part = self
                        .upload_part_copy(&upload_id, next_part_number, &range)
                        .await?;
                    let secs = started.elapsed().as_secs_f64().max(0.001);
                    probe_measured_mib_s += (part_bytes as f64 / (1024.0 * 1024.0)) / secs;
                    completed_parts.push(part);
                    next_part_number += 1;
                    next_start_byte = end_byte + 1;
                    probe_done += 1;
                }

                if probe_done > 0 {
                    let avg_probe_mib_s = probe_measured_mib_s / probe_done as f64;
                    let remaining = content_length - next_start_byte;
                    if remaining > 0 {
                        let tuned = tune_part_size_from_probe(
                            self.auto_profile,
                            remaining,
                            part_size,
                            avg_probe_mib_s,
                        );
                        let cost_optimized = optimize_part_size_for_cost(
                            remaining,
                            tuned,
                            self.auto_profile,
                            same_region_for_auto,
                        );
                        let remaining_slots = (10000 - (next_part_number - 1) as usize).max(1);
                        part_size = clamp_part_size_for_limit(
                            remaining,
                            cost_optimized,
                            remaining_slots as i64,
                        );
                    }
                    if !self.quiet {
                        println!(
                            "🧪 Probe completed in {:.2}s at {:.1} MiB/s. Tuned part size={} MB",
                            probe_start.elapsed().as_secs_f64(),
                            avg_probe_mib_s,
                            part_size / 1024 / 1024
                        );
                    }
                }
            }

            let remaining_bytes = content_length - next_start_byte;
            let remaining_parts = if remaining_bytes > 0 {
                ((remaining_bytes + part_size - 1) / part_size) as usize
            } else {
                0
            };
            let num_parts = completed_parts.len() + remaining_parts;

            if !self.quiet {
                println!(
                    "File size: {:.2} GB",
                    content_length as f64 / (1024.0 * 1024.0 * 1024.0)
                );
                println!("Number of parts: {}", num_parts);
                println!("Final part size: {} MB", part_size / 1024 / 1024);
            }

            let progress = CopyProgress::new(remaining_parts);
            let progress_bar = if self.quiet {
                ProgressBar::hidden()
            } else {
                let pb = ProgressBar::new(remaining_bytes.max(0) as u64);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({percent}%) {binary_bytes_per_sec} ETA: {eta} {msg}")
                        .unwrap()
                        .progress_chars("=>-"),
                );
                pb
            };

            if !self.quiet && remaining_parts > 0 {
                println!("\n📥 Copying parts...\n");
            }

            while next_start_byte < content_length {
                let mut batch = Vec::with_capacity(target_concurrency);
                for _ in 0..target_concurrency {
                    if next_start_byte >= content_length {
                        break;
                    }
                    let end_byte = std::cmp::min(next_start_byte + part_size, content_length) - 1;
                    let range = format!("bytes={}-{}", next_start_byte, end_byte);
                    let part_bytes = (end_byte - next_start_byte + 1) as u64;
                    batch.push((next_part_number, range, part_bytes));
                    next_part_number += 1;
                    next_start_byte = end_byte + 1;
                }

                let (mut window_parts, metrics) = self
                    .run_copy_window(&upload_id, batch, &progress, &progress_bar)
                    .await?;
                completed_parts.append(&mut window_parts);

                if self.auto {
                    let next = adapt_concurrency(
                        self.auto_profile,
                        target_concurrency,
                        4,
                        max_auto_concurrency,
                        metrics,
                    );
                    if next != target_concurrency && !self.quiet {
                        println!(
                            "🤖 Auto Mode: concurrency {} -> {} (avg part {:.1}s, throughput {:.1} MiB/s)",
                            target_concurrency, next, metrics.avg_part_seconds, metrics.throughput_mib_s
                        );
                    }
                    target_concurrency = next;
                }
            }

            if remaining_parts > 0 {
                progress_bar.finish_with_message("All parts copied!");
            }
            if !self.quiet {
                println!("\n✅ All parts copied successfully");
            }
            // Sort parts by part number
            completed_parts.sort_by(|a, b| a.part_number.cmp(&b.part_number));

            // Complete multipart upload
            if !self.quiet {
                println!("\n📦 Completing multipart upload...");
            }
            self.complete_multipart_upload(&upload_id, completed_parts)
                .await?;
            if !self.quiet {
                println!("   ✅ Multipart upload completed successfully!");
            }

            Ok(())
        }
        .await;

        // Cleanup if error occurred during upload
        if let Err(e) = upload_result {
            eprintln!("\n⚠️  Error occurred during upload: {}. Cleaning up...", e);
            if let Err(abort_err) = self.abort_multipart_upload(&upload_id).await {
                eprintln!("   Failed to abort multipart upload: {}", abort_err);
            }
            return Err(e);
        }

        // Verify the copy
        if !self.dry_run && self.verify_integrity != VerifyIntegrity::Off {
            let source_metadata = self
                .source_client
                .head_object()
                .bucket(&self.source_bucket)
                .key(&self.source_key)
                .send()
                .await
                .with_context(|| "Failed to load source metadata for verification")?;
            let dest_metadata = self
                .client
                .head_object()
                .bucket(&self.dest_bucket)
                .key(&self.dest_key)
                .send()
                .await
                .with_context(|| "Failed to verify destination object")?;

            if dest_metadata.content_length != Some(content_length) {
                return Err(anyhow::anyhow!(
                    "Verification failed: source/destination size mismatch ({} != {})",
                    content_length,
                    dest_metadata.content_length.unwrap_or(0)
                ));
            }

            match self.verify_integrity {
                VerifyIntegrity::Off => {}
                VerifyIntegrity::Etag => {
                    let src_etag = source_metadata.e_tag().unwrap_or_default();
                    let dst_etag = dest_metadata.e_tag().unwrap_or_default();
                    if !src_etag.is_empty() && !dst_etag.is_empty() && src_etag != dst_etag {
                        let tracked_src = dest_metadata
                            .metadata()
                            .and_then(|m| m.get("source-etag"))
                            .map(|v| format!("\"{}\"", v.trim_matches('"')))
                            .unwrap_or_default();
                        let normalized_src = format!("\"{}\"", src_etag.trim_matches('"'));
                        if tracked_src != normalized_src {
                            return Err(anyhow::anyhow!(
                                "Verification failed: ETag mismatch and source-etag metadata mismatch"
                            ));
                        }
                    }
                }
                VerifyIntegrity::Checksum => {
                    let provider = HeadObjectChecksumProvider;
                    Self::verify_checksum_with_provider(&provider, &source_metadata, &dest_metadata)?;
                }
            }

            if !self.quiet {
                println!("\n✅ Copy verification successful!");
                println!("   Source size:      {} bytes", content_length);
                println!(
                    "   Destination size: {} bytes",
                    dest_metadata.content_length.unwrap_or(0)
                );
                println!("   Mode:             {:?}", self.verify_integrity);
            }
        } else if !self.quiet {
            println!("\n[Dry Run/Config] Copy verification skipped.");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_s3::Config;
    use mockall::Sequence;

    fn build_test_app(dry_run: bool) -> S3CopyApp {
        let config = Config::builder()
            .region(Region::new("us-east-1"))
            .behavior_version_latest()
            .build();
        let client = Client::from_conf(config);

        S3CopyApp {
            source_client: client.clone(),
            client,
            source_bucket: "src-bucket".to_string(),
            source_key: "src-key".to_string(),
            dest_bucket: "dst-bucket".to_string(),
            dest_key: "dst-key".to_string(),
            part_size: 128 * 1024 * 1024,
            concurrency: 4,
            storage_class: None,
            full_control: false,
            auto: false,
            auto_profile: AutoProfile::Balanced,
            no_metadata: false,
            no_tags: false,
            no_storage_class: false,
            no_acl: false,
            quiet: true,
            dry_run,
            force_copy: false,
            verify_integrity: VerifyIntegrity::Etag,
            checksum_algorithm: None,
            sse: None,
            sse_kms_key_id: None,
        }
    }

    /// Ensures dry-run `upload_part_copy` returns a deterministic stub part without AWS calls.
    #[tokio::test]
    async fn upload_part_copy_dry_run_returns_stub_part() {
        let app = build_test_app(true);
        let part = app
            .upload_part_copy("dry-upload", 1, "bytes=0-1023")
            .await
            .expect("dry-run part copy should succeed");

        assert_eq!(part.part_number, Some(1));
        assert_eq!(part.e_tag.as_deref(), Some("dry-run-etag"));
    }

    /// Verifies dry-run multipart lifecycle methods succeed and return deterministic values.
    #[tokio::test]
    async fn multipart_lifecycle_dry_run_succeeds() {
        let app = build_test_app(true);
        let src_meta = HeadObjectOutput::builder().build();

        let upload_id = app
            .initiate_multipart_upload("src-etag", &src_meta, None)
            .await
            .expect("dry-run initiate should succeed");
        assert_eq!(upload_id, "DRY-RUN-UPLOAD-ID");

        app.complete_multipart_upload(&upload_id, Vec::new())
            .await
            .expect("dry-run complete should succeed");
        app.abort_multipart_upload(&upload_id)
            .await
            .expect("dry-run abort should succeed");
    }

    /// Confirms checksum extraction prefers SHA256 over other checksum headers when available.
    #[test]
    fn extract_checksum_value_prefers_sha256() {
        let meta = HeadObjectOutput::builder()
            .checksum_sha1("sha1-value")
            .checksum_sha256("sha256-value")
            .build();

        let extracted = S3CopyApp::extract_checksum_value(&meta);
        assert_eq!(extracted.as_deref(), Some("SHA256:sha256-value"));
    }

    /// Verifies checksum verification succeeds when mocked source and destination checksums match.
    #[test]
    fn verify_checksum_with_mock_provider_succeeds_on_match() {
        let src_meta = HeadObjectOutput::builder().build();
        let dst_meta = HeadObjectOutput::builder().build();
        let mut mock = MockChecksumProvider::new();
        let mut seq = Sequence::new();

        mock.expect_extract_checksum_value()
            .times(1)
            .in_sequence(&mut seq)
            .return_const(Some("SHA256:abc".to_string()));
        mock.expect_extract_checksum_value()
            .times(1)
            .in_sequence(&mut seq)
            .return_const(Some("SHA256:abc".to_string()));

        let result = S3CopyApp::verify_checksum_with_provider(&mock, &src_meta, &dst_meta);
        assert!(result.is_ok());
    }

    /// Verifies checksum verification fails with an explicit mismatch when mocked checksums differ.
    #[test]
    fn verify_checksum_with_mock_provider_fails_on_mismatch() {
        let src_meta = HeadObjectOutput::builder().build();
        let dst_meta = HeadObjectOutput::builder().build();
        let mut mock = MockChecksumProvider::new();
        let mut seq = Sequence::new();

        mock.expect_extract_checksum_value()
            .times(1)
            .in_sequence(&mut seq)
            .return_const(Some("SHA256:src".to_string()));
        mock.expect_extract_checksum_value()
            .times(1)
            .in_sequence(&mut seq)
            .return_const(Some("SHA256:dst".to_string()));

        let err = S3CopyApp::verify_checksum_with_provider(&mock, &src_meta, &dst_meta)
            .expect_err("mismatched checksums must fail");
        assert!(err.to_string().contains("Checksum mismatch"));
    }

    /// Verifies checksum verification fails when checksum headers are unavailable.
    #[test]
    fn verify_checksum_with_mock_provider_fails_when_missing() {
        let src_meta = HeadObjectOutput::builder().build();
        let dst_meta = HeadObjectOutput::builder().build();
        let mut mock = MockChecksumProvider::new();
        let mut seq = Sequence::new();

        mock.expect_extract_checksum_value()
            .times(1)
            .in_sequence(&mut seq)
            .return_const(None);
        mock.expect_extract_checksum_value()
            .times(1)
            .in_sequence(&mut seq)
            .return_const(None);

        let err = S3CopyApp::verify_checksum_with_provider(&mock, &src_meta, &dst_meta)
            .expect_err("missing checksums must fail");
        assert!(err
            .to_string()
            .contains("Checksum verification requested but checksum headers are not available"));
    }
}
