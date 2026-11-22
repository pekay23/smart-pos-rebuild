// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, Json},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use pos_core::{allocate_sale_components, calculate_sale_totals, CalculationError, PricedLine};
use serde::{Deserialize, Serialize};

pub const MAX_CALCULATION_LINES: usize = 500;
pub const MAX_REQUEST_BYTES: usize = 128 * 1024;
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaleCalculationRequest {
    lines: Vec<PricedLineInput>,
    discount_minor: i64,
    tax_rate_bps: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PricedLineInput {
    quantity: i64,
    unit_price_minor: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiEnvelope<T> {
    data: T,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope {
    error: ApiError,
}

#[derive(Debug, Serialize)]
struct ApiError {
    code: &'static str,
    message: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaleCalculationResponse {
    subtotal_minor: i64,
    discount_minor: i64,
    tax_minor: i64,
    total_minor: i64,
    lines: Vec<LineAllocationResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LineAllocationResponse {
    line_subtotal_minor: i64,
    discount_minor: i64,
    tax_minor: i64,
}

#[derive(Debug, Clone, Copy)]
struct ApiFailure {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorEnvelope {
                error: ApiError {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}

pub(crate) fn router() -> Router {
    Router::new()
        .route("/sale-calculations", post(calculate))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
}

async fn calculate(
    payload: Result<Json<SaleCalculationRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<SaleCalculationResponse>>, ApiFailure> {
    let Json(input) = payload.map_err(|rejection| {
        let (status, code, message) = match rejection.status() {
            StatusCode::PAYLOAD_TOO_LARGE => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
                "The request body is larger than the supported limit.",
            ),
            StatusCode::UNSUPPORTED_MEDIA_TYPE => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "Send a JSON request with Content-Type: application/json.",
            ),
            _ => (
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "The request body must match the sale calculation schema.",
            ),
        };
        ApiFailure {
            status,
            code,
            message,
        }
    })?;

    if input.lines.is_empty() {
        return Err(invalid_calculation(CalculationError::EmptySale));
    }
    if input.lines.len() > MAX_CALCULATION_LINES {
        return Err(ApiFailure {
            status: StatusCode::BAD_REQUEST,
            code: "too_many_lines",
            message: "A calculation may contain at most 500 lines.",
        });
    }
    if !is_safe_integer(input.discount_minor)
        || !is_safe_integer(input.tax_rate_bps)
        || input
            .lines
            .iter()
            .any(|line| !is_safe_integer(line.quantity) || !is_safe_integer(line.unit_price_minor))
    {
        return Err(ApiFailure {
            status: StatusCode::BAD_REQUEST,
            code: "unsafe_integer",
            message: "All numeric inputs must be within the supported safe integer range.",
        });
    }

    let lines = input
        .lines
        .into_iter()
        .map(|line| PricedLine {
            quantity: line.quantity,
            unit_price_minor: line.unit_price_minor,
        })
        .collect::<Vec<_>>();
    let totals = calculate_sale_totals(&lines, input.discount_minor, input.tax_rate_bps)
        .map_err(invalid_calculation)?;
    let allocations = allocate_sale_components(&lines, totals).map_err(invalid_calculation)?;

    if [
        totals.subtotal_minor,
        totals.discount_minor,
        totals.tax_minor,
        totals.total_minor,
    ]
    .into_iter()
    .any(|amount| !is_safe_integer(amount))
        || allocations.iter().any(|line| {
            !is_safe_integer(line.line_subtotal_minor)
                || !is_safe_integer(line.discount_minor)
                || !is_safe_integer(line.tax_minor)
        })
    {
        return Err(ApiFailure {
            status: StatusCode::BAD_REQUEST,
            code: "unsafe_result",
            message: "The calculated amounts exceed the supported safe integer range.",
        });
    }

    Ok(Json(ApiEnvelope {
        data: SaleCalculationResponse {
            subtotal_minor: totals.subtotal_minor,
            discount_minor: totals.discount_minor,
            tax_minor: totals.tax_minor,
            total_minor: totals.total_minor,
            lines: allocations
                .into_iter()
                .map(|line| LineAllocationResponse {
                    line_subtotal_minor: line.line_subtotal_minor,
                    discount_minor: line.discount_minor,
                    tax_minor: line.tax_minor,
                })
                .collect(),
        },
    }))
}

fn is_safe_integer(value: i64) -> bool {
    (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value)
}

fn invalid_calculation(error: CalculationError) -> ApiFailure {
    let (code, message) = match error {
        CalculationError::EmptySale => ("empty_sale", "A calculation needs at least one item."),
        CalculationError::InvalidQuantity => (
            "invalid_quantity",
            "Item quantities must be positive whole numbers.",
        ),
        CalculationError::NegativePrice => ("negative_price", "Item prices cannot be negative."),
        CalculationError::InvalidDiscount => (
            "invalid_discount",
            "The discount must be between zero and the subtotal.",
        ),
        CalculationError::InvalidTaxRate => (
            "invalid_tax_rate",
            "The tax rate must be between 0 and 100 percent.",
        ),
        CalculationError::Overflow => (
            "calculation_overflow",
            "The calculated amount exceeds the supported range.",
        ),
    };
    ApiFailure {
        status: StatusCode::BAD_REQUEST,
        code,
        message,
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{header, Method, Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::{MAX_CALCULATION_LINES, MAX_REQUEST_BYTES};
    use crate::app;

    async fn send<B>(
        method: Method,
        uri: &str,
        body: B,
        content_type: Option<&'static str>,
    ) -> (StatusCode, String)
    where
        B: Into<Body>,
    {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        let response = app(Vec::new())
            .oneshot(builder.body(body.into()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), MAX_REQUEST_BYTES + 1)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn versioned_calculation_matches_pos_core_contract() {
        let (status, body) = send(
            Method::POST,
            "/v1/sale-calculations",
            r#"{"lines":[{"quantity":2,"unitPriceMinor":1250},{"quantity":1,"unitPriceMinor":500}],"discountMinor":100,"taxRateBps":1500}"#,
            Some("application/json"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["data"]["subtotalMinor"], 3000);
        assert_eq!(value["data"]["discountMinor"], 100);
        assert_eq!(value["data"]["taxMinor"], 435);
        assert_eq!(value["data"]["totalMinor"], 3335);
        assert_eq!(value["data"]["lines"][0]["discountMinor"], 83);
    }

    #[tokio::test]
    async fn validation_errors_are_stable_and_do_not_expose_internal_text() {
        let (status, body) = send(
            Method::POST,
            "/v1/sale-calculations",
            r#"{"lines":[{"quantity":0,"unitPriceMinor":500}],"discountMinor":0,"taxRateBps":0}"#,
            Some("application/json"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["error"]["code"], "invalid_quantity");
        assert_eq!(
            value["error"]["message"],
            "Item quantities must be positive whole numbers."
        );
    }

    #[tokio::test]
    async fn rejects_oversized_line_sets_and_payloads() {
        let request = serde_json::json!({
            "lines": (0..=MAX_CALCULATION_LINES)
                .map(|_| serde_json::json!({"quantity": 1, "unitPriceMinor": 1}))
                .collect::<Vec<_>>(),
            "discountMinor": 0,
            "taxRateBps": 0
        });
        let (status, body) = send(
            Method::POST,
            "/v1/sale-calculations",
            request.to_string(),
            Some("application/json"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["error"]["code"], "too_many_lines");

        let oversized = format!(
            "{{\"lines\":[],\"discountMinor\":0,\"taxRateBps\":0}}{}",
            " ".repeat(MAX_REQUEST_BYTES)
        );
        let (status, body) = send(
            Method::POST,
            "/v1/sale-calculations",
            oversized,
            Some("application/json"),
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["error"]["code"], "request_too_large");
    }

    #[tokio::test]
    async fn maps_invalid_json_to_stable_error_envelope() {
        let (status, body) = send(
            Method::POST,
            "/v1/sale-calculations",
            "{not json}",
            Some("application/json"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["error"]["code"], "invalid_json");
        assert_eq!(
            value["error"]["message"],
            "The request body must match the sale calculation schema."
        );
    }

    #[tokio::test]
    async fn health_is_versioned_minimal_and_has_no_tenant_data_routes() {
        let response = app(Vec::new())
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 64).await.unwrap()).unwrap();
        assert_eq!(value["status"], "ok");
        assert!(value.get("businessId").is_none());
        assert_eq!(
            app(Vec::new())
                .oneshot(
                    Request::get("/v1/catalog/products")
                        .body(Body::empty())
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
}
