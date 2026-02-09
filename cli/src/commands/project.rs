use colored::Colorize;

use crate::current_region_handler;
use env_defs::CloudProvider;

pub async fn handle_get_current() {
    match current_region_handler().await.get_current_project().await {
        Ok(project) => {
            println!(
                "Project: {}",
                serde_json::to_string_pretty(&project).unwrap()
            );
        }
        Err(e) => {
            eprintln!("{}", format!("Error: {}", e).red());
            std::process::exit(1);
        }
    }
}

pub async fn handle_get_all() {
    match current_region_handler().await.get_all_projects().await {
        Ok(projects) => {
            if projects.is_empty() {
                println!("No projects found.");
            } else {
                println!("{:<20} {:<50}", "Project ID", "Name");
                println!("{}", "-".repeat(70));
                for project in projects {
                    println!(
                        "{:<20} {:<50}",
                        project.project_id,
                        if project.name.is_empty() {
                            "(no name)"
                        } else {
                            &project.name
                        }
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("{}", format!("Error: {}", e).red());
            std::process::exit(1);
        }
    }
}
