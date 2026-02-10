use crate::progress::CopyProgress;
use anyhow::{Context, Result};
use aws_sdk_s3::operation::head_object::HeadObjectOutput;
use aws_sdk_s3::types::{
    ChecksumAlgorithm, CompletedPart, ObjectCannedAcl, ServerSideEncryption, StorageClass, Tag,
    Tagging,
};
use aws_sdk_s3::{config::Region, Client};
use aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder;
use aws_smithy_types::retry::RetryConfig;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::{atomic::Ordering, Arc};
use tokio::sync::Semaphore;
use tokio::task;

/// Main application structure
#[derive(Clone)]
pub struct S3CopyApp {
    client: Client,
    source_bucket: String,
    source_key: String,
    dest_bucket: String,
    dest_key: String,
    part_size: i64,
    concurrency: usize,
    storage_class: Option<StorageClass>,
    full_control: bool,
    auto: bool,
    no_metadata: bool,
    no_tags: bool,
    no_storage_class: bool,
    no_acl: bool,
    pub quiet: bool,
    pub dry_run: bool,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    pub sse: Option<ServerSideEncryption>,
    pub sse_kms_key_id: Option<String>,
}

impl S3CopyApp {
    /// Create a new S3CopyApp instance
    pub async fn new(
        source_bucket: String,
        source_key: String,
        dest_bucket: String,
        dest_key: String,
        region: Option<String>,
        part_size: i64,
        concurrency: usize,
        storage_class: Option<String>,
        full_control: bool,
        auto: bool,
        no_metadata: bool,
        no_tags: bool,
        no_storage_class: bool,
        no_acl: bool,
        quiet: bool,
        dry_run: bool,
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

        // Autotune concurrency if auto is set
        let final_concurrency = if auto { 100 } else { concurrency };

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
        let max_attempts = if auto { 10 } else { 5 };
        let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .http_client(http_client)
            .retry_config(RetryConfig::standard().with_max_attempts(max_attempts));

        if let Some(r) = region {
            config_loader = config_loader.region(Region::new(r));
        }

        let config = config_loader.load().await;
        let client = Client::new(&config);

        Ok(Self {
            client,
            source_bucket,
            source_key,
            dest_bucket,
            dest_key,
            part_size,
            concurrency: final_concurrency,
            storage_class,
            full_control,
            auto,
            no_metadata,
            no_tags,
            no_storage_class,
            no_acl,
            quiet,
            dry_run,
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
        match self
            .client
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

    /// Get object tagging
    async fn get_object_tagging(&self, bucket: &str, key: &str) -> Result<Option<Vec<Tag>>> {
        match self
            .client
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
                println!("   [Dry Run] Would initiate multipart upload (dest: s3://{}/{})", self.dest_bucket, self.dest_key);
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
                println!("   [Dry Run] Would complete multipart upload (upload_id: {})", upload_id);
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
                println!("   [Dry Run] Would abort multipart upload (upload_id: {})", upload_id);
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

        // Check if destination exists and is identical
        if let Some(dest_metadata) = self
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
                            println!("   [Dry Run] Would sync properties via CopyObject (REPLACE directive)");
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
                        println!("   S3 requires a full copy for metadata updates > 5GB. Proceeding with Multipart Copy...");
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

        // Check if file is larger than 5GB
        let five_gb: i64 = 5 * 1024 * 1024 * 1024;
        if content_length < five_gb {
            if self.auto {
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
                            builder = builder
                                .tagging_directive(aws_sdk_s3::types::TaggingDirective::Replace);
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

            if !self.quiet {
                println!(
                    "⚠️  Warning: File size ({:.2} GB) is less than 5GB. Consider using standard copy.",
                    content_length as f64 / (1024.0 * 1024.0 * 1024.0)
                );
            }
        }

        // Calculate number of parts with adaptive sizing (S3 limit: 10,000 parts)
        let mut part_size = self.part_size;

        if self.auto {
            let hundred_gb = 100 * 1024 * 1024 * 1024;
            let one_tb = 1024 * 1024 * 1024 * 1024;
            let ten_tb = 10 * 1024 * 1024 * 1024 * 1024;

            part_size = if content_length < hundred_gb {
                128 * 1024 * 1024 // 128MB
            } else if content_length < one_tb {
                256 * 1024 * 1024 // 256MB
            } else if content_length < ten_tb {
                512 * 1024 * 1024 // 512MB
            } else {
                1024 * 1024 * 1024 // 1GB
            };
            if !self.quiet {
                println!(
                    "🤖 Auto Mode: Tuned initial part size to {} MB",
                    part_size / 1024 / 1024
                );
            }
        }

        let max_s3_parts = 10000;
        if (content_length + part_size - 1) / part_size > max_s3_parts {
            // Adjust part size to stay within 10,000 parts limit
            let min_adaptive_size =
                (content_length / 9500 + 1024 * 1024 - 1) / (1024 * 1024) * 1024 * 1024;
            part_size = std::cmp::max(part_size, min_adaptive_size);
            if !self.quiet {
                println!(
                    "⚠️  Adaptive Sizing: Adjusting part size to {} MB to stay within S3 limits",
                    part_size / 1024 / 1024
                );
            }
        }

        let num_parts = ((content_length + part_size - 1) / part_size) as usize;
        if !self.quiet {
            println!(
                "File size: {:.2} GB",
                content_length as f64 / (1024.0 * 1024.0 * 1024.0)
            );
            println!("Number of parts: {}", num_parts);
            println!("Final part size: {} MB", part_size / 1024 / 1024);
        }

        // Create progress bar
        let progress = CopyProgress::new(num_parts);
        let progress_bar = if self.quiet {
            ProgressBar::hidden()
        } else {
            let pb = ProgressBar::new(content_length as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({percent}%) {binary_bytes_per_sec} ETA: {eta} {msg}")
                    .unwrap()
                    .progress_chars("=>-"),
            );
            pb
        };

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
            // Create semaphore for concurrency control
            let semaphore = Arc::new(Semaphore::new(self.concurrency));
            let mut handles = Vec::new();

            // Upload parts concurrently
            if !self.quiet {
                println!("\n📥 Copying parts...\n");
            }

            for part_num in 1..=num_parts {
                let start_byte = (part_num as i64 - 1) * part_size;
                let end_byte = std::cmp::min(part_num as i64 * part_size, content_length) - 1;
                let range = format!("bytes={}-{}", start_byte, end_byte);
                let part_size_bytes = (end_byte - start_byte + 1) as u64;

                let app = self.clone();
                let upload_id = upload_id.clone();
                let semaphore = semaphore.clone();
                let progress = progress.clone();
                let progress_bar = progress_bar.clone();

                let handle = task::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();

                    let completed_part = app
                        .upload_part_copy(&upload_id, part_num as i32, &range)
                        .await?;

                    // Update progress
                    progress.add_completed(part_size_bytes);
                    progress_bar.set_position(
                        progress
                            .copied_bytes
                            .load(Ordering::SeqCst),
                    );
                    let completed = progress
                        .completed_parts
                        .load(Ordering::SeqCst);
                    let total = progress.total_parts;
                    progress_bar.set_message(format!("{}/{} parts completed", completed, total));

                    Ok::<_, anyhow::Error>((part_num as i32, completed_part))
                });

                handles.push(handle);
            }

            // Wait for all parts to complete and collect them
            let mut completed_parts = Vec::new();
            for handle in handles {
                let result = handle.await??;
                completed_parts.push(result.1);
            }

            progress_bar.finish_with_message("All parts copied!");
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
        if !self.dry_run {
            let dest_metadata = self
                .client
                .head_object()
                .bucket(&self.dest_bucket)
                .key(&self.dest_key)
                .send()
                .await
                .with_context(|| "Failed to verify destination object")?;

            if dest_metadata.content_length == Some(content_length) {
                if !self.quiet {
                    println!("\n✅ Copy verification successful!");
                    println!("   Source size:      {} bytes", content_length);
                    println!(
                        "   Destination size: {} bytes",
                        dest_metadata.content_length.unwrap_or(0)
                    );
                }
            } else {
                if !self.quiet {
                    println!("\n⚠️  Warning: Size mismatch detected!");
                }
            }
        } else if !self.quiet {
            println!("\n[Dry Run] Copy verification skipped.");
        }

        Ok(())
    }
}
