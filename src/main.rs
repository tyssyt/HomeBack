#[macro_use]
extern crate lazy_static;

mod process;
mod twitch;
mod download;
mod dvbc;
mod dvbc_preview;
mod files;

use dvbc_preview::ChannelPreview;

use std::env;
use dotenv::dotenv;
use env_logger::{Env, WriteStyle};
use actix_web::{App, HttpResponse, HttpServer, Responder, get, put, post, delete, web, http};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use process::*;

lazy_static! {
    static ref CHAT:             ProcessHandler<String, process::Chat>        = process::ProcessHandler::new(process::Chat{}, None);
    static ref VIDEO_PLAYER:     ProcessHandler<VideoPlayerArgs, VideoPlayer> = ProcessHandler::new(process::VideoPlayer{}, Some(|args, _| if let VideoPlayerArgs::Twitch(_) = args {CHAT.stop().unwrap()}));
    static ref TWITCH:           twitch::Twitch                               = twitch::Twitch::new();
    static ref DOWNLOAD_MANAGER: download::DownloadManager                    = download::DownloadManager::new();
    static ref DVBC:             dvbc::DvbC                                   = dvbc::DvbC::new();
    static ref DVBC_PREVIEWS:    dvbc_preview::DvbCPreviews                   = dvbc_preview::DvbCPreviews::new();
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "uri")]
pub enum VideoPlayerSomthing {
    Twitch(String),
    DvbC(String),
}
impl From<&VideoPlayerArgs> for VideoPlayerSomthing {
    fn from(args: &VideoPlayerArgs) -> Self {
        return match args {
            VideoPlayerArgs::Twitch(stream) => VideoPlayerSomthing::Twitch(stream.clone()),
            VideoPlayerArgs::DvbC(channel) => VideoPlayerSomthing::DvbC(channel.name.clone()),
        };
    }
}

#[get("/videoplayer")]
async fn get_videoplayer() -> impl Responder {
    match VIDEO_PLAYER.running() {
        Some(args) => HttpResponse::Ok().json(VideoPlayerSomthing::from(&*args)),
        None => HttpResponse::NoContent().finish()
    }
}

#[put("/videoplayer")]
async fn start_videoplayer(web::Json(args): web::Json<VideoPlayerSomthing>) -> impl Responder {
    return match args {
        VideoPlayerSomthing::Twitch(stream) => HttpResponse::Ok().json(VideoPlayerSomthing::from(&*VIDEO_PLAYER.start(VideoPlayerArgs::Twitch(stream)).unwrap())),
        VideoPlayerSomthing::DvbC(channel_name) => {                
            match DVBC.get_channels() {
                None => HttpResponse::InternalServerError().finish(), // TODO some return code / header that specifies we couldn't load channels
                Some(channels) => {
                    match channels.tv.iter().find(|channel| channel.name == channel_name) {
                        None => HttpResponse::NotFound().finish(),
                        Some(channel) => HttpResponse::Ok().json(VideoPlayerSomthing::from(&*VIDEO_PLAYER.start(VideoPlayerArgs::DvbC(channel.clone())).unwrap()))
                    }
                }
            }
        }
    }
}

#[delete("/videoplayer")]
async fn stop_videoplayer() -> impl Responder {
    VIDEO_PLAYER.stop().unwrap();
    HttpResponse::NoContent().finish()
}

#[get("/chat")]
async fn get_chat() -> impl Responder {
    match CHAT.running() {
        Some(stream) => HttpResponse::Ok().json(&*stream),
        None => HttpResponse::NoContent().finish(),
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

#[put("/twitch/login")]
async fn put_twitch_login() -> impl Responder {
    HttpResponse::Ok().json(TWITCH.create_user_login().unwrap())
}

#[get("/twitch/login/{id}")]
async fn get_twitch_login(id: web::Path<Uuid>) -> impl Responder {
    if let Some(login) = TWITCH.get_user_login(*id) {
        HttpResponse::Ok().json(login)
    } else {
        HttpResponse::NotFound().finish()
    }
}

#[get("/twitch/live/{id}")]
async fn get_twitch_live(id: web::Path<Uuid>) -> impl Responder {
    if let Some(streams) = TWITCH.get_online_following(*id).unwrap() {
        HttpResponse::Ok().json(streams)
    } else {
        HttpResponse::NotFound().finish()
    }
}

#[get("/download/scan")]
async fn get_scans() -> impl Responder {
    HttpResponse::Ok().json(download::read_scan_folder().unwrap())
}

#[get("/download/scan/{file}")]
async fn get_scan(file: web::Path<String>) -> impl Responder {
    HttpResponse::Ok().json(download::read_scan_file(file.into_inner()).unwrap())
}

#[get("/download/files/{subfolder}")]
async fn get_downloads_subfolder(subfolder: web::Path<String>) -> impl Responder {
    HttpResponse::Ok().json(download::read_downloads_subfolder(subfolder.into_inner()).unwrap())
}


#[get("/download/{uuid}")]
async fn get_download(uuid: web::Path<Uuid>) -> impl Responder {
    match DOWNLOAD_MANAGER.get_download(uuid.into_inner()) {
        Some(download) => HttpResponse::Ok().json(download),
        None => HttpResponse::NoContent().finish(),
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
}
#[post("/download")]
async fn post_download(web::Json(Download{url, path}): web::Json<Download>) -> impl Responder {
    let download = DOWNLOAD_MANAGER.trigger_download(url, path);
    let location = format!("/download/{}", download.uuid);
    HttpResponse::Created().append_header((http::header::LOCATION, &*location)).json(download)
}

#[delete("/download/{uuid}")]
async fn cancel_download(uuid: web::Path<Uuid>) -> impl Responder {
    DOWNLOAD_MANAGER.cancel_download(uuid.into_inner());
    HttpResponse::NoContent().finish()
}

#[get("/dvbc/tv")]
async fn get_dvbc_tv() -> impl Responder {
    match DVBC.get_channels() {
        Some(channels) => { let response: Vec<&String> = channels.tv.iter().map(|c| &c.name).collect(); HttpResponse::Ok().json(response) }
        None => HttpResponse::NoContent().finish(), // TODO some return code that specifies we couldn't load channels
    }
}

#[get("/dvbc/radio")]
async fn get_dvbc_radio() -> impl Responder {
    match DVBC.get_channels() {
        Some(channels) => { let response: Vec<&String> = channels.radio.iter().map(|c| &c.name).collect(); HttpResponse::Ok().json(response) }
        None => HttpResponse::NoContent().finish(), // TODO some return code that specifies we couldn't load channels
    }
}

#[post("/dvbc/tv/previews")] // it's a get with a body...
async fn get_dvbc_tv_previews(web::Json(channel_names): web::Json<Vec<String>>) -> impl Responder {
    match DVBC.get_channels() {
        None => HttpResponse::InternalServerError().finish(), // TODO some return code / header that specifies we couldn't load channels
        Some(channels) => {
            let previews : Vec<Option<ChannelPreview>> = channel_names.iter()
                .map(|name| channels.tv.iter()
                    .find(|channel| &channel.name == name)
                    .map(|channel| DVBC_PREVIEWS.get_preview(channel).unwrap())
            ).collect();
            HttpResponse::Ok().json(&previews)
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).write_style(WriteStyle::Always).init();

    HttpServer::new(move || {
        App::new()
            .service(get_videoplayer)
            .service(start_videoplayer)
            .service(stop_videoplayer)
            .service(get_chat)
            .service(open_chat)
            .service(stop_chat)
            .service(put_twitch_login)
            .service(get_twitch_login)
            .service(get_twitch_live)
            .service(get_scans)
            .service(get_scan)
            .service(get_downloads_subfolder)
            .service(get_download)
            .service(get_downloads)
            .service(post_download)
            .service(cancel_download)
            .service(get_dvbc_tv)
            .service(get_dvbc_radio)
            .service(get_dvbc_tv_previews)
    })
        .bind(env::var("ADDR").unwrap_or("127.0.0.1:23559".to_string()))?
        .run()
        .await
}
