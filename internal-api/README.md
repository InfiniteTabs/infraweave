# Internal API

Multi-cloud serverless API for Infraweave. Supports both direct invocation (AWS legacy) and HTTP (modern).

## Architecture

**Binaries:**
- `internal-api-aws-unified` - AWS Lambda (direct invocation + HTTP via API Gateway)
- `internal-api-azure-unified` - Azure Functions (direct invocation + HTTP)
- `internal-api-local` - Local HTTP server for development

**Modules:**
- `lib.rs` - Module exports and router
- `api_common.rs` - Common API implementations (DatabaseQuery trait)
- `aws_handlers.rs` - AWS operations (DynamoDB, S3, ECS, CloudWatch, SNS)
- `azure_handlers.rs` - Azure operations (Cosmos DB, Blob Storage, ACI, Azure Monitor)
- `http_router.rs` - Axum HTTP router
- `common.rs` - CloudRuntime detection and utilities

## Build

```bash
# AWS
docker build -f internal-api/Dockerfile.lambda -t internal-api-lambda .

# Azure  
docker build -f internal-api/Dockerfile.azure -t internal-api-azure .

# Local
cargo run --bin internal-api-local --features aws
```

## HTTP API

All routes return JSON. See [API_EXAMPLES.md](./API_EXAMPLES.md).

**Deployments:**
- `GET /api/v1/deployment/{project}/{region}/*rest`
- `GET /api/v1/deployments/{project}/{region}`
- `GET /api/v1/deployments/module/{project}/{region}/{module}`
- `GET /api/v1/events/{project}/{region}/*rest`
- `GET /api/v1/change_record/{project}/{region}/*rest`

**Modules & Stacks:**
- `GET /api/v1/modules`
- `GET /api/v1/module/{track}/{module_name}/{module_version}`
- `GET /api/v1/modules/versions/{track}/{module}`
- `GET /api/v1/stacks`
- `GET /api/v1/stack/{track}/{stack_name}/{stack_version}`
- `GET /api/v1/stacks/versions/{track}/{stack}`

**Projects & Policies:**
- `GET /api/v1/projects`
- `GET /api/v1/policies/{environment}`
- `GET /api/v1/policy/{environment}/{policy_name}/{policy_version}`

**Logs:**
- `GET /api/v1/logs/{project}/{region}/{job_id}?limit=100&next_token=...`

## Direct Invocation (AWS Legacy)

For backwards compatibility with Python Lambda callers. Format: `{"event": "EVENT_NAME", ...}`

**Database:** `insert_db`, `transact_write`, `read_db`  
**Storage:** `upload_file_base64`, `upload_file_url`, `generate_presigned_url`  
**Execution:** `start_runner`, `get_job_status`, `read_logs`  
**Other:** `publish_notification`, `get_environment_variables`

## Environment Variables

**AWS:** See [.env](./.env)  
**Azure:** See [.env.azure](./.env.azure)

Key variables:
- `DYNAMODB_*_TABLE_NAME` / `COSMOS_CONTAINER_*` - Database containers
- `*_S3_BUCKET` / `STORAGE_ACCOUNT_NAME` - Object storage
- `REGION`, `ENVIRONMENT` - Infrastructure context
- `CLOUD_PROVIDER` - Optional override (auto-detected)

## Feature Flags

- `aws` - AWS Lambda support (default)
- `azure` - Azure Functions support

Only enable one at a time for minimal binary size.
