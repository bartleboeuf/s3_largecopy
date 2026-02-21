# S3 Copy Cost Analysis

This document provides a cost breakdown for copying large datasets using the optimized `s3_largecopy` tool. You can use the built-in pricing tool to see live data for your specific configuration.

## Check Live Pricing
Run the following command to get current pricing for any region and storage class:
```bash
s3_largecopy --get-price --region us-east-1 --storage-class STANDARD
```

## Scenario
- **Total Data**: 1 TB (10 files of 100 GB each)
- **Part Size**: 256 MB (Optimized)
- **AWS Region**: US-East-1 (Standard rates)

## Cost Breakdown

### 1. API Request Charges (Class A)
S3 charges for `PUT`, `COPY`, and `Multipart Upload` operations. 
Using a 256 MB part size significantly reduces the number of requests compared to the default 5 MB limit.

| Operation | Quantity | Rate (per 1,000) | Total Cost |
|-----------|----------|------------------|------------|
| `UploadPartCopy` | 4,000 | $0.005 | $0.02 |
| `Create/CompleteMultipartUpload` | 20 | $0.005 | $0.00 |
| **Total Request Cost** | | | **$0.02** |

### 2. Data Transfer Charges

| Scenario | Rate (per GB) | Total Cost |
|----------|---------------|------------|
| **Same Region (Intra-Region)** | $0.00 | **$0.00** |
| **Different Region (Cross-Region)** | $0.02 | **$20.00** |

> [!NOTE]
> Because this tool uses `UploadPartCopy`, data is transferred directly between S3 servers. You **do not** pay for data transfer out of S3 to your local host.

### 3. Redundancy Check Costs
The "Smart Redundancy Check" uses `HeadObject` (Class B) requests.
- **Quantity**: ~20 requests
- **Cost**: Negligible (< $0.0001)

### 4. Monthly Storage Costs (S3 Standard)
Once the files are copied, they incur standard storage fees.
- **Rate**: $0.023 per GB (first 50 TB)
- **Total**: **$23.00 / month**

---

## Comparison: Optimized vs. Default
By using a **256 MB** part size instead of the minimum **5 MB**:
- **API Requests**: Reduced from 200,000 to 4,000.
- **Request Savings**: ~$1.00 per TB copied.
- **Performance**: Significant reduction in total latency due to fewer round-trips.

---

*Generated on: 2026-02-07*
