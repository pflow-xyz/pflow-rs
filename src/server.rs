use std::{
    collections::HashMap,
    error::Error,
    sync::Arc,
};
use std::ops::Deref;
use std::sync::Mutex;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    Router,
    routing::get,
};
use clap::Parser;
use include_dir::{Dir, include_dir};
use pflow_metamodel::compression::decompress_brotli_decode;
use pflow_metamodel::oid;
use pflow_metamodel::petri_net::PetriNet;
use tower_http::trace::TraceLayer;

use crate::storage::{Storage, Zblob};

async fn src_handler(
    Path(ipfs_cid): Path<String>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zblob = state.lock().unwrap()
        .get_by_cid("pflow_models", &*ipfs_cid)
        .unwrap_or(Option::from(Zblob::default()))
        .unwrap_or(Zblob::default());

    let encoded_str = zblob.base64_zipped;
    let data = decompress_brotli_decode(&*encoded_str).unwrap_or("".to_string());
    let content_type = "application/json charset=utf-8";
    let status = StatusCode::OK;
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .body(data)
        .unwrap()
}

async fn img_handler(
    Path(ipfs_cid): Path<String>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zblob = state.lock().unwrap()
        .get_by_cid("pflow_models", &*ipfs_cid)
        .unwrap_or(Option::from(Zblob::default()))
        .unwrap_or(Zblob::default());

    let data = decompress_brotli_decode(&zblob.base64_zipped).unwrap_or("".to_string());
    let content_type = "application/json charset=utf-8";
    let status = StatusCode::OK;
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .body(data)
        .unwrap()
}

fn index_response(cid: String, data: String) -> impl IntoResponse {
    let html = format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width,initial-scale=1"/>
    <title>pflow.dev | metamodel editor</title>
    <script>
        sessionStorage.cid = "{}";
        sessionStorage.data = "{}".replaceAll(' ', '+');
    </script>
    <script defer="defer" src="https://cdn.jsdelivr.net/gh/pflow-dev/pflow-js@1.1.2/p/static/js/main.5dc69f67.js"> </script>
    <link href="https://cdn.jsdelivr.net/gh/pflow-dev/pflow-js@1.1.2/p/static/css/main.63d515f3.css" rel="stylesheet">
</head>
<body>
    <noscript>You need to enable JavaScript to run this app.</noscript>
    <div id="root"></div>
</body></html>
"#, cid, data);

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html charset=utf-8")
        .body(html)
        .unwrap()
}

fn index_response_redirect(cid: String) -> impl IntoResponse {
    let uri = format!("/p/{}/", cid);
    Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", uri)
        .body("".to_string())
        .unwrap()
}

async fn model_handler(
    Path(ipfs_cid): Path<String>,
    req: Query<HashMap<String, String>>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zparam = req.get("z");
    let zblob = string_to_zblob(zparam);

    let new_blob = state.lock().unwrap().create_or_retrieve(
        "pflow_models",
        &zblob.ipfs_cid,
        &zblob.base64_zipped,
        &zblob.title,
        &zblob.description,
        &zblob.keywords,
        &zblob.referrer,
    ).unwrap_or(zblob);

    if zparam.is_some() && new_blob.id > 0 {
        return index_response_redirect(new_blob.ipfs_cid).into_response();
    }

    let zblob = state.lock().unwrap()
        .get_by_cid("pflow_models", &*ipfs_cid)
        .unwrap_or(Option::from(Zblob::default()))
        .unwrap_or(Zblob::default());

    index_response(zblob.ipfs_cid, zblob.base64_zipped).into_response()
}

fn string_to_zblob(data: Option<&String>) -> Zblob {
    let mut zblob = Zblob::default();
    if data.is_some() {
        zblob.base64_zipped = data.unwrap().to_string();
        zblob.ipfs_cid = oid::Oid::new(data.unwrap().as_bytes()).unwrap().to_string();
    }

    zblob
}

async fn index_handler(
    req: Query<HashMap<String, String>>,
    state: State<Arc<Mutex<Storage>>>,
) -> impl IntoResponse {
    let zblob = string_to_zblob(req.get("z"));
    let new_blob = state.lock().unwrap().create_or_retrieve(
        "pflow_models",
        &zblob.ipfs_cid,
        &zblob.base64_zipped,
        &zblob.title,
        &zblob.description,
        &zblob.keywords,
        &zblob.referrer,
    ).unwrap_or(Zblob::default());

    if new_blob.id > 0 {
        let redirect_uri = format!("/p/{}/", zblob.ipfs_cid);
        return Redirect::permanent(&*redirect_uri).into_response();
    }

    return index_response(zblob.ipfs_cid, zblob.base64_zipped).into_response();
}

const STATIC_DIR: Dir = include_dir!("static");

async fn serve_static(filepath: Path<String>) -> impl IntoResponse {
    let filepath = if filepath.as_str().is_empty() {
        "index.html".to_string()
    } else {
        (*filepath).clone()
    };

    tracing::info!("Serving static file: {:?}", filepath);

    match STATIC_DIR.get_file(&filepath) {
        Some(file) => Html::<axum::body::Body>(file.contents_utf8().unwrap().into()),
        None => Html::<axum::body::Body>("404 Not Found".into()),
    }
}

pub fn app() -> Router {
    let store = Storage::new("pflow.db").unwrap();
    store.create_tables().unwrap();
    let state: Arc<Mutex<Storage>> = Arc::new(Mutex::new(store));

    // Build route service
    Router::new()
        .route("/p/:filepath", get(serve_static)) // serve static files
        .route("/img/:ipfs_cid.svg", get(img_handler))
        .route("/src/:ipfs_cid.json", get(src_handler))
        .route("/p/:ipfs_cid/", get(model_handler))
        .route("/p/", get(get(index_handler)))
        .route("/p", get(|| async { Redirect::to("/p/") }))
        .route("/", get(|| async { Redirect::to("/p/") }))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}