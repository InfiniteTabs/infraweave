# CLI

This package builds the CLI that can be used to interface with InfraWeave, it is used to:

* Publish modules & policies
* Apply/destroy manifests
* View modules

## Architecture & Flows

The CLI supports two execution modes:

### 1. Production Flow (Default)
```
CLI → Direct Lambda Invocation → DynamoDB
```

**Usage:**
```bash
AWS_PROFILE=central AWS_REGION=us-west-2 cargo run --bin cli -- module list dev
AWS_PROFILE=central AWS_REGION=us-west-2 cargo run --bin cli -- module publish ./my-module stable
```

**Characteristics:**
- Uses AWS IAM-based authentication via Lambda invocation
- Lambda receives the IAM identity of the caller
- Enables secure multi-account authorization (Lambda can assume roles into workload accounts)
- No API Gateway or HTTP layer - simpler, more secure for CLI usage
- Suitable for production use, automation, and CI/CD pipelines
- **Used for:** All commands (list, get, publish, apply, etc.)

### 2. Local Development with Direct Access
```
CLI --features local → Direct DynamoDB Access
```

**Usage:**
```bash
# With custom port (check 'docker ps' for actual port)
AWS_REGION=us-west-2 \
AWS_ACCESS_KEY_ID=dummy \
AWS_SECRET_ACCESS_KEY=dummy \
DYNAMODB_ENDPOINT=http://localhost:32803 \
cargo run --bin cli --features local -- module list dev

# Or use default port 8000
AWS_REGION=us-west-2 \
AWS_ACCESS_KEY_ID=dummy \
AWS_SECRET_ACCESS_KEY=dummy \
cargo run --bin cli --features local -- module list dev
```

**Characteristics:**
- Bypasses Lambda and HTTP layers entirely, accesses DynamoDB directly
- Used for quick local development and testing of business logic
- Uses dummy AWS credentials (required for SDK initialization)
- Default endpoint: `http://localhost:8000` (override with `DYNAMODB_ENDPOINT`)
- Requires `AWS_REGION` (any value works, e.g., `us-west-2`)
- Skips infrastructure concerns (HTTP serialization, Lambda invocation) to focus on core logic
- **Used for:** All read operations (list, get, versions) and manifest operations during development
- Fastest option for rapid iteration during development

### Design Rationale

**Why two different flows?**

1. **Production (Direct Lambda)**: Best for production use
   - IAM-based authentication through Lambda identity
   - Lambda can authorize and assume roles into workload accounts
   - No HTTP overhead or API Gateway quotas
   - Native AWS credential management
   - Consistent approach for all CLI operations (read and write)

2. **Local Direct Access**: Best for rapid development iteration
   - Bypasses infrastructure layers (HTTP, Lambda) to focus on business logic
   - Fastest feedback loop for core logic changes
   - No environment variables needed - just use `--features local`
   - Perfect for TDD and rapid prototyping

**Why not use API Gateway for production CLI?**
- API Gateway uses Cognito JWT authentication (designed for web applications)
- CLI benefits from IAM-based authentication for:
  - Simpler credential management (AWS profiles)
  - Better security for automation/CI/CD
  - Native multi-account authorization through IAM role assumption
  - No token refresh logic needed
- Direct Lambda invocation avoids CORS, API Gateway quotas, and additional latency

**API Gateway is still used for:**
- Web application frontend (browser-based access)
- Where Cognito user pools and JWT tokens make sense
- Where CORS, caching, and WAF are beneficial

## Development

If you are developing a new functionality and you are done with unit-tests and integration-tests, you can try it out for real by running `cargo run -p cli <COMMAND> <ARG1> ...` against a live account
