# Troubleshooting and Performance

This document covers common issues and tips for getting the best performance.

## Error Handling
- **Automatic Cleanup**: If a transfer fails, the tool automatically attempts to call `AbortMultipartUpload` on the destination to prevent you from being charged for incomplete parts.
- **Redundancy**: If the tool detects that the destination file already matches the source (Size + ETag), it will skip the copy unless `--force-copy` is used.

## Performance Tips

1. **Enable `--auto` Mode**: This is usually the best way to get maximum throughput.
2. **EC2 Proximity**: Run the tool from an EC2 instance in the same region as your destination bucket for the fastest transfer speeds (using the AWS backbone).
3. **Network Concurrency**: In high-latency environments (cross-continental), increase `--concurrency` (e.g., 100-200) to keep the pipe full.
4. **Part Sizes**: For multi-terabyte files, larger part sizes (500MB+) help reduce the number of API calls and improve overhead efficiency.

## Common Issues

### Access Denied
- **Cause**: Missing IAM permissions or bucket policy restrictions.
- **Fix**: Check [Permissions Guide](./PERMISSIONS.md) and ensure your user/role has access to BOTH buckets.

### NoSuchBucket
- **Cause**: Typos in bucket names or checking the wrong region.
- **Fix**: Verify bucket names and use the `--region` flag if your environment doesn't have a default region set.

### Slow Transfers
- **Cause**: Network throttling or local CPU bottlenecks.
- **Fix**: Check your instance's network throughput limits. Use `--auto` to let the tool find the optimal settings for your specific hardware.
