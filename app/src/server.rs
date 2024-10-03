use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc};

use crate::storage::{Storage, Zblob};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use pflow_metamodel::compression::decompress_brotli_decode;
use pflow_metamodel::oid::Oid;

async fn _src_handler(
    Path(ipfs_cid): Path<String>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zblob = state
        .lock()
        .unwrap()
        .get_by_cid("pflow_models", &ipfs_cid)
        .unwrap_or(Option::from(Zblob::default()))
        .unwrap_or_default();

    let encoded_str = zblob.base64_zipped;
    let data = decompress_brotli_decode(&encoded_str).unwrap_or("".to_string());
    let content_type = "application/json charset=utf-8";
    let status = StatusCode::OK;
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .body(data)
        .unwrap()
}

async fn _img_handler(
    Path(ipfs_cid): Path<String>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zblob = state
        .lock()
        .unwrap()
        .get_by_cid("pflow_models", &ipfs_cid)
        .unwrap_or(Option::from(Zblob::default()))
        .unwrap_or_default();

    let data = decompress_brotli_decode(&zblob.base64_zipped).unwrap_or("".to_string());
    // FIXME: actually convert to SVG
    let content_type = "application/json charset=utf-8";
    let status = StatusCode::OK;
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .body(data)
        .unwrap()
}

fn index_response(cid: String, data: String) -> impl IntoResponse {
    let html = format!(
        r#"<!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="utf-8"/>
            <meta name="viewport" content="width=device-width,initial-scale=1"/>
            <title>pflow.dev | metamodel editor</title>
            <script>
                sessionStorage.cid = "{}";
                sessionStorage.data = "{}";
            </script>
            <script defer="defer" src="https://cdn.jsdelivr.net/gh/pflow-dev/pflow-js@1.1.2/p/static/js/main.5dc69f67.js"> </script>
            <link href="https://cdn.jsdelivr.net/gh/pflow-dev/pflow-js@1.1.2/p/static/css/main.63d515f3.css" rel="stylesheet">
        </head>
        <body>
            <noscript>You need to enable JavaScript to run this app.</noscript>
            <div id="root"></div>
        </body></html>
        "#,
        cid, data
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html charset=utf-8")
        .body(html)
        .unwrap()
}

fn string_to_zblob(data: Option<&String>) -> Zblob {
    let mut zblob = Zblob::default();
    if data.is_some() {
        zblob.base64_zipped = data.unwrap().to_string();
        zblob.ipfs_cid = Oid::new(data.unwrap().as_bytes()).unwrap().to_string();
    }

    zblob
}

async fn index_handler(
    req: Query<HashMap<String, String>>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zblob = string_to_zblob(req.get("z"));
    let zblob = state
        .lock()
        .unwrap()
        .create_or_retrieve(
            "pflow_models",
            &zblob.ipfs_cid,
            &zblob.base64_zipped,
            &zblob.title,
            &zblob.description,
            &zblob.keywords,
            &zblob.referrer,
        )
        .unwrap_or(zblob);

    index_response(zblob.ipfs_cid, zblob.base64_zipped).into_response()
}

pub fn app() -> Router {
    let store = Storage::new("pflow.db").unwrap();
    store.create_tables().unwrap();
    let state: Arc<Mutex<Storage>> = Arc::new(Mutex::new(store));

    // Build route service
    // .layer(TraceLayer::new_for_http())
    Router::new()
        //.route("/img.svg", get(img_handler))
        //.route("/src.json", get(src_handler))
        .route("/", get(index_handler))
        .with_state(state)
}
