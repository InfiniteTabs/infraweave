use crate::api_common::{self, DatabaseQuery};
use crate::queries::*;
use crate::get_param;
use crate::common::get_env_var; // Assuming common is accessible
use serde_json::{json, Value};
use anyhow::{anyhow, Result};
use axum::response::{IntoResponse, Response};
use log::info;

#[cfg(feature = "aws")]
use crate::aws_handlers::{AwsDatabase as Database, get_user_allowed_projects, download_file, download_file_as_string, get_bucket_name};

#[cfg(feature = "azure")]
use crate::azure_handlers::{AzureDatabase as Database, get_user_allowed_projects, download_file, download_file_as_string};

pub async fn describe_deployment(payload: &Value) -> Result<Value> {
    api_common::describe_deployment_impl(&Database, payload, get_deployment_and_dependents_query)
        .await
}

pub async fn get_deployments(payload: &Value) -> Result<Value> {
    api_common::get_deployments_impl(&Database, payload, get_all_deployments_query).await
}

pub async fn get_modules(payload: &Value) -> Result<Value> {
    api_common::get_modules_impl(&Database, payload, get_all_latest_modules_query).await
}

pub async fn get_projects(payload: &Value) -> Result<Value> {
    let mut result =
        api_common::get_projects_impl(&Database, payload, get_all_projects_query).await?;

    // Filter projects based on user access
    if let Some(user_id) = payload.get("user_id").and_then(|v| v.as_str()) {
        info!("Filtering projects for user_id: {}", user_id);
        // Fetch allowed projects from DB once
        let allowed_projects = get_user_allowed_projects(user_id).await?;

        if let Some(items) = result.get_mut("Items").and_then(|i| i.as_array_mut()) {
            // Filter the items efficiently
            items.retain(|item| {
                let project_id = item
                    .get("project")
                    .or_else(|| item.get("project_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();

                allowed_projects.contains(&project_id.to_string())
            });
        }
    }

    Ok(result)
}

pub async fn get_stacks(payload: &Value) -> Result<Value> {
    api_common::get_stacks_impl(&Database, payload, get_all_latest_stacks_query).await
}

pub async fn get_providers(payload: &Value) -> Result<Value> {
    api_common::get_providers_impl(&Database, payload, get_all_latest_providers_query).await
}

pub async fn get_policies(payload: &Value) -> Result<Value> {
    api_common::get_policies_impl(&Database, payload, get_all_policies_query).await
}

pub async fn get_policy_version(payload: &Value) -> Result<Value> {
    api_common::get_policy_version_impl(&Database, payload, get_policy_query).await
}

pub async fn get_module_version(payload: &Value) -> Result<Value> {
    api_common::get_module_version_impl(&Database, payload, get_module_version_query).await
}

pub async fn get_module_download_url(payload: &Value) -> Result<Response> {
    let module_version =
        api_common::get_module_version_impl(&Database, payload, get_module_version_query)
            .await?;
    let s3_key = module_version
        .get("s3_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Module has no s3_key"))?;

    #[cfg(feature = "aws")]
    let bucket = get_bucket_name("modules")?;
    #[cfg(feature = "azure")]
    let bucket = get_env_var("MODULE_S3_BUCKET")?;
    
    download_file(&bucket, s3_key).await
}

pub async fn get_provider_version(payload: &Value) -> Result<Value> {
    api_common::get_provider_version_impl(&Database, payload, get_provider_version_query).await
}

pub async fn get_provider_download_url(payload: &Value) -> Result<Response> {
    let provider_version =
        api_common::get_provider_version_impl(&Database, payload, get_provider_version_query)
            .await?;
    let s3_key = provider_version
        .get("s3_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Provider has no s3_key"))?;

    #[cfg(feature = "aws")]
    let bucket = get_bucket_name("providers")?;
    #[cfg(feature = "azure")]
    let bucket = get_env_var("PROVIDERS_S3_BUCKET")?;

    download_file(&bucket, s3_key).await
}

pub async fn get_stack_version(payload: &Value) -> Result<Value> {
    api_common::get_stack_version_impl(&Database, payload, get_stack_version_query).await
}

pub async fn get_stack_download_url(payload: &Value) -> Result<Response> {
    let stack_version =
        api_common::get_stack_version_impl(&Database, payload, get_stack_version_query).await?;
    let s3_key = stack_version
        .get("s3_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Stack has no s3_key"))?;

    #[cfg(feature = "aws")]
    let bucket = get_bucket_name("modules")?;
    #[cfg(feature = "azure")]
    let bucket = get_env_var("MODULE_S3_BUCKET")?;

    download_file(&bucket, s3_key).await
}

pub async fn get_all_versions_for_module(payload: &Value) -> Result<Value> {
    api_common::get_all_versions_for_module_impl(
        &Database,
        payload,
        get_all_module_versions_query,
    )
    .await
}

pub async fn get_all_versions_for_stack(payload: &Value) -> Result<Value> {
    api_common::get_all_versions_for_stack_impl(&Database, payload, get_all_stack_versions_query)
        .await
}

pub async fn get_deployments_for_module(payload: &Value) -> Result<Value> {
    api_common::get_deployments_for_module_impl(
        &Database,
        payload,
        get_deployments_using_module_query,
    )
    .await
}

pub async fn get_events(payload: &Value) -> Result<Value> {
    api_common::get_events_impl(&Database, payload, get_events_query).await
}

pub async fn get_change_record(payload: &Value) -> Result<Value> {
    api_common::get_change_record_impl(&Database, payload, get_change_records_query).await
}

pub async fn get_change_record_graph(payload: &Value) -> Result<Response> {
    info!("get_change_record_graph payload: {:?}", payload);
    let change_record =
        match api_common::get_change_record_impl(&Database, payload, get_change_records_query)
            .await
        {
            Ok(cr) => cr,
            Err(e) => {
                log::error!("Failed to fetch change record: {:?}", e);
                return Err(e);
            }
        };

    let plan_key = change_record
        .get("plan_raw_json_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Change record has no plan_raw_json_key"))?;

    let graph_key = plan_key.replace("_mutate_output.json", "_graph.dot");

    #[cfg(feature = "aws")]
    let container_name = get_bucket_name("change_records")?;
    #[cfg(feature = "azure")]
    let container_name = get_env_var("CHANGE_RECORD_S3_BUCKET")?;

    info!(
        "Fetching plan from container: {}, key: {}",
        container_name, plan_key
    );
    let plan_content = download_file_as_string(&container_name, plan_key).await?;
    info!("Plan content length: {}", plan_content.len());

    info!(
        "Fetching graph from container: {}, key: {}",
        container_name, graph_key
    );
    let graph_content = download_file_as_string(&container_name, &graph_key).await?;
    info!("Graph content length: {}", graph_content.len());
    info!("Graph content preview: {:.500}", graph_content);

    let graph = json!({}); // Placeholder until tofu is imported
    // let graph = tofu::process_graph(&plan_content, &graph_content, true, None)
    //     .map_err(|e| anyhow!("Failed to process graph: {}", e))?;

    // info!(
    //     "Processed graph nodes: {}, edges: {}",
    //     graph.nodes.len(),
    //     graph.edges.len()
    // );

    Ok((axum::http::StatusCode::OK, axum::Json(graph)).into_response())
}

pub async fn get_deployment_graph(payload: &Value) -> Result<Response> {
    info!("get_deployment_graph payload: {:?}", payload);
    let project = get_param!(payload, "project");
    let region = get_param!(payload, "region");
    let deployment_id = get_param!(payload, "deployment_id");
    let environment = get_param!(payload, "environment");

    // Get job_id and command from query params (merged into payload)
    let job_id = get_param!(payload, "job_id");

    info!(
        "Using job_id: {} and change_type: {} provided in request",
        job_id, "MUTATE"
    );

    // 2. Fetch the specific change record
    let cr_query = get_change_records_query(
        project,
        region,
        environment,
        deployment_id,
        job_id,
        "MUTATE",
    );

    let cr_resp = Database.query_container("change_records", &cr_query).await?;
    let change_record = cr_resp
        .get("Items")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| anyhow!("Change record not found for job_id: {}", job_id))?;

    let plan_key = change_record
        .get("plan_raw_json_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Change record has no plan_raw_json_key"))?;

    let state_key = plan_key.replace("_mutate_output.json", "_state_output.json");
    let graph_key = plan_key.replace("_mutate_output.json", "_graph.dot");

    #[cfg(feature = "aws")]
    let container_name = get_bucket_name("change_records")?;
    #[cfg(feature = "azure")]
    let container_name = get_env_var("CHANGE_RECORD_S3_BUCKET")?;

    info!(
        "Fetching state from container: {}, key: {}",
        container_name, state_key
    );
    let state_content = download_file_as_string(&container_name, &state_key).await?;

    info!(
        "Fetching graph from container: {}, key: {}",
        container_name, graph_key
    );
    let graph_content = download_file_as_string(&container_name, &graph_key).await?;

    let graph = json!({}); // Placeholder until tofu is imported
    // let graph = tofu::process_graph(&state_content, &graph_content, true, None)
    //     .map_err(|e| anyhow!("Failed to process graph: {}", e))?;

    // info!(
    //     "Processed graph nodes: {}, edges: {}",
    //     graph.nodes.len(),
    //     graph.edges.len()
    // );

    Ok((axum::http::StatusCode::OK, axum::Json(graph)).into_response())
}

pub async fn deprecate_module(payload: &Value) -> Result<Value> {
    use env_common::interface::GenericCloudHandler;
    use env_common::logic::deprecate_module as deprecate_module_impl;

    let module = get_param!(payload, "module");
    let track = get_param!(payload, "track");
    let version = get_param!(payload, "version");
    let message = payload.get("message").and_then(|v| v.as_str());

    // Create a GenericCloudHandler for AWS
    let handler = GenericCloudHandler::default().await;

    // Call the deprecate_module logic
    deprecate_module_impl(&handler, module, track, version, message).await?;

    Ok(json!({
        "success": true,
        "message": format!("Module {} version {} in track {} has been deprecated", module, version, track)
    }))
}

// Re-export or delegate remaining handlers if needed, assuming they are imported from handlers module


// Specialized handlers
#[cfg(feature = "aws")]
pub use crate::aws_handlers::{
    check_project_access, generate_presigned_url, get_environment_variables, get_job_status,
    get_publish_job_status, insert_db, publish_module, publish_notification, read_db, read_logs,
    start_runner, transact_write, upload_file_base64, upload_file_url,
};

#[cfg(feature = "azure")]
pub use crate::azure_handlers::{
    check_project_access, generate_presigned_url, get_environment_variables, get_job_status,
    get_publish_job_status, insert_db, publish_module, publish_notification, read_db, read_logs,
    start_runner, transact_write, upload_file_base64, upload_file_url,
};
