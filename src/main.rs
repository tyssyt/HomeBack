#[macro_use]
extern crate lazy_static;

use std::env;
use dotenv::dotenv;
use env_logger::{Env, WriteStyle};
use actix_web::{App, HttpResponse, HttpServer, Responder, get, put, post, delete, web, http};
use serde::Deserialize;
use uuid::Uuid;

mod process;
mod twitch;
mod download;

lazy_static! {
    static ref CHAT:             process::ProcessHandler<String, process::Chat>       = process::ProcessHandler::new(process::Chat{}, None);
    static ref PLAYER:           process::ProcessHandler<String, process::Streamlink> = process::ProcessHandler::new(process::Streamlink{}, Some(|_| CHAT.stop().unwrap()));
    static ref TWITCH:           twitch::Twitch                                       = twitch::Twitch::new();
    static ref DOWNLOAD_MANAGER: download::DownloadManager                            = download::DownloadManager::new();
}

#[get("/stream")]
async fn get_stream() -> impl Responder {
    match PLAYER.running() {
        Some(stream) => HttpResponse::Ok().json(&*stream),
        None         => HttpResponse::NoContent().finish()
    }
}

#[put("/stream")]
async fn open_stream(web::Json(stream): web::Json<String>) -> impl Responder {
    HttpResponse::Ok().json(&*PLAYER.start(stream).unwrap())
}

#[delete("/stream")]
async fn stop_stream() -> impl Responder {
    PLAYER.stop().unwrap();
    HttpResponse::NoContent().finish()
}

#[get("/chat")]
async fn get_chat() -> impl Responder {
    match CHAT.running() {
        Some(stream) => HttpResponse::Ok().json(&*stream),
        None         => HttpResponse::NoContent().finish()
    }
}

#[put("/chat")]
async fn open_chat(web::Json(stream): web::Json<String>) -> impl Responder {
    HttpResponse::Ok().json(&*CHAT.start(stream).unwrap())
}

#[delete("/chat")]
async fn stop_chat() -> impl Responder {
    CHAT.stop().unwrap();
    HttpResponse::NoContent().finish()
}

#[get("/twitch/live/{channel}")]
async fn get_twitch_live(web::Path(channel): web::Path<String>) -> impl Responder {
    HttpResponse::Ok().json(&*TWITCH.get_online_following(channel.to_lowercase()).unwrap())
}

#[get("/download/scan")]
async fn get_scans() -> impl Responder {
    HttpResponse::Ok().json(download::read_scan_folder().unwrap())
}

#[get("/download/scan/{file}")]
async fn get_scan(web::Path(file): web::Path<String>) -> impl Responder {
    HttpResponse::Ok().json(download::read_scan_file(file).unwrap())
}

#[delete("/download/scan/{file}")]
async fn delete_scan(web::Path(file): web::Path<String>) -> impl Responder {
    download::delete_scan_file(file).unwrap();
    HttpResponse::NoContent().finish()
}

#[get("/download/files/{file}")]
async fn get_download_file(web::Path(file): web::Path<String>) -> impl Responder {
    if download::find_in_download_folder(&file) {
        HttpResponse::Ok().json(&file)
    } else {
        HttpResponse::NoContent().finish()
    }
}
#[derive(Deserialize)]
struct StupidQueryWrapper {
    file: String,
}
#[get("/download/files")]
async fn get_download_files(web::Query(StupidQueryWrapper{file}): web::Query<StupidQueryWrapper>) -> impl Responder {
    let files = file.split(",");
    let response: Vec<&str> = files.filter(|file| download::find_in_download_folder(file)).collect();
    HttpResponse::Ok().json(response)
}


#[get("/download/{uuid}")]
async fn get_download(web::Path(uuid): web::Path<Uuid>) -> impl Responder {
    match DOWNLOAD_MANAGER.get_download(uuid) {
        Some(download) => HttpResponse::Ok().json(download),
        None           => HttpResponse::NoContent().finish()
    }
}
#[get("/download")]
async fn get_downloads() -> impl Responder {
    HttpResponse::Ok().json(DOWNLOAD_MANAGER.get_downloads())
}

#[derive(Deserialize)]
struct Download {
    url: String,
    path: String,
    query: Option<String>,
}
#[post("/download")]
async fn post_download(web::Json(Download{url, path, query}): web::Json<Download>) -> impl Responder {
    let download = DOWNLOAD_MANAGER.trigger_download(url, path, query).unwrap();
    let location = format!("/download/{}", download.uuid);
    HttpResponse::Created().header(http::header::LOCATION, &*location).json(download)
}

#[delete("/download/{uuid}")]
async fn delete_download(web::Path(uuid): web::Path<Uuid>) -> impl Responder {
    DOWNLOAD_MANAGER.delete_download(uuid);
    HttpResponse::NoContent().finish()
}


#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).write_style(WriteStyle::Always).init();
    HttpServer::new(move || {
        App::new()
            .service(get_stream)
            .service(open_stream)
            .service(stop_stream)
            .service(get_chat)
            .service(open_chat)
            .service(stop_chat)
            .service(get_twitch_live)
            .service(get_scans)
            .service(get_scan)
            .service(delete_scan)
            .service(get_download_file)
            .service(get_download_files)
            .service(get_download)
            .service(get_downloads)
            .service(post_download)
            .service(delete_download)
    })
        .bind(env::var("ADDR").unwrap_or("127.0.0.1:23559".to_string()))?
        .run()
        .await
}
