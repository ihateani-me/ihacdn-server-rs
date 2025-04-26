use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_extra::body::AsyncReadBody;
use tokio::io::AsyncWriteExt;

use crate::{
    state::{
        CDNData, DELETED_ERROR, PREFIX, READ_FILE_ERROR, REDIS_CONNECTION_ERROR, REDIS_GET_ERROR,
        SharedState,
    },
    templating::{HtmlTemplate, TemplateCodeData, TemplatePaste},
};

pub async fn file_reader(
    method: axum::http::Method,
    State(state): State<Arc<SharedState>>,
    Path(id_path): Path<String>,
) -> Response {
    // Placeholder for file reading logic
    let mut connection = match state.make_connection().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::error!("Failed to connect to Redis: {}", err);
            return (StatusCode::INTERNAL_SERVER_ERROR, REDIS_CONNECTION_ERROR).into_response();
        }
    };

    // Split id_path into ID and extension
    let (raw_id, ext) = match id_path.rsplit_once('.') {
        Some((id, ext)) => (id.to_string(), ext.to_string()),
        None => (id_path.clone(), String::new()),
    };

    match redis::cmd("GET")
        .arg(format!("{PREFIX}{}", &raw_id))
        .query_async::<Option<String>>(&mut connection)
        .await
    {
        Ok(Some(data)) => {
            let parsed_data = match serde_json::from_str::<CDNData>(&data) {
                Ok(parsed_data) => parsed_data,
                Err(err) => {
                    tracing::error!("Failed to parse data: {}", err);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to parse data")
                        .into_response();
                }
            };

            match parsed_data {
                CDNData::Code {
                    is_admin: _,
                    path,
                    mimetype,
                    time_added: _,
                } => {
                    if method == axum::http::Method::HEAD {
                        // Peek file if exists
                        let mut builder = axum::http::Response::builder();
                        let headers = builder.headers_mut().unwrap();
                        headers.insert(
                            axum::http::header::CONTENT_TYPE,
                            "text/html; charset=UTF-8".parse().unwrap(),
                        );

                        match tokio::fs::try_exists(path).await {
                            Ok(true) => {
                                return builder
                                    .status(axum::http::StatusCode::OK)
                                    .body(Body::empty())
                                    .unwrap()
                                    .into_response();
                            }
                            Ok(false) => {
                                return builder
                                    .status(axum::http::StatusCode::GONE)
                                    .body(Body::empty())
                                    .unwrap()
                                    .into_response();
                            }
                            Err(err) => {
                                if err.kind() == std::io::ErrorKind::NotFound {
                                    return builder
                                        .status(axum::http::StatusCode::GONE)
                                        .body(Body::empty())
                                        .unwrap()
                                        .into_response();
                                } else {
                                    return builder
                                        .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                                        .body(Body::empty())
                                        .unwrap()
                                        .into_response();
                                };
                            }
                        }
                    }
                    // Check if file exists in the filesystem
                    match tokio::fs::read_to_string(&path).await {
                        Ok(content) => {
                            // Render the HTML content
                            let prefer_type = if ext.is_empty() { mimetype } else { ext };

                            let tpl = TemplatePaste {
                                code_type: prefer_type,
                                code_data: TemplateCodeData::new(content),
                                file_id: raw_id,
                            };
                            HtmlTemplate::new(tpl).into_response()
                        }
                        Err(err) => {
                            if err.kind() == std::io::ErrorKind::NotFound {
                                tracing::warn!("File not found: {}", path.display());
                                let missing_key =
                                    DELETED_ERROR.to_string().replace("{{ FN }}", &id_path);
                                (StatusCode::GONE, missing_key).into_response()
                            } else {
                                tracing::error!("Failed to read file: {}", err);
                                let read_error =
                                    READ_FILE_ERROR.to_string().replace("{{ FN }}", &id_path);
                                (StatusCode::INTERNAL_SERVER_ERROR, read_error).into_response()
                            }
                        }
                    }
                }
                CDNData::File {
                    is_admin: _,
                    path,
                    mimetype,
                    time_added: _,
                } => {
                    // We want to stream the file for images and videos, everything else we want to download
                    let mut stream = match tokio::fs::File::open(&path).await {
                        Ok(file) => file,
                        Err(err) => {
                            if err.kind() == std::io::ErrorKind::NotFound {
                                tracing::warn!("File not found: {}", path.display());
                                let missing_key =
                                    DELETED_ERROR.to_string().replace("{{ FN }}", &id_path);
                                return (StatusCode::GONE, missing_key).into_response();
                            } else {
                                tracing::error!("Failed to read file: {}", err);
                                let read_error =
                                    READ_FILE_ERROR.to_string().replace("{{ FN }}", &id_path);
                                return (StatusCode::INTERNAL_SERVER_ERROR, read_error)
                                    .into_response();
                            }
                        }
                    };
                    let data = match stream.metadata().await {
                        Ok(metadata) => metadata,
                        Err(err) => {
                            tracing::error!("Failed to get metadata: {}", err);
                            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get metadata")
                                .into_response();
                        }
                    };

                    let file_name_part = path.file_name().unwrap_or_default().to_string_lossy();
                    let mut raw_headers = vec![
                        (axum::http::header::CONTENT_TYPE, mimetype.clone()),
                        (axum::http::header::CONTENT_LENGTH, data.len().to_string()),
                    ];

                    let should_stream =
                        mimetype.starts_with("image/") || mimetype.starts_with("video/");
                    if should_stream {
                        raw_headers.push((
                            axum::http::header::CONTENT_DISPOSITION,
                            format!("inline; filename=\"{}\"", file_name_part),
                        ));
                    } else {
                        raw_headers.push((
                            axum::http::header::CONTENT_DISPOSITION,
                            format!("attachment; filename=\"{}\"", file_name_part),
                        ));
                    }

                    let (mut tx, rx) = tokio::io::duplex(64 * 1024);
                    let body = AsyncReadBody::new(rx);

                    if method == axum::http::Method::HEAD {
                        let mut builder = axum::http::Response::builder();
                        let headers = builder.headers_mut().unwrap();
                        for (key, value) in raw_headers {
                            headers.insert(key, value.parse().unwrap());
                        }

                        return builder
                            .status(axum::http::StatusCode::OK)
                            .body(body)
                            .unwrap()
                            .into_response();
                    }

                    tokio::spawn(async move {
                        let _ = tokio::io::copy(&mut stream, &mut tx).await;
                        let _ = tx.flush().await;
                    });

                    let mut builder = axum::http::Response::builder();
                    let headers = builder.headers_mut().unwrap();
                    for (key, value) in raw_headers {
                        headers.insert(key, value.parse().unwrap());
                    }

                    builder
                        .status(StatusCode::OK)
                        .body(body)
                        .unwrap()
                        .into_response()
                }
                CDNData::Short { target } => {
                    (StatusCode::TEMPORARY_REDIRECT, target).into_response()
                }
            }
        }
        Ok(None) => {
            tracing::warn!("No data found for ID: {}", raw_id);
            let missing_key = DELETED_ERROR.to_string().replace("{{ FN }}", &id_path);
            (StatusCode::NOT_FOUND, missing_key).into_response()
        }
        Err(err) => {
            tracing::error!("Failed to get data from Redis: {}", err);
            let fetch_error = REDIS_GET_ERROR.to_string().replace("{{ FN }}", &id_path);
            (StatusCode::INTERNAL_SERVER_ERROR, fetch_error).into_response()
        }
    }
}

pub async fn file_reader_raw(
    method: axum::http::Method,
    State(state): State<Arc<SharedState>>,
    Path(id_path): Path<String>,
) -> Response {
    // Placeholder for file reading logic
    let mut connection = match state.make_connection().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::error!("Failed to connect to Redis: {}", err);
            return (StatusCode::INTERNAL_SERVER_ERROR, REDIS_CONNECTION_ERROR).into_response();
        }
    };

    // Split id_path into ID and extension
    let (raw_id, _) = match id_path.rsplit_once('.') {
        Some((id, ext)) => (id.to_string(), ext.to_string()),
        None => (id_path.clone(), String::new()),
    };

    match redis::cmd("GET")
        .arg(format!("{PREFIX}{}", &raw_id))
        .query_async::<Option<String>>(&mut connection)
        .await
    {
        Ok(Some(data)) => {
            let parsed_data = match serde_json::from_str::<CDNData>(&data) {
                Ok(parsed_data) => parsed_data,
                Err(err) => {
                    tracing::error!("Failed to parse data: {}", err);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to parse data")
                        .into_response();
                }
            };

            match parsed_data {
                CDNData::Code {
                    is_admin: _,
                    path,
                    mimetype,
                    time_added: _,
                } => {
                    let actual_mimetype = match mime_guess::from_ext(&mimetype)
                        .first()
                        .map(|m| m.essence_str().to_string())
                    {
                        Some(mime) => mime,
                        None => "text/plain".to_string(),
                    };

                    if method == axum::http::Method::HEAD {
                        // Peek file if exists
                        let mut builder = axum::http::Response::builder();
                        let headers = builder.headers_mut().unwrap();

                        headers.insert(
                            axum::http::header::CONTENT_TYPE,
                            actual_mimetype.parse().unwrap(),
                        );

                        match tokio::fs::try_exists(path).await {
                            Ok(true) => {
                                return builder
                                    .status(axum::http::StatusCode::OK)
                                    .body(Body::empty())
                                    .unwrap()
                                    .into_response();
                            }
                            Ok(false) => {
                                return builder
                                    .status(axum::http::StatusCode::GONE)
                                    .body(Body::empty())
                                    .unwrap()
                                    .into_response();
                            }
                            Err(err) => {
                                if err.kind() == std::io::ErrorKind::NotFound {
                                    return builder
                                        .status(axum::http::StatusCode::GONE)
                                        .body(Body::empty())
                                        .unwrap()
                                        .into_response();
                                } else {
                                    return builder
                                        .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                                        .body(Body::empty())
                                        .unwrap()
                                        .into_response();
                                };
                            }
                        }
                    };

                    // send as attachment data
                    match tokio::fs::read_to_string(&path).await {
                        Ok(content) => {
                            let builder = axum::http::Response::builder()
                                .header(
                                    axum::http::header::CONTENT_DISPOSITION,
                                    format!(
                                        "attachment; filename=\"{}\"",
                                        path.file_name().unwrap_or_default().to_string_lossy()
                                    ),
                                )
                                .header(axum::http::header::CONTENT_LENGTH, content.len())
                                .header(axum::http::header::CONTENT_TYPE, actual_mimetype)
                                .body(Body::from(content))
                                .unwrap();
                            builder.into_response()
                        }
                        Err(err) => {
                            if err.kind() == std::io::ErrorKind::NotFound {
                                tracing::warn!("File not found: {}", path.display());
                                let missing_key =
                                    DELETED_ERROR.to_string().replace("{{ FN }}", &id_path);
                                (StatusCode::GONE, missing_key).into_response()
                            } else {
                                tracing::error!("Failed to read file: {}", err);
                                let read_error =
                                    READ_FILE_ERROR.to_string().replace("{{ FN }}", &id_path);
                                (StatusCode::INTERNAL_SERVER_ERROR, read_error).into_response()
                            }
                        }
                    }
                }
                CDNData::File { .. } => {
                    let missing_key = DELETED_ERROR.to_string().replace("{{ FN }}", &id_path);
                    (StatusCode::NOT_FOUND, missing_key).into_response()
                }
                CDNData::Short { .. } => {
                    let missing_key = DELETED_ERROR.to_string().replace("{{ FN }}", &id_path);
                    (StatusCode::NOT_FOUND, missing_key).into_response()
                }
            }
        }
        Ok(None) => {
            tracing::warn!("No data found for ID: {}", raw_id);
            let missing_key = DELETED_ERROR.to_string().replace("{{ FN }}", &id_path);
            (StatusCode::NOT_FOUND, missing_key).into_response()
        }
        Err(err) => {
            tracing::error!("Failed to get data from Redis: {}", err);
            let fetch_error = REDIS_GET_ERROR.to_string().replace("{{ FN }}", &id_path);
            (StatusCode::INTERNAL_SERVER_ERROR, fetch_error).into_response()
        }
    }
}
