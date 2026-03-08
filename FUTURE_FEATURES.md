# Future Features

## Cross-Region optimizations
Optimize the transfer between regions beyond the generic autopilot settings.

How it works: Automatically detect when S3 Transfer Acceleration or multi-threaded routes would reduce latency and surface the cost tradeoff to the user.
Why: Cross-region copies still spend most of their cost on long-haul transfers, so highlighting transfer acceleration per region gives teams a predictable knob for performance.
Implementation: Add a `--transfer-acceleration` flag, surface the estimated acceleration surcharge in the `--estimate` output, and optionally recommend it when latency is the bottleneck.

## Cross-Partition support
Offer a simple way to copy between AWS partitions (consumer/standard, govcloud, china).

Feature: Add `--partition-source` and `--partition-dest` flags plus built-in credential chaining for partition-specific endpoints.
Why: Customers moving data into GovCloud or the China partition currently have to script around different endpoints and credentials.
Implementation: Introduce partition-aware clients, and keep the CLI options minimal by tying the partition flags to the detected bucket region when possible.

## Transfer validation and sync audits
Add post-copy validation to ensure the destination data matches the source metadata.

Feature: `--verify-esit` (existence + size + optional checksum) that runs in parallel after copy completion.
Why: Heavy-duty migrations require proof that nothing was missed and no silent failures occurred.
Implementation: Reuse the existing metadata/tag copying logic and extend `--estimate` to project validation time so users can plan their window.

## Observable telemetry hooks
Expose hooks for integration with dashboards, so the tool can track throughput or cost trends.

Feature: Emit structured JSON progress events (file name, bytes transferred, errors) to a socket or file path.
Why: Teams running bulk migrations often want to feed progress into CloudWatch, Splunk, or their own dashboards.
Implementation: Add `--progress-sink` (path or unix socket) that writes newline-delimited JSON events alongside the existing human-friendly UI.
