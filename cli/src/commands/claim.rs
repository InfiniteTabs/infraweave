use env_common::interface::GenericCloudHandler;
use env_common::logic::{destroy_infra, driftcheck_infra};
use env_defs::{CloudProvider, DeploymentManifest, ExtraData};
use log::{error, info};
use serde::Deserialize;
use std::path::Path;

use crate::run::run_claim_file;
use crate::utils::current_region_handler;
use crate::{follow_execution, ClaimJobStruct};

pub async fn handle_plan(
    environment: &str,
    claim: &str,
    store_files: bool,
    destroy: bool,
    follow: bool,
) {
    if !follow {
        eprintln!("Error: Plan operations require --follow flag to be enabled.");
        eprintln!("Usage: infraweave plan {} {} --follow", environment, claim);
        std::process::exit(1);
    }

    run_claim_file(environment, claim, "plan", store_files, destroy, follow)
        .await
        .unwrap();
}

pub async fn handle_driftcheck(deployment_id: &str, environment: &str, remediate: bool) {
    match driftcheck_infra(
        &current_region_handler().await,
        deployment_id,
        environment,
        remediate,
        ExtraData::None,
    )
    .await
    {
        Ok(_) => {
            info!("Successfully requested drift check");
        }
        Err(e) => {
            error!("Failed to request drift check: {}", e);
            std::process::exit(1);
        }
    };
}

pub async fn handle_apply(environment: &str, claim: &str, store_files: bool, follow: bool) {
    match run_claim_file(environment, claim, "apply", store_files, false, follow).await {
        Ok(_) => {
            info!("Successfully applied claim");
        }
        Err(e) => {
            error!("Failed to apply claim: {}", e);
            std::process::exit(1);
        }
    };
}

pub async fn handle_destroy(
    deployment_id_or_path: &str,
    environment: &str,
    version: Option<&str>,
    store_files: bool,
    follow: bool,
) {
    let region_handler = current_region_handler().await;

    // Warn if user wants to store files but didn't enable following
    if store_files && !follow {
        eprintln!(
            "Warning: --store-files requires --follow to be enabled. Files will not be stored."
        );
        eprintln!("Add --follow to enable file storage.");
    }

    // Check if input is a file and extract deployment IDs if so
    struct DeploymentTarget {
        id: String,
        region: Option<String>,
    }
    let mut targets = Vec::new();

    let path = Path::new(deployment_id_or_path);
    if path.exists() && path.is_file() {
        if let Ok(content) = std::fs::read_to_string(path) {
            let docs: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&content)
                .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                .collect();

            for doc in docs {
                if let Ok(manifest) = serde_yaml::from_value::<DeploymentManifest>(doc) {
                    targets.push(DeploymentTarget {
                        id: format!(
                            "{}/{}",
                            manifest.kind.to_lowercase(),
                            manifest.metadata.name
                        ),
                        region: Some(manifest.spec.region),
                    });
                }
            }
        }
    }

    if targets.is_empty() {
        targets.push(DeploymentTarget {
            id: deployment_id_or_path.to_string(),
            region: None,
        });
    }

    let mut job_ids = Vec::new();
    let mut deployment_ids = Vec::new();

    for target in &targets {
        // Use region from the file if available, otherwise use default
        let handler = if let Some(region) = &target.region {
            GenericCloudHandler::region(region).await
        } else {
            current_region_handler().await
        };

        let job_id = match destroy_infra(
            &handler,
            &target.id,
            environment,
            ExtraData::None,
            version,
        )
        .await
        {
            Ok(job_id) => {
                info!(
                    "Successfully requested destroying deployment: {}",
                    target.id
                );
                job_id
            }
            Err(e) => {
                error!(
                    "Failed to request destroying deployment {}: {}",
                    target.id, e
                );
                std::process::exit(1);
            }
        };
        job_ids.push(job_id);
        deployment_ids.push(target.id.clone());
    }

    if follow {
        // Get region from the handler (use the last one if multiple, or default)
        // Note: following multiple deployments across regions isn't perfectly supported by this loop logic
        // but typically claims are in one region.
        let region = if let Some(first) = targets.first() {
            if let Some(r) = &first.region {
                r.clone()
            } else {
                region_handler.get_region().to_string()
            }
        } else {
            region_handler.get_region().to_string()
        };

        let job_structs: Vec<ClaimJobStruct> = job_ids
            .iter()
            .zip(deployment_ids.iter())
            .map(|(job_id, dep_id)| ClaimJobStruct {
                job_id: job_id.clone(),
                deployment_id: dep_id.clone(),
                environment: environment.to_string(),
                region: region.clone(),
            })
            .collect();

        match follow_execution(&job_structs, "destroy").await {
            Ok((overview, std_output, _violations)) => {
                info!("Successfully followed destroy operation");

                if store_files {
                    std::fs::write("overview.txt", overview)
                        .expect("Failed to write overview file");
                    println!("Overview written to overview.txt");

                    std::fs::write("std_output.txt", std_output)
                        .expect("Failed to write std output file");
                    println!("Std output written to std_output.txt");
                }
            }
            Err(e) => {
                error!("Failed to follow destroy operation: {}", e);
                std::process::exit(1);
            }
        }
    }
}
