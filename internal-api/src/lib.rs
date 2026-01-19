mod api_common;
mod queries;
#[cfg(feature = "aws")]
pub mod aws_handlers;
#[cfg(feature = "azure")]
pub mod azure_handlers;
mod common;
pub mod handlers;
pub mod http_router;

pub use common::CloudRuntime;
