use clap::Parser;

pub const MIN_PART_SIZE_MB: i64 = 5;
pub const DEFAULT_PART_SIZE_MB: i64 = 256;
pub const MAX_PART_SIZE_MB: i64 = 5 * 1024; // 5GB maximum in MB
pub const DEFAULT_CONCURRENCY: usize = 50;
pub const MAX_CONCURRENT_PARTS: usize = 1000;

/// CLI arguments for the S3 large file copy tool
#[derive(Parser, Debug)]
#[command(name = "s3_largecopy")]
#[command(author, version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("CARGO_PKG_AUTHORS"), ")"), about, long_about = None)]
pub struct Args {
    /// Source S3 bucket name
    #[arg(short, long)]
    pub source_bucket: String,

    /// Source object key
    #[arg(short = 'k', long)]
    pub source_key: String,

    /// Destination S3 bucket name
    #[arg(short = 'b', long)]
    pub dest_bucket: String,

    /// Destination object key
    #[arg(short = 't', long)]
    pub dest_key: String,

    /// AWS region (optional, uses default region if not specified)
    #[arg(short = 'r', long)]
    pub region: Option<String>,

    /// Part size in MB (default: 256, min: 5, max: 5120)
    #[arg(short = 'p', long, value_parser = clap::value_parser!(i64).range(5..=5120))]
    pub part_size: Option<i64>,

    /// Number of concurrent part uploads (default: 50)
    #[arg(long)]
    pub concurrency: Option<usize>,

    /// Target storage class (e.g. STANDARD, INTELLIGENT_TIERING, GLACIER_IR)
    #[arg(long)]
    pub storage_class: Option<String>,

    /// Set bucket-owner-full-control ACL (useful for cross-account copies)
    #[arg(long, default_value_t = false)]
    pub full_control: bool,

    /// Automatically tune part size and concurrency based on object size
    #[arg(long, default_value_t = false)]
    pub auto: bool,

    /// Disable replication of standard and custom metadata
    #[arg(long, default_value_t = false)]
    pub no_metadata: bool,

    /// Disable replication of S3 object tags
    #[arg(long, default_value_t = false)]
    pub no_tags: bool,

    /// Do not inherit storage class from source (use destination default unless --storage-class is provided)
    #[arg(long, default_value_t = false)]
    pub no_storage_class: bool,

    /// Disable applying bucket-owner-full-control ACL
    #[arg(long, default_value_t = false)]
    pub no_acl: bool,

    /// Suppress informational output and progress bars
    #[arg(short, long, default_value_t = false)]
    pub quiet: bool,

    /// Perform a dry run without modifying any data
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Checksum algorithm to use (CRC32, CRC32C, SHA1, SHA256)
    #[arg(long, value_parser = ["CRC32", "CRC32C", "SHA1", "SHA256"])]
    pub checksum_algorithm: Option<String>,

    /// Server-side encryption algorithm (AES256, aws:kms)
    #[arg(long, value_parser = ["AES256", "aws:kms"])]
    pub sse: Option<String>,

    /// KMS Key ID (ARN or Alias) to use with aws:kms encryption
    #[arg(long)]
    pub sse_kms_key_id: Option<String>,

    /// Estimate the cost of the copy operation without executing it
    #[arg(long, default_value_t = false)]
    pub estimate: bool,

    /// Destination region (for cross-region cost estimation; defaults to --region)
    #[arg(long)]
    pub dest_region: Option<String>,
}
