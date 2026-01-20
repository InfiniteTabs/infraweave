#[cfg(feature = "aws")]
use anyhow::{anyhow, Result};
#[cfg(feature = "aws")]
use aws_sdk_dynamodb::operation::RequestId;
#[cfg(feature = "aws")]
use axum::{body::Body, http::header, response::Response};
#[cfg(feature = "aws")]
use base64::{engine::general_purpose, Engine as _};
#[cfg(feature = "aws")]
use log::info;
#[cfg(feature = "aws")]
use serde_dynamo::{from_item, to_attribute_value, to_item};
#[cfg(feature = "aws")]
use serde_json::{json, Value};
#[cfg(feature = "aws")]
use std::collections::HashMap;
#[cfg(feature = "aws")]
use tokio_util::io::ReaderStream;

#[cfg(feature = "aws")]
use crate::common::get_env_var;

#[cfg(feature = "aws")]
use crate::api_common::DatabaseQuery;
#[cfg(feature = "aws")]
use crate::get_param;

// #[cfg(feature = "aws")]
// use env_aws::{
//     get_all_deployments_query, get_all_latest_modules_query, get_all_latest_providers_query,
//     get_all_latest_stacks_query, get_all_module_versions_query, get_all_policies_query,
//     get_all_projects_query, get_all_stack_versions_query, get_change_records_query,
//     get_deployment_and_dependents_query, get_deployments_using_module_query, get_events_query,
//     get_module_version_query, get_policy_query, get_provider_version_query,
//     get_stack_version_query,
// };

#[cfg(feature = "aws")]
use cached::proc_macro::cached;

// Helper functions to reduce boilerplate
#[cfg(feature = "aws")]
async fn get_aws_config() -> aws_config::SdkConfig {
    aws_config::from_env().load().await
}

#[cfg(feature = "aws")]
async fn dynamodb_client() -> aws_sdk_dynamodb::Client {
    aws_sdk_dynamodb::Client::new(&get_aws_config().await)
}

#[cfg(feature = "aws")]
async fn s3_client() -> aws_sdk_s3::Client {
    aws_sdk_s3::Client::new(&get_aws_config().await)
}

#[cfg(feature = "aws")]
pub fn get_table_name(table_type: &str) -> Result<String> {
    let env_var = match table_type.to_lowercase().as_str() {
        "events" => "DYNAMODB_EVENTS_TABLE_NAME",
        "modules" => "DYNAMODB_MODULES_TABLE_NAME",
        "deployments" => "DYNAMODB_DEPLOYMENTS_TABLE_NAME",
        "policies" => "DYNAMODB_POLICIES_TABLE_NAME",
        "change_records" | "changerecords" => "DYNAMODB_CHANGE_RECORDS_TABLE_NAME",
        "config" => "DYNAMODB_CONFIG_TABLE_NAME",
        "jobs" => "DYNAMODB_JOBS_TABLE_NAME",
        "permissions" => "DYNAMODB_PERMISSIONS_TABLE_NAME",
        _ => return Err(anyhow!("Unknown table type: {}", table_type)),
    };
    get_env_var(env_var)
}

#[cfg(feature = "aws")]
pub fn get_bucket_name(bucket_type: &str) -> Result<String> {
    let env_var = match bucket_type.to_lowercase().as_str() {
        "modules" => "MODULE_S3_BUCKET",
        "policies" => "POLICY_S3_BUCKET",
        "change_records" | "changerecords" => "CHANGE_RECORD_S3_BUCKET",
        "providers" => "PROVIDERS_S3_BUCKET",
        _ => return Err(anyhow!("Unknown bucket type: {}", bucket_type)),
    };
    get_env_var(env_var)
}

// DatabaseQuery implementation for AWS (DynamoDB)
#[cfg(feature = "aws")]
pub struct AwsDatabase;

#[cfg(feature = "aws")]
impl DatabaseQuery for AwsDatabase {
    async fn query_container(&self, container: &str, query: &Value) -> Result<Value> {
        let payload = json!({
            "table": container,
            "data": {
                "query": query
            }
        });

        read_db(&payload).await
    }
}

#[cfg(feature = "aws")]
pub async fn insert_db(payload: &Value) -> Result<Value> {
    let table = get_param!(payload, "table");
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let table_name = get_table_name(table)?;
    let client = dynamodb_client().await;
    let item = json_to_dynamodb_item(data)?;

    let result = client
        .put_item()
        .table_name(table_name)
        .set_item(Some(item))
        .send()
        .await?;

    Ok(json!({
        "ResponseMetadata": {
            "HTTPStatusCode": 200,
            "RequestId": result.request_id().unwrap_or("")
        }
    }))
}

#[cfg(feature = "aws")]
pub async fn transact_write(payload: &Value) -> Result<Value> {
    let operations = payload
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing 'items' array"))?;
    let client = dynamodb_client().await;

    let mut transact_items = Vec::new();

    for op in operations {
        if let Some(put_op) = op.get("Put") {
            let table_key = put_op
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'TableName' in Put operation"))?;
            let table_name = get_table_name(table_key)?;

            let item_data = put_op
                .get("Item")
                .ok_or_else(|| anyhow!("Missing 'Item' in Put operation"))?;
            let item = json_to_dynamodb_item(item_data)?;

            let put_request = aws_sdk_dynamodb::types::Put::builder()
                .table_name(table_name)
                .set_item(Some(item))
                .build()
                .map_err(|e| anyhow!("Failed to build Put request: {}", e))?;

            transact_items.push(
                aws_sdk_dynamodb::types::TransactWriteItem::builder()
                    .put(put_request)
                    .build(),
            );
        } else if let Some(delete_op) = op.get("Delete") {
            let table_key = delete_op
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'TableName' in Delete operation"))?;
            let table_name = get_table_name(table_key)?;

            let key_data = delete_op
                .get("Key")
                .ok_or_else(|| anyhow!("Missing 'Key' in Delete operation"))?;
            let key = json_to_dynamodb_item(key_data)?;

            let delete_request = aws_sdk_dynamodb::types::Delete::builder()
                .table_name(table_name)
                .set_key(Some(key))
                .build()
                .map_err(|e| anyhow!("Failed to build Delete request: {}", e))?;

            transact_items.push(
                aws_sdk_dynamodb::types::TransactWriteItem::builder()
                    .delete(delete_request)
                    .build(),
            );
        } else {
            return Err(anyhow!("Unknown operation type in transact_write"));
        }
    }

    let _result = client
        .transact_write_items()
        .set_transact_items(Some(transact_items))
        .send()
        .await?;

    Ok(json!({
        "ResponseMetadata": {
            "HTTPStatusCode": 200
        }
    }))
}

#[cfg(feature = "aws")]
pub async fn read_db(payload: &Value) -> Result<Value> {
    let start_time = std::time::Instant::now();
    let table = get_param!(payload, "table");
    let query_data = payload
        .get("data")
        .and_then(|v| v.get("query"))
        .ok_or_else(|| anyhow!("Missing 'query' parameter"))?;

    let table_name = get_table_name(table)?;
    let client = dynamodb_client().await;
    let mut query_builder = client.query().table_name(table_name);

    if let Some(key_condition) = query_data.get("KeyConditionExpression") {
        if let Some(expr) = key_condition.as_str() {
            query_builder = query_builder.key_condition_expression(expr);
        }
    }

    if let Some(filter_expr) = query_data.get("FilterExpression") {
        if let Some(expr) = filter_expr.as_str() {
            query_builder = query_builder.filter_expression(expr);
        }
    }

    if let Some(attr_values) = query_data.get("ExpressionAttributeValues") {
        if let Some(obj) = attr_values.as_object() {
            for (key, value) in obj {
                let attr_value = to_attribute_value(value)?;
                query_builder = query_builder.expression_attribute_values(key, attr_value);
            }
        }
    }

    if let Some(attr_names) = query_data.get("ExpressionAttributeNames") {
        if let Some(obj) = attr_names.as_object() {
            for (key, value) in obj {
                if let Some(name) = value.as_str() {
                    query_builder = query_builder.expression_attribute_names(key, name);
                }
            }
        }
    }

    if let Some(index_name) = query_data.get("IndexName") {
        if let Some(name) = index_name.as_str() {
            query_builder = query_builder.index_name(name);
        }
    }

    if let Some(exclusive_start_key) = query_data.get("ExclusiveStartKey") {
        if let Some(obj) = exclusive_start_key.as_object() {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), to_attribute_value(v)?);
            }
            query_builder = query_builder.set_exclusive_start_key(Some(map));
        }
    }

    if let Some(limit) = query_data.get("Limit") {
        if let Some(num) = limit.as_i64() {
            query_builder = query_builder.limit(num as i32);
        }
    }

    if let Some(scan_forward) = query_data.get("ScanIndexForward") {
        if let Some(val) = scan_forward.as_bool() {
            query_builder = query_builder.scan_index_forward(val);
        }
    }

    let result = query_builder.send().await?;

    let items: Vec<Value> = result
        .items()
        .iter()
        .map(|item| from_item(item.clone()))
        .collect::<Result<Vec<_>, _>>()?;

    let mut response = json!({
        "Items": items,
        "Count": result.count(),
    });

    if let Some(last_key) = result.last_evaluated_key() {
        if !last_key.is_empty() {
            if let Ok(json_key) = from_item::<_, Value>(last_key.clone()) {
                if let Ok(json_str) = serde_json::to_string(&json_key) {
                    let token = general_purpose::STANDARD.encode(json_str);
                    response["next_token"] = json!(token);
                }
            }
        }
    }

    if let Some(consumed_capacity) = result.consumed_capacity() {
        response["ConsumedCapacity"] = json!({
            "TableName": consumed_capacity.table_name(),
            "CapacityUnits": consumed_capacity.capacity_units(),
        });
    }

    let elapsed = start_time.elapsed();
    info!(
        "DB query to table '{}' completed in {:.2}ms. Query: {}",
        table,
        elapsed.as_secs_f64() * 1000.0,
        serde_json::to_string(&query_data).unwrap_or_default()
    );

    Ok(response)
}

#[cfg(feature = "aws")]
pub async fn upload_file_base64(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let bucket_key = data
        .get("bucket_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'bucket_name' parameter"))?;
    let key = data
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'key' parameter"))?;
    let content_base64 = data
        .get("base64_content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'base64_content' parameter"))?;

    let bucket_name = get_bucket_name(bucket_key)?;

    let content = general_purpose::STANDARD
        .decode(content_base64)
        .map_err(|e| anyhow!("Failed to decode base64: {}", e))?;

    let client = s3_client().await;

    client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(content.into())
        .send()
        .await?;

    Ok(json!({
        "statusCode": 200,
        "body": "File uploaded successfully"
    }))
}

#[cfg(feature = "aws")]
pub async fn upload_file_url(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let bucket_key = data
        .get("bucket_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'bucket_name' parameter"))?;
    let key = data
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'key' parameter"))?;
    let url = data
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'url' parameter"))?;

    let bucket_name = get_bucket_name(bucket_key)?;

    let client = s3_client().await;

    match client
        .head_object()
        .bucket(&bucket_name)
        .key(key)
        .send()
        .await
    {
        Ok(_) => {
            return Ok(json!({"object_already_exists": true}));
        }
        Err(_) => {}
    }

    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;

    client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(bytes.to_vec().into())
        .send()
        .await?;

    Ok(json!({"object_already_exists": false}))
}

#[cfg(feature = "aws")]
pub async fn generate_presigned_url(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let key = get_param!(data, "key");
    let bucket_key = data
        .get("bucket_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'bucket_name' parameter"))?;
    let expires_in = data
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(3600);

    let bucket_name = get_bucket_name(bucket_key)?;
    let client = s3_client().await;
    let presigning_config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
        std::time::Duration::from_secs(expires_in as u64),
    )?;

    let presigned_request = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .presigned(presigning_config)
        .await?;

    Ok(json!({
        "url": presigned_request.uri()
    }))
}

#[cfg(feature = "aws")]
pub async fn start_runner(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let cluster = get_env_var("ECS_CLUSTER")?;
    let task_definition = get_env_var("ECS_TASK_DEFINITION")?;
    let subnets = get_env_var("ECS_SUBNETS")?
        .split(',')
        .map(|s| s.to_string())
        .collect::<Vec<String>>();
    let security_groups = get_env_var("ECS_SECURITY_GROUPS")?
        .split(',')
        .map(|s| s.to_string())
        .collect::<Vec<String>>();

    let cpu = data.get("cpu").and_then(|v| v.as_str()).unwrap_or("256");
    let memory = data.get("memory").and_then(|v| v.as_str()).unwrap_or("512");

    let config = get_aws_config().await;
    let ecs_client = aws_sdk_ecs::Client::new(&config);

    let mut environment = Vec::new();
    if let Some(env_vars) = data.get("environment") {
        if let Some(obj) = env_vars.as_object() {
            for (key, value) in obj {
                let env_var = aws_sdk_ecs::types::KeyValuePair::builder()
                    .name(key)
                    .value(value.as_str().unwrap_or(""))
                    .build();
                environment.push(env_var);
            }
        }
    }

    let network_config = aws_sdk_ecs::types::NetworkConfiguration::builder()
        .awsvpc_configuration(
            aws_sdk_ecs::types::AwsVpcConfiguration::builder()
                .set_subnets(Some(subnets))
                .set_security_groups(Some(security_groups))
                .assign_public_ip(aws_sdk_ecs::types::AssignPublicIp::Enabled)
                .build()?,
        )
        .build();

    let container_override = aws_sdk_ecs::types::ContainerOverride::builder()
        .name("runner")
        .set_environment(Some(environment))
        .cpu(cpu.parse::<i32>()?)
        .memory(memory.parse::<i32>()?)
        .build();

    let task_override = aws_sdk_ecs::types::TaskOverride::builder()
        .container_overrides(container_override)
        .cpu(cpu)
        .memory(memory)
        .build();

    let result = ecs_client
        .run_task()
        .cluster(cluster)
        .task_definition(task_definition)
        .launch_type(aws_sdk_ecs::types::LaunchType::Fargate)
        .network_configuration(network_config)
        .overrides(task_override)
        .send()
        .await?;

    let task_arn = result
        .tasks()
        .first()
        .and_then(|t| t.task_arn())
        .ok_or_else(|| anyhow!("No task ARN returned"))?;

    Ok(json!({
        "job_id": task_arn.split('/').last().unwrap_or(task_arn)
    }))
}

#[cfg(feature = "aws")]
pub async fn get_job_status(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let job_id = data
        .get("job_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'job_id' parameter"))?;

    let cluster = get_env_var("ECS_CLUSTER")?;

    let config = get_aws_config().await;
    let ecs_client = aws_sdk_ecs::Client::new(&config);

    let result = ecs_client
        .describe_tasks()
        .cluster(&cluster)
        .tasks(job_id)
        .send()
        .await?;

    let task = result
        .tasks()
        .first()
        .ok_or_else(|| anyhow!("Task not found"))?;

    let status = task.last_status().unwrap_or("UNKNOWN");
    let stopped_reason = task.stopped_reason().unwrap_or("");

    Ok(json!({
        "status": status,
        "stopped_reason": stopped_reason
    }))
}

#[cfg(feature = "aws")]
pub async fn read_logs(payload: &Value) -> Result<Value> {
    log::info!(
        "read_logs called with payload: {}",
        serde_json::to_string(payload).unwrap_or_else(|_| "invalid json".to_string())
    );

    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let job_id = data
        .get("job_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'job_id' parameter"))?;
    let project_id = data
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'project_id' parameter"))?;
    let region = data
        .get("region")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'region' parameter"))?;

    // Optional pagination parameters
    let next_token = data.get("next_token").and_then(|v| v.as_str());
    let limit = data.get("limit").and_then(|v| v.as_i64()).map(|l| l as i32);

    log::info!(
        "read_logs: job_id={}, project_id={}, region={}, next_token={:?}, limit={:?}",
        job_id,
        project_id,
        region,
        next_token,
        limit
    );

    // Get environment from environment variable
    let environment = get_env_var("ENVIRONMENT").map_err(|e| {
        log::error!("Failed to get ENVIRONMENT variable: {}", e);
        e
    })?;

    let central_account_id = get_env_var("CENTRAL_ACCOUNT_ID").map_err(|e| {
        log::error!("Failed to get CENTRAL_ACCOUNT_ID variable: {}", e);
        e
    })?;

    log::info!(
        "read_logs: environment={}, central_account_id={}",
        environment,
        central_account_id
    );

    // Construct log group and stream names
    let log_group = format!("/infraweave/{}/{}/runner", region, environment);
    let log_stream_name = format!("ecs/runner/{}", job_id);

    log::info!(
        "read_logs: log_group={}, log_stream_name={}",
        log_group,
        log_stream_name
    );

    // Check if we need to assume a role in the target project account
    let client = if central_account_id == project_id {
        log::info!("Using current account credentials (central account)");
        let config = get_aws_config().await;
        aws_sdk_cloudwatchlogs::Client::new(&config)
    } else {
        log::info!("Assuming role in target account: {}", project_id);
        let config = get_aws_config().await;
        let sts_client = aws_sdk_sts::Client::new(&config);

        let role_arn = format!(
            "arn:aws:iam::{}:role/infraweave_api_read_log-{}",
            project_id, environment
        );
        log::info!("Assuming role: {}", role_arn);

        let assumed_role = sts_client
            .assume_role()
            .role_arn(&role_arn)
            .role_session_name("CentralApiAssumeRoleSession")
            .send()
            .await
            .map_err(|e| {
                log::error!("Failed to assume role {}: {:?}", role_arn, e);
                anyhow!("Failed to assume role: {:?}", e)
            })?;

        let credentials = assumed_role
            .credentials()
            .ok_or_else(|| anyhow!("No credentials returned from assume role"))?;

        log::info!("Successfully assumed role");

        // Create new config with assumed role credentials
        use aws_credential_types::Credentials;
        let creds = Credentials::new(
            credentials.access_key_id(),
            credentials.secret_access_key(),
            Some(credentials.session_token().to_string()),
            None,
            "AssumedRole",
        );

        let new_config = aws_config::SdkConfig::builder()
            .credentials_provider(
                aws_credential_types::provider::SharedCredentialsProvider::new(creds),
            )
            .region(aws_config::Region::new(region.to_string()))
            .behavior_version(aws_config::BehaviorVersion::latest())
            .build();

        aws_sdk_cloudwatchlogs::Client::new(&new_config)
    };

    log::info!("Fetching log events directly from stream...");
    let mut request = client
        .get_log_events()
        .log_group_name(&log_group)
        .log_stream_name(&log_stream_name);

    // Add pagination parameters if provided
    // Note: start_from_head should only be used when NOT using next_token
    if let Some(token) = next_token {
        log::info!("Using next_token for pagination: {}", token);
        request = request.next_token(token);
    } else {
        // Only set start_from_head when not using pagination token
        request = request.start_from_head(true);
    }

    if let Some(max_items) = limit {
        request = request.limit(max_items);
    }

    let logs_result = request.send().await.map_err(|e| {
        log::error!("Failed to get log events: {:?}", e);
        anyhow!("Failed to get log events: {:?}", e)
    })?;

    let events_count = logs_result.events().len();
    let next_forward_token_result = logs_result.next_forward_token();
    log::info!(
        "Retrieved {} events, input_token={:?}, output_token={:?}",
        events_count,
        next_token,
        next_forward_token_result
    );

    // Concatenate all log messages into a single string (matching webserver-openapi format)
    let mut log_str = String::new();
    for event in logs_result.events() {
        if let Some(message) = event.message() {
            log_str.push_str(message);
            log_str.push('\n');
        }
    }

    // Check if this is the end of the log stream
    // When using a token, CloudWatch returns the same token when there are no MORE events
    // But it still returns the same batch of events around that token
    // So we need to check: same token returned with a token provided = end of stream
    let is_end_of_stream =
        if let (Some(input_token), Some(output_token)) = (next_token, next_forward_token_result) {
            let same_token = input_token == output_token;
            if same_token {
                log::info!("Same token returned - this means we're at the end of available logs");
            }
            same_token
        } else {
            false
        };

    // If we're at the end, return empty logs and no token
    if is_end_of_stream {
        log::info!("End of stream detected - returning empty response");
        return Ok(json!({
            "logs": ""
        }));
    }

    let mut response = json!({
        "logs": log_str
    });

    // Include pagination tokens
    if let Some(next_forward_token) = next_forward_token_result {
        response["nextForwardToken"] = json!(next_forward_token);
        log::info!("Next forward token: {}", next_forward_token);
    }
    if let Some(next_backward_token) = logs_result.next_backward_token() {
        response["nextBackwardToken"] = json!(next_backward_token);
    }

    Ok(response)
}

#[cfg(feature = "aws")]
pub async fn publish_notification(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let message = data
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'message' parameter"))?;
    let subject = data.get("subject").and_then(|v| v.as_str());

    let topic_arn = get_env_var("NOTIFICATION_TOPIC_ARN")?;

    let config = get_aws_config().await;
    let sns_client = aws_sdk_sns::Client::new(&config);

    let mut request = sns_client.publish().topic_arn(topic_arn).message(message);

    if let Some(subj) = subject {
        request = request.subject(subj);
    }

    let result = request.send().await?;

    Ok(json!({
        "message_id": result.message_id().unwrap_or("")
    }))
}

#[cfg(feature = "aws")]
pub async fn get_environment_variables(
    _payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    Ok(json!({
        "DYNAMODB_TF_LOCKS_TABLE_ARN": std::env::var("DYNAMODB_TF_LOCKS_TABLE_ARN").ok(),
        "TF_STATE_S3_BUCKET": std::env::var("TF_STATE_S3_BUCKET").ok(),
        "REGION": std::env::var("REGION").ok(),
    }))
}

#[cfg(feature = "aws")]
fn json_to_dynamodb_item(
    json: &Value,
) -> Result<HashMap<String, aws_sdk_dynamodb::types::AttributeValue>> {
    to_item(json).map_err(|e| anyhow!("{}", e))
}

// API routes from webserver-openapi - MOVED TO handlers.rs

#[cfg(feature = "aws")]
pub async fn download_file_as_string(bucket_name: &str, key: &str) -> Result<String> {
    let client = s3_client().await;
    let object = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?;

    let bytes = object.body.collect().await?.into_bytes();
    let content = String::from_utf8(bytes.to_vec())?;
    Ok(content)
}

#[cfg(feature = "aws")]
pub async fn download_file(bucket_name: &str, key: &str) -> Result<Response> {
    let client = s3_client().await;
    let object = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?;

    let content_length = object.content_length;
    let content_type = object.content_type.unwrap_or_else(|| {
        if key.ends_with(".zip") {
            "application/zip".to_string()
        } else {
            "application/octet-stream".to_string()
        }
    });

    info!(
        "Downloading file from bucket: {}, key: {}. S3 Content Length: {:?}",
        bucket_name, key, content_length
    );

    // aws_sdk_s3::primitives::ByteStream can be converted to AsyncRead
    let stream = ReaderStream::new(object.body.into_async_read());
    let body = Body::from_stream(stream);

    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_str(&content_type)
            .unwrap_or_else(|_| header::HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", key))
            .unwrap_or_else(|_| header::HeaderValue::from_static("attachment")),
    );

    if let Some(len) = content_length {
        if let Ok(val) = header::HeaderValue::from_str(&len.to_string()) {
            response.headers_mut().insert(header::CONTENT_LENGTH, val);
        }
    }

    Ok(response)
}

// get_module_download_url removed

// get_provider_download_url removed

// get_stack_download_url removed

// Common handlers removed: get_all_versions_for_module, get_all_versions_for_stack, get_deployments_for_module, get_events, get_change_record

// Graph handlers moved to handlers.rs

#[cfg(feature = "aws")]
pub async fn publish_module(payload: &Value) -> Result<Value> {
    use base64::Engine;
    use env_common::interface::GenericCloudHandler;
    use env_common::logic::publish_module as publish_module_impl;
    use env_defs::{get_publish_job_identifier, PublishJob};
    use env_utils::{tempdir, unzip_vec_to};

    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let zip_base64 = data
        .get("zip_base64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'zip_base64' parameter"))?;
    let track = data
        .get("track")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'track' parameter"))?;
    let version = data.get("version").and_then(|v| v.as_str());
    let job_id = data
        .get("job_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'job_id' parameter"))?;

    // Create initial job record
    let job = PublishJob::new(job_id.to_string());
    let job_key = get_publish_job_identifier(job_id);

    // Store job in DynamoDB
    let client = dynamodb_client().await;
    let table_name = get_table_name("jobs")?;

    client
        .put_item()
        .table_name(&table_name)
        .item(
            "pk",
            aws_sdk_dynamodb::types::AttributeValue::S(job_key.clone()),
        )
        .item(
            "job_id",
            aws_sdk_dynamodb::types::AttributeValue::S(job.job_id.clone()),
        )
        .item(
            "status",
            aws_sdk_dynamodb::types::AttributeValue::S("processing".to_string()),
        )
        .item(
            "created_at",
            aws_sdk_dynamodb::types::AttributeValue::N(job.created_at.to_string()),
        )
        .item(
            "ttl",
            aws_sdk_dynamodb::types::AttributeValue::N(job.ttl.to_string()),
        )
        .send()
        .await?;

    // Do the work synchronously
    let result: Result<(), anyhow::Error> = async {
        // Decode base64 to get zip bytes
        let zip_bytes = general_purpose::STANDARD
            .decode(zip_base64)
            .map_err(|e| anyhow!("Failed to decode base64: {}", e))?;

        // Create a temporary directory to extract the module
        let temp_dir = tempdir().map_err(|e| anyhow!("Failed to create temp directory: {}", e))?;
        let temp_path = temp_dir.path();

        // Extract zip to temporary directory
        unzip_vec_to(&zip_bytes, temp_path)
            .map_err(|e| anyhow!("Failed to extract zip file: {}", e))?;

        info!("Extracted files to: {:?}", temp_path);
        if let Ok(entries) = std::fs::read_dir(temp_path) {
            for entry in entries.flatten() {
                info!("  - {:?}", entry.file_name());
            }
        }

        // Create a GenericCloudHandler for AWS
        let handler = GenericCloudHandler::default().await;

        // Call publish_module logic with the extracted directory path
        publish_module_impl(
            &handler,
            temp_path
                .to_str()
                .ok_or_else(|| anyhow!("Invalid temp path"))?,
            track,
            version,
            None,
        )
        .await
        .map_err(|e| anyhow!("Failed to publish module: {}", e))?;

        Ok(())
    }
    .await;

    // Update job status based on result
    match result {
        Ok(()) => {
            client
                .update_item()
                .table_name(&table_name)
                .key(
                    "pk",
                    aws_sdk_dynamodb::types::AttributeValue::S(job_key.clone()),
                )
                .update_expression("SET #status = :status, #result = :result")
                .expression_attribute_names("#status", "status")
                .expression_attribute_names("#result", "result")
                .expression_attribute_values(
                    ":status",
                    aws_sdk_dynamodb::types::AttributeValue::S("completed".to_string()),
                )
                .expression_attribute_values(
                    ":result",
                    aws_sdk_dynamodb::types::AttributeValue::S(
                        serde_json::to_string(&json!({
                            "track": track,
                            "version": version
                        }))
                        .unwrap_or_default(),
                    ),
                )
                .send()
                .await?;

            Ok(json!({
                "job_id": job_id,
                "status": "completed"
            }))
        }
        Err(e) => {
            log::error!("Publish module failed: {}", e);
            client
                .update_item()
                .table_name(&table_name)
                .key("pk", aws_sdk_dynamodb::types::AttributeValue::S(job_key))
                .update_expression("SET #status = :status, #error = :error")
                .expression_attribute_names("#status", "status")
                .expression_attribute_names("#error", "error")
                .expression_attribute_values(
                    ":status",
                    aws_sdk_dynamodb::types::AttributeValue::S("failed".to_string()),
                )
                .expression_attribute_values(
                    ":error",
                    aws_sdk_dynamodb::types::AttributeValue::S(e.to_string()),
                )
                .send()
                .await?;

            Ok(json!({
                "job_id": job_id,
                "status": "failed",
                "error": e.to_string()
            }))
        }
    }
}

#[cfg(feature = "aws")]
pub async fn get_publish_job_status(payload: &Value) -> Result<Value> {
    use env_defs::get_publish_job_identifier;

    let job_id = payload
        .get("job_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'job_id' parameter"))?;

    let job_key = get_publish_job_identifier(job_id);

    // Query DynamoDB for job status
    let client = dynamodb_client().await;
    let table_name = get_table_name("jobs")?;

    let result = client
        .get_item()
        .table_name(&table_name)
        .key("pk", aws_sdk_dynamodb::types::AttributeValue::S(job_key))
        .send()
        .await?;

    let item = result.item().ok_or_else(|| anyhow!("Job not found"))?;

    // Convert DynamoDB item to JSON
    let job_data = from_item(item.clone())?;

    Ok(job_data)
}

#[cfg(feature = "aws")]
#[cached(
    time = 300, // Cache for 5 minutes (300 seconds)
    result = true, // Only cache Ok results
    sync_writes = true, // Prevent stampedes
    key = "String",
    convert = r#"{ user_id.to_string() }"#
)]
pub async fn get_user_allowed_projects(user_id: &str) -> Result<Vec<String>> {
    log::info!(
        "Cache miss for user_id: {}. Fetching permissions from DynamoDB.",
        user_id
    );
    // 1. Get the table name
    let table_name = get_table_name("permissions")?;
    let client = dynamodb_client().await;

    // 2. Query the permissions table for the user
    // Assumes Schema: PK = "user_id"
    let result = client
        .get_item()
        .table_name(table_name)
        .key(
            "user_id",
            aws_sdk_dynamodb::types::AttributeValue::S(user_id.to_string()),
        )
        .send()
        .await?;

    // 3. Extract the list of allowed projects
    if let Some(item) = result.item {
        if let Some(projects_attr) = item.get("allowed_projects") {
            if let Ok(projects_list) = projects_attr.as_l() {
                let projects: Result<Vec<String>> = projects_list
                    .iter()
                    .map(|p| {
                        p.as_s()
                            .map(|s| s.clone())
                            .map_err(|_| anyhow!("Invalid project ID format"))
                    })
                    .collect();
                return projects;
            }
        }
    }

    // Default: No access if no record found
    Ok(vec![])
}

#[cfg(feature = "aws")]
pub async fn check_project_access(user_id: &str, project_id: &str) -> Result<bool> {
    let allowed = get_user_allowed_projects(user_id).await?;
    Ok(allowed.contains(&project_id.to_string()))
}

// check_user_access removed in favor of get_user_allowed_projects list lookup
