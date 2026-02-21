use anyhow::Result;
use aws_sdk_s3::Client;

/// Detect the region of an S3 bucket.
pub async fn get_bucket_region(client: &Client, bucket: &str, user_override: Option<&String>) -> Result<String> {
    if let Some(r) = user_override {
        return Ok(r.clone());
    }
    match client.get_bucket_location().bucket(bucket).send().await {
        Ok(loc) => {
            if let Some(constraint) = loc.location_constraint() {
                let s = constraint.as_str();
                if s == "EU" {
                    Ok("eu-west-1".to_string())
                } else {
                    Ok(s.to_string())
                }
            } else {
                Ok("us-east-1".to_string())
            }
        }
        Err(e) => {
            let err_str = format!("{}", e);
            if err_str.contains("NoSuchBucket") || err_str.contains("NotFound") || err_str.contains("not found") {
                anyhow::bail!("Error: Bucket '{}' does not exist.", bucket);
            } else if err_str.contains("AccessDenied") || err_str.contains("Access Denied") {
                anyhow::bail!("Error: Access denied for bucket '{}'. Please check your permissions or credentials.", bucket);
            }

            let service_err = e.into_service_error();
            anyhow::bail!(
                "Error: Could not verify bucket '{}'. Ensure it exists and you have access.\nDetails: {}",
                bucket,
                service_err.to_string()
            );
        }
    }
}
