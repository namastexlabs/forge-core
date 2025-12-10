use axum::response::Json;
use forge_core_utils::response::ApiResponse;

pub async fn health_check() -> Json<ApiResponse<String>> {
    Json(ApiResponse::success("OK".to_string()))
}
