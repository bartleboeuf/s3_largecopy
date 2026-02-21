# Architecture and How It Works

This document details the internal logic and architecture of the S3 Large File Copy Tool.

## Decision Flow

The application follows a complex decision process to ensure efficiency and data integrity:

```mermaid
flowchart TD
    A[Start] --> B{--estimate?}

    B -- Yes --> C[HeadObject source]
    C --> D[Estimate strategy and requests]
    D --> E[Print cost report]
    E --> Z[Exit]

    B -- No --> F[HeadObject source and destination]
    F --> G{--force-copy?}

    G -- No --> H{Destination exists and matches data?}
    H -- Yes --> I{Properties/tags/storage class match?}
    I -- Yes --> J[Skip copy]
    I -- No --> K{Small enough for property-only sync?}
    K -- Yes --> L[CopyObject REPLACE]
    K -- No --> M{Only tags differ?}
    M -- Yes --> N[PutObjectTagging]
    M -- No --> O[Continue to full copy]
    H -- No --> O
    G -- Yes --> O

    O --> P{--auto and size < 5 GiB?}
    P -- Yes --> Q[Instant Copy via CopyObject]
    P -- No --> R{--auto?}

    R -- No --> S[Manual part size and concurrency cap]
    R -- Yes --> T[Build auto plan]
    T --> T1{Auto profile}
    T1 -- cost-efficient --> U1[Prefer larger parts + lower concurrency]
    T1 -- balanced/aggressive/conservative --> U2[Balanced speed/reliability/cost tuning]
    U1 --> U[Apply cost-aware part-size floor]
    U2 --> U
    U --> V[Clamp for S3 multipart limits]
    V --> W[Warm-up probe]
    W --> X[Retune size from throughput]
    X --> Y[Re-apply cost floor]
    Y --> AA[Windowed multipart copy]
    AA --> AB[Adapt concurrency each window]
    AB --> AC{More parts?}
    AC -- Yes --> AA
    AC -- No --> AD[CompleteMultipartUpload]

    S --> AE[Multipart copy]
    AE --> AD

    J --> AF[Post-copy verification mode]
    L --> AF
    N --> AF
    Q --> AF
    AD --> AF
    AF --> AG[Done]
```

## Internal Architecture

The application is structured into modular layers for maintainability:

```mermaid
graph TD
    subgraph CLI Layer
        Main[main.rs]
        Args[args.rs]
    end

    subgraph Core Logic
        App[app.rs - S3CopyApp]
        Auto[auto.rs - Strategy Engine]
        Progress[progress.rs - UI/UX]
    end

    subgraph Service Layer
        Pricing[pricing.rs - AWS Pricing API]
        S3Utils[s3_utils.rs - Bucket Detection]
        Estimate[estimate.rs - Cost Orchestration]
    end

    Main --> Args
    Main --> App
    Main --> Pricing
    Main --> Estimate
    Main --> S3Utils
    
    App --> Auto
    App --> Progress
    Estimate --> Pricing
    Estimate --> App
```

### Module Descriptions
- **`app.rs`**: The primary state machine. Coordinates the multipart upload lifecycle.
- **`auto.rs`**: The "brain" of the tool. Calculates part sizes, throughput-based adjustments, and adaptive concurrency.
- **`pricing.rs`**: Fetches real-time cost data from the AWS Price List API.
- **`estimate.rs`**: Logic for dry-run cost projections.
- **`progress.rs`**: Handles the terminal UI and throughput statistics.

## Part Size Guidelines

- **Min size**: 5 MB (S3 requirement)
- **Max size**: 5 GB per part
- **Max parts**: 10,000 per object
- **Adaptive behavior**: If the file size exceeds ~2.5 TB with the default 256 MB parts, the tool automatically grows the part size to remain under the 10,000-part limit.
