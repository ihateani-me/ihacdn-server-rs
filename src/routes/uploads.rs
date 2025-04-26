use std::sync::Arc;

use axum::{
    Form,
    extract::{Multipart, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use rand::seq::IteratorRandom;
use redis::aio::MultiplexedConnection;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::{
    notifier::{extract_ip_address, notify_discord},
    state::{
        BLOCKED_EXTENSION, CDNData, CREATE_FILE_ERROR, CUSTOM_NAME_GENERATION_ERROR,
        INVALID_URL_FORMAT, MISSING_FIELD, PAYLOAD_TOO_LARGE, PREFIX, REDIS_CONNECTION_ERROR,
        REDIS_SAVE_ERROR, SAVE_FILE_ERROR, SharedState, humanize_bytes,
    },
};

enum ErrorState {
    BlockedExt(String),
    FileTooLarge(u64),
}

struct FileState {
    // skip debug
    chunks: Vec<u8>,
    mime_types: String,
    extension: String,
    real_extension: String,
    file_name: String,
}

impl std::fmt::Debug for FileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileState")
            .field("chunks", &"***")
            .field("mime_types", &self.mime_types)
            .field("extension", &self.extension)
            .field("file_name", &self.file_name)
            .finish()
    }
}

#[derive(Deserialize)]
pub struct ShortenForm {
    url: String,
}

fn randomize_file_name(amount: usize) -> String {
    // alphanumeric
    // generate a random string of alphanumeric characters of the given length
    let chars = "abcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::rng();
    let file_name: String = (0..amount)
        .map(|_| chars.chars().choose(&mut rng).unwrap())
        .collect();
    file_name
}

async fn generate_file_name(
    amount: usize,
    engine: &mut MultiplexedConnection,
) -> Result<String, String> {
    loop {
        let file_name = randomize_file_name(amount);
        let key_exist = match redis::cmd("EXISTS")
            .arg(format!("{PREFIX}{}", file_name))
            .query_async::<i64>(engine)
            .await
        {
            Ok(t) => t > 0,
            Err(err) => {
                tracing::error!("Failed to check redis for existing file name: {}", err);
                return Err("Unable to query redis for existing name".to_string());
            }
        };

        if !key_exist {
            return Ok(file_name);
        }
    }
}

pub(crate) async fn uploads_file(
    State(state): State<Arc<SharedState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> impl IntoResponse {
    // get field "file"
    let secret = match headers.get("x-admin-key") {
        Some(key) => key.to_str().unwrap_or_default(),
        None => "",
    };

    let is_admin = state.config.verify_admin_password(secret);
    let mut connection = match state.make_connection().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::error!("Failed to connect to Redis: {}", err);
            return (StatusCode::INTERNAL_SERVER_ERROR, REDIS_CONNECTION_ERROR).into_response();
        }
    };

    let mut file_state = None;
    while let Ok(Some(mut field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or_default();
        match field_name {
            "file" => {
                let file_name =
                    match generate_file_name(state.config.filename_length, &mut connection).await {
                        Ok(file_name) => file_name,
                        Err(err) => {
                            let error = CUSTOM_NAME_GENERATION_ERROR
                                .to_string()
                                .replace("{{ REASON }}", &err);
                            return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
                        }
                    };

                let file_type = field.content_type().unwrap_or_default();
                let file_name_orig = field.file_name().unwrap_or_default();
                // Split at last dot
                let file_extension = file_name_orig.split('.').last();

                // Check if file type is allowed
                if !state.config.is_filetype_allowed(file_type) {
                    tracing::error!("File type not allowed: {}", file_type);
                    let blocked_ext = BLOCKED_EXTENSION
                        .to_string()
                        .replace("{{ FILE_TYPE }}", file_type);
                    return (StatusCode::UNSUPPORTED_MEDIA_TYPE, blocked_ext).into_response();
                }
                let file_ext_actual = match file_extension {
                    Some(ext) => {
                        if !state.config.is_extension_allowed(ext) {
                            drop(file_state);
                            tracing::error!("File extension not allowed: {}", ext);
                            let blocked_ext = BLOCKED_EXTENSION
                                .to_string()
                                .replace("{{ FILE_TYPE }}", ext);
                            return (StatusCode::UNSUPPORTED_MEDIA_TYPE, blocked_ext)
                                .into_response();
                        }
                        ext
                    }
                    None => "bin",
                }
                .to_string();

                let file_name_actual = format!("{}.{}", file_name, file_ext_actual);
                let file_size_limit = state.config.get_limit(is_admin);

                let mut initial_read = false;
                let mut consumed_length = vec![];
                let mut blocked_state = None;
                let mut guess_type = None;
                while let Ok(Some(chunk)) = field.chunk().await {
                    let consumed_u8 = chunk.as_ref();
                    if !initial_read {
                        // read mimetype via magic number
                        let gtype = tika_magic::from_u8(consumed_u8);
                        if !state.config.is_filetype_allowed(gtype) {
                            blocked_state = Some(ErrorState::BlockedExt(gtype.to_string()));
                            break;
                        }
                        guess_type = Some(gtype.to_string());
                        initial_read = true;
                    }

                    // Check if file size is too large
                    if let Some(file_size_limit) = file_size_limit {
                        let expected_length = consumed_length.len() as u64 + chunk.len() as u64;
                        if expected_length > file_size_limit {
                            blocked_state = Some(ErrorState::FileTooLarge(expected_length));
                            break;
                        }
                    }

                    consumed_length.extend_from_slice(chunk.as_ref());
                }

                if let Some(blocked_state) = blocked_state {
                    drop(consumed_length);

                    match blocked_state {
                        ErrorState::BlockedExt(ext) => {
                            tracing::error!("File extension not allowed: {}", ext);
                            let blocked_ext = BLOCKED_EXTENSION
                                .to_string()
                                .replace("{{ FILE_TYPE }}", &ext);
                            return (StatusCode::UNSUPPORTED_MEDIA_TYPE, blocked_ext)
                                .into_response();
                        }
                        ErrorState::FileTooLarge(size) => {
                            tracing::error!("File size too large: {}", size);
                            let error_msg = PAYLOAD_TOO_LARGE
                                .to_string()
                                .replace("{{ FS }}", &humanize_bytes(file_size_limit.unwrap()))
                                .replace("{{ FN }}", &file_name_actual);
                            // TODO: This will break the connection and browser is fucking dumb and would return NETWORK_ERROR instead of actually the content body
                            return (StatusCode::PAYLOAD_TOO_LARGE, error_msg).into_response();
                        }
                    }
                }

                let guessed_type = guess_type.unwrap_or("application/octet-stream".to_string());
                let guessed_ext = match mime_guess::get_mime_extensions_str(&guessed_type) {
                    Some(exts) => match exts.first() {
                        Some(&ext) => {
                            if ext == "bin" {
                                file_ext_actual.to_string()
                            } else {
                                ext.to_string()
                            }
                        }
                        None => file_ext_actual.to_string(),
                    },
                    None => file_ext_actual.to_string(),
                };

                file_state = Some(FileState {
                    chunks: consumed_length,
                    mime_types: guessed_type,
                    extension: guessed_ext,
                    real_extension: file_ext_actual,
                    file_name,
                });
                break;
            }
            _ => {}
        }
    }

    if file_state.is_none() {
        tracing::error!("No file found in the request");
        return (StatusCode::BAD_REQUEST, MISSING_FIELD).into_response();
    }

    let file_state = file_state.unwrap();
    let is_code = file_state.mime_types.starts_with("text/");
    tracing::info!("File state: {:?}", &file_state);

    // Store to disk
    let base_dir = state.config.get_path(is_admin);
    let file_name_actual = format!("{}.{}", &file_state.file_name, &file_state.real_extension);
    let file_path = base_dir.join(&file_name_actual);

    // Write content to disk
    let mut file = match tokio::fs::File::create(&file_path).await {
        Ok(file) => file,
        Err(err) => {
            tracing::error!("Failed to create file: {}", err);
            let error = CREATE_FILE_ERROR
                .to_string()
                .replace("{{ FN }}", &file_name_actual);
            return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
        }
    };
    match file.write_all(&file_state.chunks).await {
        Err(err) => {
            tracing::error!("Failed to write file: {}", err);
            let error = SAVE_FILE_ERROR
                .to_string()
                .replace("{{ FN }}", &file_name_actual)
                .replace(
                    "{{ REASON }}",
                    &format!(
                        "Unable to write file contents of {} bytes",
                        file_state.chunks.len()
                    ),
                );
            return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
        }
        _ => (),
    }
    match file.flush().await {
        Err(err) => {
            tracing::error!("Failed to flush file: {}", err);
            let error = SAVE_FILE_ERROR
                .to_string()
                .replace("{{ FN }}", &file_name_actual)
                .replace(
                    "{{ REASON }}",
                    &format!(
                        "Unable to flush file contents of {} bytes",
                        file_state.chunks.len()
                    ),
                );
            return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
        }
        _ => (),
    }

    // close file to release the lock
    drop(file);

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Then we create the handle in Redis
    let cdn_data = if is_code {
        CDNData::Code {
            is_admin,
            path: file_path,
            mimetype: file_state.real_extension,
            time_added: current_time,
        }
    } else {
        CDNData::File {
            is_admin,
            path: file_path,
            mimetype: file_state.mime_types,
            time_added: current_time,
        }
    };

    // Set to redis
    match redis::cmd("SET")
        .arg(&format!("{PREFIX}{}", file_state.file_name))
        .arg(serde_json::to_string(&cdn_data).unwrap())
        .exec_async(&mut connection)
        .await
    {
        Ok(_) => (),
        Err(err) => {
            tracing::error!("Failed to set key in Redis: {}", err);
            return (StatusCode::INTERNAL_SERVER_ERROR, REDIS_SAVE_ERROR).into_response();
        }
    }

    let ip_address = extract_ip_address(&headers);
    let final_url = state.config.make_url(&file_name_actual);

    notify_discord(&final_url, cdn_data, &state.config, ip_address);
    return (StatusCode::OK, final_url).into_response();
}

pub(crate) async fn shorten_url(
    State(state): State<Arc<SharedState>>,
    headers: HeaderMap,
    Form(form): Form<ShortenForm>,
) -> impl IntoResponse {
    let mut connection = match state.make_connection().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::error!("Failed to connect to Redis: {}", err);
            return (StatusCode::INTERNAL_SERVER_ERROR, REDIS_CONNECTION_ERROR).into_response();
        }
    };

    let file_name = match generate_file_name(state.config.filename_length, &mut connection).await {
        Ok(file_name) => file_name,
        Err(err) => {
            let error = CUSTOM_NAME_GENERATION_ERROR
                .to_string()
                .replace("{{ REASON }}", &err);
            return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
        }
    };

    let form_url = form.url.trim().to_string();
    // parse as URL
    let parsed_url = match url::Url::parse(&form_url) {
        Ok(url) => url,
        Err(err) => {
            tracing::error!("Failed to parse URL: {}", err);
            let error = INVALID_URL_FORMAT.replace("{{ URL }}", &form_url);
            return (StatusCode::BAD_REQUEST, error).into_response();
        }
    };

    // Then we create the handle in Redis
    let cdn_data = CDNData::Short {
        target: parsed_url.to_string(),
    };

    // Set to redis
    match redis::cmd("SET")
        .arg(&format!("{PREFIX}{}", file_name))
        .arg(serde_json::to_string(&cdn_data).unwrap())
        .exec_async(&mut connection)
        .await
    {
        Ok(_) => (),
        Err(err) => {
            tracing::error!("Failed to set key in Redis: {}", err);
            return (StatusCode::INTERNAL_SERVER_ERROR, REDIS_SAVE_ERROR).into_response();
        }
    }

    let ip_address = extract_ip_address(&headers);
    let final_url = state.config.make_url(&file_name);

    notify_discord(&final_url, cdn_data, &state.config, ip_address);
    return (StatusCode::OK, final_url).into_response();
}
