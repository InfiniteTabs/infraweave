use colored::Colorize;
use log::error;

use crate::current_region_handler;
use env_defs::{CloudProvider, CloudProviderCommon};
use std::fs::File;
use std::io::Write;

pub async fn handle_describe(deployment_id: &str, environment: &str) {
    match current_region_handler()
        .await
        .get_deployment_and_dependents(deployment_id, environment, false)
        .await
    {
        Ok((deployment, _)) => {
            if let Some(deployment) = deployment {
                println!(
                    "Deployment: {}",
                    serde_json::to_string_pretty(&deployment).unwrap()
                );
            } else {
                eprintln!(
                    "{}",
                    format!("Deployment not found: {}", deployment_id).red()
                );
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("{}", format!("Error: {}", e).red());
            std::process::exit(1);
        }
    }
}

pub async fn handle_list(project: Option<&str>, region: Option<&str>) {
    let mut all_deployments = Vec::new();

    if let (Some(p), Some(r)) = (project, region) {
        let handler = env_common::interface::GenericCloudHandler::workload(p, r).await;
        match handler.get_all_deployments("", false).await {
            Ok(deps) => all_deployments.extend(deps),
            Err(e) => {
                eprintln!("{}", format!("Error: {}", e).red());
                std::process::exit(1);
            }
        }
    } else if let Some(p) = project {
        // Use project with current region
        let current_region =
            std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let handler =
            env_common::interface::GenericCloudHandler::workload(p, &current_region).await;

        match handler.get_all_deployments("", false).await {
            Ok(deps) => all_deployments.extend(deps),
            Err(e) => {
                eprintln!("{}", format!("Error: {}", e).red());
                std::process::exit(1);
            }
        }
    } else {
        let handler = current_region_handler().await;
        match handler.get_all_projects().await {
            Ok(projects) => {
                for project_data in projects {
                    let regions_to_check = if let Some(target_region) = region {
                        if project_data.regions.contains(&target_region.to_string()) {
                            vec![target_region.to_string()]
                        } else {
                            vec![]
                        }
                    } else {
                        project_data.regions
                    };

                    for r in regions_to_check {
                        let h = env_common::interface::GenericCloudHandler::workload(
                            &project_data.project_id,
                            &r,
                        )
                        .await;
                        match h.get_all_deployments("", false).await {
                            Ok(deps) => all_deployments.extend(deps),
                            Err(e) => {
                                error!(
                                    "Failed to fetch deployments for {}/{}: {}",
                                    project_data.project_id, r, e
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("{}", format!("Error getting projects list: {}", e).red());
                std::process::exit(1);
            }
        };
    };

    println!(
        "{:<15} {:<30} {:<15} {:<50} {:<20} {:<25} {:<40}",
        "Status", "Project", "Region", "Deployment ID", "Module", "Version", "Environment",
    );
    for entry in &all_deployments {
        println!(
            "{:<15} {:<30} {:<15} {:<50} {:<20} {:<25} {:<40}",
            entry.status,
            entry.project_id,
            entry.region,
            entry.deployment_id,
            entry.module,
            format!(
                "{}{}",
                &entry.module_version.chars().take(21).collect::<String>(),
                if entry.module_version.len() > 21 {
                    "..."
                } else {
                    ""
                },
            ),
            entry.environment,
        );
    }
}

pub async fn handle_get_claim(deployment_id: &str, environment: &str) {
    match current_region_handler()
        .await
        .get_deployment(deployment_id, environment, false)
        .await
    {
        Ok(deployment) => {
            if let Some(deployment) = deployment {
                let module = current_region_handler()
                    .await
                    .get_module_version(
                        &deployment.module,
                        &deployment.module_track,
                        &deployment.module_version,
                    )
                    .await
                    .unwrap()
                    .unwrap();

                println!(
                    "{}",
                    env_utils::generate_deployment_claim(&deployment, &module)
                );
            } else {
                error!("Deployment not found: {}", deployment_id);
                std::process::exit(1);
            }
        }
        Err(e) => {
            error!("Failed to get claim: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_get_logs(job_id: &str, output_path: Option<&str>) {
    match current_region_handler().await.read_logs(job_id).await {
        Ok(logs) => {
            let log_content = logs
                .iter()
                .map(|log| log.message.as_str())
                .collect::<Vec<&str>>()
                .join("\n");

            match output_path {
                Some(path) => match File::create(path) {
                    Ok(mut file) => match file.write_all(log_content.as_bytes()) {
                        Ok(_) => {
                            println!("Logs successfully written to: {}", path);
                        }
                        Err(e) => {
                            error!("Failed to write logs to file: {}", e);
                            std::process::exit(1);
                        }
                    },
                    Err(e) => {
                        error!("Failed to create file {}: {}", path, e);
                        std::process::exit(1);
                    }
                },
                None => {
                    println!("{}", log_content);
                }
            }
        }
        Err(e) => {
            error!("Failed to get logs for job {}: {}", job_id, e);
            std::process::exit(1);
        }
    }
}
