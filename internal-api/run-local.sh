#!/bin/bash
# Helper script to run the local test server

cd "$(dirname "$0")"

if [ ! -f .env ]; then
    echo "Creating .env file from .env.example..."
    cp .env.example .env
    echo "Please edit .env with your AWS credentials and configuration"
    exit 1
fi

echo "Starting local test server..."
echo ""

# Load environment variables
set -a
source .env
set +a

cargo run --bin internal-api-local --features local,aws
