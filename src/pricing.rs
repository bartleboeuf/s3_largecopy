use aws_sdk_pricing::Client;
use aws_sdk_pricing::types::Filter;
use anyhow::{Result, anyhow};

/// Client for AWS Pricing API to fetch S3 costs dynamically.
pub struct S3PricingClient {
    client: Client,
}

impl S3PricingClient {
    pub async fn new(profile: Option<&str>) -> Result<Self> {
        // Pricing API is only available in us-east-1 and ap-south-1 endpoints.
        // We set the region directly on the SdkConfig loader so that all
        // credential providers (including SSO) are correctly resolved.
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new("us-east-1"));
        
        if let Some(p) = profile {
            loader = loader.profile_name(p);
        }

        let config = loader.load().await;
        let client = Client::new(&config);
        Ok(Self { client })
    }

    /// Helper to map region ID to Pricing API location name.
    /// These must exactly match the values returned by the AWS Pricing API.
    fn region_to_location(region: &str) -> &'static str {
        match region {
            "us-east-1" => "US East (N. Virginia)",
            "us-east-2" => "US East (Ohio)",
            "us-west-1" => "US West (N. California)",
            "us-west-2" => "US West (Oregon)",
            "af-south-1" => "Africa (Cape Town)",
            "ap-east-1" => "Asia Pacific (Hong Kong)",
            "ap-east-2" => "Asia Pacific (Taipei)",
            "ap-south-1" => "Asia Pacific (Mumbai)",
            "ap-south-2" => "Asia Pacific (Hyderabad)",
            "ap-northeast-3" => "Asia Pacific (Osaka)",
            "ap-northeast-2" => "Asia Pacific (Seoul)",
            "ap-southeast-1" => "Asia Pacific (Singapore)",
            "ap-southeast-2" => "Asia Pacific (Sydney)",
            "ap-southeast-3" => "Asia Pacific (Jakarta)",
            "ap-southeast-4" => "Asia Pacific (Melbourne)",
            "ap-southeast-5" => "Asia Pacific (Malaysia)",
            "ap-southeast-6" => "Asia Pacific (New Zealand)",
            "ap-southeast-7" => "Asia Pacific (Thailand)",
            "ap-northeast-1" => "Asia Pacific (Tokyo)",
            "ca-central-1" => "Canada (Central)",
            "ca-west-1" => "Canada West (Calgary)",
            "eu-central-1" => "EU (Frankfurt)",
            "eu-central-2" => "Europe (Zurich)",
            "eu-west-1" => "EU (Ireland)",
            "eu-west-2" => "EU (London)",
            "eu-west-3" => "EU (Paris)",
            "eu-north-1" => "EU (Stockholm)",
            "eu-south-1" => "EU (Milan)",
            "eu-south-2" => "Europe (Spain)",
            "il-central-1" => "Israel (Tel Aviv)",
            "me-central-1" => "Middle East (UAE)",
            "me-south-1" => "Middle East (Bahrain)",
            "mx-central-1" => "Mexico (Central)",
            "sa-east-1" => "South America (Sao Paulo)",
            _ => "US East (N. Virginia)",
        }
    }

    /// Map storage class to volumeType used in Pricing API.
    fn storage_class_to_volume_type(storage_class: &str) -> &'static str {
        match storage_class {
            "STANDARD" => "Standard",
            "STANDARD_IA" => "Standard - Infrequent Access",
            "ONEZONE_IA" => "One Zone - Infrequent Access",
            "INTELLIGENT_TIERING" => "Intelligent-Tiering",
            "GLACIER" | "GLACIER_FLEXIBLE_RETRIEVAL" => "Amazon Glacier",
            "DEEP_ARCHIVE" => "Glacier Deep Archive",
            "GLACIER_IR" | "GLACIER_INSTANT_RETRIEVAL" => "Glacier Instant Retrieval",
            "EXPRESS_ONEZONE" => "Express One Zone",
            "REDUCED_REDUNDANCY" => "Reduced Redundancy",
            _ => "Standard",
        }
    }

    /// Map storage class to the storageClass filter value used in Pricing API.
    fn storage_class_to_filter(storage_class: &str) -> &'static str {
        match storage_class {
            "STANDARD" => "General Purpose",
            "STANDARD_IA" | "ONEZONE_IA" => "Infrequent Access",
            "INTELLIGENT_TIERING" => "Intelligent-Tiering",
            "GLACIER" | "GLACIER_FLEXIBLE_RETRIEVAL" => "Archive",
            "DEEP_ARCHIVE" => "Archive",
            "GLACIER_IR" | "GLACIER_INSTANT_RETRIEVAL" => "Archive Instant Retrieval",
            "EXPRESS_ONEZONE" => "High Performance",
            _ => "General Purpose",
        }
    }

    /// Map storage class to the API request group prefix used in Pricing API.
    /// Standard uses "S3-API-Tier1" / "S3-API-Tier2",
    /// Standard-IA uses "S3-API-SIA-Tier1" / "S3-API-SIA-Tier2", etc.
    fn storage_class_to_api_group_prefix(storage_class: &str) -> &'static str {
        match storage_class {
            "STANDARD" => "S3-API",
            "STANDARD_IA" => "S3-API-SIA",
            "ONEZONE_IA" => "S3-API-ZIA",
            "INTELLIGENT_TIERING" => "S3-API-INT",
            "GLACIER" | "GLACIER_FLEXIBLE_RETRIEVAL" => "S3-API-GLACIER",
            "DEEP_ARCHIVE" => "S3-API-DAA",
            "GLACIER_IR" | "GLACIER_INSTANT_RETRIEVAL" => "S3-API-GIR",
            "EXPRESS_ONEZONE" => "S3-API-XZ",
            _ => "S3-API",
        }
    }

    /// Fetch storage price per GB per month for a given region and storage class.
    pub async fn get_storage_price(&self, region: &str, storage_class: &str) -> Result<f64> {
        let location = Self::region_to_location(region);
        let volume_type = Self::storage_class_to_volume_type(storage_class);
        let sc_filter = Self::storage_class_to_filter(storage_class);

        let filters = vec![
            Filter::builder().field("location").value(location).set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
            Filter::builder().field("productFamily").value("Storage").set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
            Filter::builder().field("volumeType").value(volume_type).set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
            Filter::builder().field("storageClass").value(sc_filter).set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
        ];

        let result = self.client.get_products()
            .service_code("AmazonS3")
            .set_filters(Some(filters))
            .send()
            .await?;

        self.extract_first_tier_price(result.price_list(), "GB-Mo")
    }

    /// Fetch price for Class A requests (PUT, COPY, POST, LIST) per 1,000 requests.
    pub async fn get_class_a_request_price(&self, region: &str, storage_class: &str) -> Result<f64> {
        let prefix = Self::storage_class_to_api_group_prefix(storage_class);
        let group = format!("{}-Tier1", prefix);
        
        // Try specific group first
        if let Some(price) = self.get_request_price_by_group(region, &group).await? {
            return Ok(price);
        }
        
        // Fallback to standard if specific group not found
        if prefix != "S3-API" {
            if let Some(price) = self.get_request_price_by_group(region, "S3-API-Tier1").await? {
                return Ok(price);
            }
        }
        
        Err(anyhow!("Could not find Class A request price for {} in {}", storage_class, region))
    }

    /// Fetch price for Class B requests (GET and all other) per 10,000 requests.
    pub async fn get_class_b_request_price(&self, region: &str, storage_class: &str) -> Result<f64> {
        let prefix = Self::storage_class_to_api_group_prefix(storage_class);
        let group = format!("{}-Tier2", prefix);
        
        // Try specific group first
        if let Some(price) = self.get_request_price_by_group(region, &group).await? {
            return Ok(price);
        }
        
        // Fallback to standard if specific group not found
        if prefix != "S3-API" {
            if let Some(price) = self.get_request_price_by_group(region, "S3-API-Tier2").await? {
                return Ok(price);
            }
        }
        
        Err(anyhow!("Could not find Class B request price for {} in {}", storage_class, region))
    }

    /// Internal: fetch request price using the API group filter.
    /// Returns the price per single request as returned by the API.
    async fn get_request_price_by_group(&self, region: &str, group: &str) -> Result<Option<f64>> {
        let location = Self::region_to_location(region);

        let filters = vec![
            Filter::builder().field("location").value(location).set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
            Filter::builder().field("productFamily").value("API Request").set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
            Filter::builder().field("group").value(group).set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
        ];

        let result = self.client.get_products()
            .service_code("AmazonS3")
            .set_filters(Some(filters))
            .send()
            .await?;

        let price_list = result.price_list();
        if price_list.is_empty() {
            return Ok(None);
        }

        self.extract_first_tier_price(price_list, "Requests").map(Some)
    }

    /// Fetch data transfer OUT price per GB (S3 to Internet).
    pub async fn get_data_transfer_price(&self, region: &str) -> Result<f64> {
        let location = Self::region_to_location(region);

        let filters = vec![
            Filter::builder().field("fromLocation").value(location).set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
            Filter::builder().field("transferType").value("AWS Outbound").set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
        ];

        let result = self.client.get_products()
            .service_code("AWSDataTransfer")
            .set_filters(Some(filters))
            .send()
            .await?;

        self.extract_first_tier_price(result.price_list(), "GB")
    }

    /// Fetch cross-region data transfer price between two regions.
    pub async fn get_cross_region_transfer_price(&self, from_region: &str, to_region: &str) -> Result<f64> {
        let filters = vec![
            Filter::builder().field("fromRegionCode").value(from_region).set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
            Filter::builder().field("toRegionCode").value(to_region).set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
            Filter::builder().field("transferType").value("InterRegion Outbound").set_type(Some(aws_sdk_pricing::types::FilterType::TermMatch)).build()?,
        ];

        let result = self.client.get_products()
            .service_code("AWSDataTransfer")
            .set_filters(Some(filters))
            .send()
            .await?;

        let price_list = result.price_list();
        if price_list.is_empty() {
            return Err(anyhow!("No cross-region transfer price found between {} and {}", from_region, to_region));
        }

        // Prefer results with empty operation (standard data transfer)
        let filtered_list: Vec<String> = price_list.iter()
            .filter(|json| json.contains("\"operation\": \"\"") || json.contains("\"operation\":\"\""))
            .cloned()
            .collect();

        if !filtered_list.is_empty() {
            self.extract_first_tier_price(&filtered_list, "GB")
        } else {
            self.extract_first_tier_price(price_list, "GB")
        }
    }

    /// Display regional pricing information.
    pub async fn display_pricing(&self, region: &str, storage_class: &str, dest_region_opt: Option<&String>) -> Result<()> {
        let storage_cost = self.get_storage_price(region, storage_class).await?;
        let put_cost = self.get_class_a_request_price(region, storage_class).await?;
        let get_cost = self.get_class_b_request_price(region, storage_class).await?;

        println!("\n📦 S3 Regional Pricing for {} in {}", storage_class, region);
        println!("─────────────────────────────────────────");
        println!("  Storage:                    ${:.4} per GB-Mo", storage_cost);
        println!("  PUT/COPY/POST/LIST requests: ${:.10} per request (${:.4} per 1,000)", put_cost, put_cost * 1000.0);
        println!("  GET and all other requests:  ${:.10} per request (${:.4} per 10,000)", get_cost, get_cost * 10000.0);
        
        if let Some(dest_region) = dest_region_opt {
            if region == dest_region {
                println!("  Data Transfer to {}:    FREE (same region)", dest_region);
            } else {
                match self.get_cross_region_transfer_price(region, dest_region).await {
                    Ok(transfer_cost) => {
                        println!("  Data Transfer to {}:    ${:.4} per GB", dest_region, transfer_cost);
                    }
                    Err(_) => {
                        if let Ok(transfer_cost) = self.get_data_transfer_price(region).await {
                            println!("  Data Transfer (Standard):   ${:.4} per GB", transfer_cost);
                        }
                    }
                }
            }
        } else {
            if let Ok(transfer_cost) = self.get_data_transfer_price(region).await {
                println!("  Data Transfer to Internet:  ${:.4} per GB", transfer_cost);
            }
        }
        println!();

        Ok(())
    }

    /// Parse the Pricing API JSON and extract the first-tier OnDemand price.
    /// Selects the price dimension where beginRange == "0" (cheapest tier).
    fn extract_first_tier_price(&self, price_list: &[String], unit_contains: &str) -> Result<f64> {
        let unit_lower = unit_contains.to_lowercase();

        for product_json in price_list {
            let v: serde_json::Value = serde_json::from_str(product_json)?;

            if let Some(on_demand) = v.get("terms").and_then(|t| t.get("OnDemand")) {
                for (_term_id, term_val) in on_demand.as_object().ok_or_else(|| anyhow!("Invalid OnDemand format"))? {
                    if let Some(price_dimensions) = term_val.get("priceDimensions") {
                        // Collect all matching dimensions and pick the first tier (beginRange == "0")
                        let mut first_tier_price: Option<f64> = None;
                        let mut any_tier_price: Option<f64> = None;

                        for (_dim_id, dim_val) in price_dimensions.as_object().ok_or_else(|| anyhow!("Invalid priceDimensions format"))? {
                            let unit = dim_val.get("unit").and_then(|u| u.as_str()).unwrap_or("");
                            if !unit.to_lowercase().contains(&unit_lower) {
                                continue;
                            }

                            if let Some(price_per_unit) = dim_val.get("pricePerUnit").and_then(|p| p.get("USD")) {
                                if let Some(price_str) = price_per_unit.as_str() {
                                    let price = price_str.parse::<f64>()?;
                                    any_tier_price = Some(price);

                                    let begin_range = dim_val.get("beginRange").and_then(|b| b.as_str()).unwrap_or("");
                                    if begin_range == "0" {
                                        first_tier_price = Some(price);
                                    }
                                }
                            }
                        }

                        // Prefer the first tier; fall back to any matching tier
                        if let Some(price) = first_tier_price.or(any_tier_price) {
                            return Ok(price);
                        }
                    }
                }
            }
        }
        Err(anyhow!("Could not find price for unit containing '{}' in results", unit_contains))
    }
}
