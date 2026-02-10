# Future Features

## Recursive Synchronization
The current tool works file-to-file. The most high-impact feature would be implementing directory syncing similar to aws s3 sync.

How it works: Iterate through a source prefix, list objects, and map them to the destination.
Why: It turns the tool from a "large file copier" into a "large dataset migration tool."
Implementation: I would need to add list_objects_v2 logic and loop the existing 
copy_file logic over the results.

## Advanced Filtering (Include/Exclude)
If I implement recursive syncing, users will need to filter what gets copied.

Feature: Add --include "*.iso" or --exclude "*.tmp" flags.
Implementation: Simple glob pattern matching on keys before initiating the copy.

## Cross-Region optimizations
Optimizing the transfer between regions is critical for performance.

How it works: Use S3 Transfer Acceleration or S3 Transfer Acceleration for cross-region transfers.
Why: S3 Transfer Acceleration is a feature that allows you to transfer data to and from S3 at a lower cost and with a faster transfer rate.
Implementation: Add a --transfer-acceleration flag.

## Cross-Partition support
Offer a simple way to copy between AWS partitions.

Feature: Add --partition-source and --partition-dest flags.
Why: AWS partitions are different regions, but they are not accessible from each other.

## Add a progress bar for the overall transfer

Feature: Add a progress bar for the overall transfer.
Why: It is useful to see the progress of the transfer.
Implementation: Add a progress bar for the overall transfer.

## Dynamic cost estimation

Feature: Use aws pricing api to estimate the cost of the transfer.
Why: Currently, we can estimate the cost of the transfer using a static values for each region at a point in time.