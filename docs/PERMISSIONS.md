# IAM Permissions

This document lists the AWS permissions required to run the S3 Large File Copy Tool.

## Required Permissions

### Source Bucket
- `s3:GetObject`: Read the source object/data.
- `s3:GetObjectAttributes`: Retrieve ETag and size.
- `s3:GetObjectTagging`: Retrieve object tags.
- `s3:GetBucketLocation`: Detect bucket region (required for estimation).

### Destination Bucket
- `s3:GetBucketLocation`: Detect bucket region.
- `s3:CreateMultipartUpload`: Start the multipart process.
- `s3:UploadPartCopy`: Copy data directly from the source.
- `s3:CompleteMultipartUpload`: Finalize the upload.
- `s3:AbortMultipartUpload`: Cleanup temporary parts on failure.
- `s3:PutObject`: Write the final object.
- `s3:PutObjectTagging`: Replicate tags.
- `s3:PutObjectAcl`: Apply cross-account ownership (if using `--full-control`).

### Pricing API (Optional)
Required only if using `--estimate` or `--get-price`:
- `pricing:GetProducts`: Fetch real-time S3 pricing data.

## IAM Policy Example

```json
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Sid": "SourceBucketPermissions",
            "Effect": "Allow",
            "Action": [
                "s3:GetObject",
                "s3:GetObjectAttributes",
                "s3:GetObjectTagging",
                "s3:GetBucketLocation"
            ],
            "Resource": [
                "arn:aws:s3:::source-bucket",
                "arn:aws:s3:::source-bucket/*"
            ]
        },
        {
            "Sid": "DestBucketPermissions",
            "Effect": "Allow",
            "Action": [
                "s3:PutObject",
                "s3:PutObjectTagging",
                "s3:PutObjectAcl",
                "s3:CreateMultipartUpload",
                "s3:UploadPartCopy",
                "s3:CompleteMultipartUpload",
                "s3:AbortMultipartUpload",
                "s3:ListMultipartUploadParts",
                "s3:GetBucketLocation"
            ],
            "Resource": [
                "arn:aws:s3:::dest-bucket",
                "arn:aws:s3:::dest-bucket/*"
            ]
        },
        {
            "Sid": "PricingPermissions",
            "Effect": "Allow",
            "Action": [
                "pricing:GetProducts"
            ],
            "Resource": "*"
        }
    ]
}
```
