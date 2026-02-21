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

How it works: Use S3 Transfer Acceleration for supported transfer paths.
Why: S3 Transfer Acceleration can improve transfer speed for long-distance paths, but it adds acceleration charges and should be optional based on cost/performance needs.
Implementation: Add a --transfer-acceleration flag.

## Cross-Partition support
Offer a simple way to copy between AWS partitions.

Feature: Add --partition-source and --partition-dest flags.
Why: AWS partitions are different regions, but they are not accessible from each other.

## Add a progress bar for the overall transfer

Feature: Add a progress bar for the overall transfer.
Why: It is useful to see the progress of the transfer.
Implementation: Add a progress bar for the overall transfer.

## Enhance errors messages

Feature: Show clear error messages and not the exceptions from the API or the rust language.
Why: Current messages are now really understandable to most of the users.
Implementation: Make some human readable messages based on error patterns. For some, like SCP errors, give some advice in the message.
