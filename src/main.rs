#[macro_use]
extern crate lazy_static;

use std::env;
use dotenv::dotenv;
use env_logger::{Env, WriteStyle};
use actix_web::{App, HttpResponse, HttpServer, Responder, get, put, delete, web};

mod process;
mod twitch;

lazy_static! {
    static ref CHAT:   process::ProcessHandler<String, process::Chat>       = process::ProcessHandler::new(process::Chat{}, None);
    static ref PLAYER: process::ProcessHandler<String, process::Streamlink> = process::ProcessHandler::new(process::Streamlink{}, Some(|_| CHAT.stop().unwrap()));
    static ref TWITCH: twitch::Twitch                                       = twitch::Twitch::new();
}

#[get("/stream")]
async fn get_stream() -> impl Responder {
    return match PLAYER.running() {
        Some(stream) => HttpResponse::Ok().json(&*stream),
        None         => HttpResponse::NoContent().finish()
    };
}

#[put("/stream")]
async fn open_stream(web::Json(stream): web::Json<String>) -> impl Responder {
    return HttpResponse::Ok().json(&*PLAYER.start(stream).unwrap());
}

#[delete("/stream")]
async fn stop_stream() -> impl Responder {
    PLAYER.stop().unwrap();
    return HttpResponse::NoContent().finish();
}

#[get("/chat")]
async fn get_chat() -> impl Responder {
    return match CHAT.running() {
        Some(stream) => HttpResponse::Ok().json(&*stream),
        None         => HttpResponse::NoContent().finish()
    };
}

#[put("/chat")]
async fn open_chat(web::Json(stream): web::Json<String>) -> impl Responder {
    return HttpResponse::Ok().json(&*CHAT.start(stream).unwrap());
}

#[delete("/chat")]
async fn stop_chat() -> impl Responder {
    CHAT.stop().unwrap();
    return HttpResponse::NoContent().finish();
}

#[get("/twitch/live/{channel}")]
async fn get_twitch_live(web::Path(channel): web::Path<String>) -> impl Responder {
    return HttpResponse::Ok().json(&*TWITCH.get_online_following(channel.to_lowercase()).unwrap());
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
    })
        .bind(env::var("ADDR").unwrap_or("127.0.0.1:23559".to_string()))?
        .run()
        .await
}
