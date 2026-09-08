use axum::{body::{to_bytes, Body}, http::{header, Request, StatusCode}, middleware::Next, response::{IntoResponse, Response}, Json};
use serde_json::json;

fn code(status:StatusCode)->&'static str{match status{StatusCode::BAD_REQUEST=>"BAD_REQUEST",StatusCode::UNAUTHORIZED=>"UNAUTHORIZED",StatusCode::FORBIDDEN=>"FORBIDDEN",StatusCode::NOT_FOUND=>"NOT_FOUND",StatusCode::CONFLICT=>"CONFLICT",StatusCode::TOO_MANY_REQUESTS=>"RATE_LIMITED",StatusCode::PAYLOAD_TOO_LARGE=>"PAYLOAD_TOO_LARGE",StatusCode::BAD_GATEWAY=>"BAD_GATEWAY",_=>"INTERNAL_ERROR"}}

pub async fn normalize(request:Request<Body>,next:Next)->Response{
    let response=next.run(request).await; let status=response.status(); if status.is_success()||status==StatusCode::NO_CONTENT{return response;}
    let (parts,body)=response.into_parts(); let bytes=to_bytes(body,64*1024).await.unwrap_or_default();
    if let Ok(value)=serde_json::from_slice::<serde_json::Value>(&bytes){if value.get("error").is_some(){return Response::from_parts(parts,Body::from(bytes));}}
    let message=String::from_utf8_lossy(&bytes).trim().to_owned(); let message=if message.is_empty(){status.canonical_reason().unwrap_or("Request failed").to_owned()}else{message};
    let body=Json(json!({"error":{"code":code(status),"message":message}})).into_response().into_body();
    let mut response=Response::from_parts(parts,body); response.headers_mut().insert(header::CONTENT_TYPE,"application/json".parse().unwrap()); response
}
