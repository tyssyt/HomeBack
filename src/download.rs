use std::env;
use std::fs;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use actix_web::rt::spawn;
use futures::StreamExt;
use log::info;
use reqwest::Client;
use uuid::Uuid;
use serde::Serialize;
use super::files::sanitize_path;
use lazy_static::lazy_static;
use regex::Regex;

const MAX_PARALLEL_DOWNLOADS: usize = 4;

lazy_static! {
    static ref SCAN_FOLDER :     PathBuf = PathBuf::from(env::var("SCAN_FOLDER").expect("SCAN_FOLDER not set"));
    static ref DOWNLOAD_FOLDER : PathBuf = PathBuf::from(env::var("DOWNLOAD_FOLDER").expect("DOWNLOAD_FOLDER not set"));
}

pub fn read_scan_folder() -> io::Result<Vec<String>> { 
    Ok(fs::read_dir(&*SCAN_FOLDER)?
        .filter_map(|file| file.ok())
        .filter(|file| file.file_type().map_or(false, |f_type| f_type.is_file()))
        .map(|file| file.file_name().into_string().unwrap())
        .collect())
}

pub fn read_scan_file(file: String) -> io::Result<Vec<String>> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r#"https://[A-Za-z0-9]+?\.hi10an[^>";]*"#).unwrap();
    }

    let content: &str = &fs::read_to_string(SCAN_FOLDER.join(sanitize_path(&file)))?;
    
    let mut links = RE.find_iter(content)
        .map(|m| m.as_str().to_string() )
        .filter(|link| !link.starts_with("https://stream."))
        .collect::<Vec<String>>();

    links.sort();
    links.dedup();

    info!("found {} links in {}", links.len(), file);
    return Ok(links);
}

pub fn read_downloads_subfolder(subfolder: String) -> io::Result<Vec<String>> {
    let files: Vec<_> = fs::read_dir(DOWNLOAD_FOLDER.join(sanitize_path(&subfolder)))?
        .filter_map(|file| file.ok())
        .filter(|file| file.file_type().map_or(false, |f_type| f_type.is_file()))
        .map(|file| file.file_name().into_string().unwrap())
        .collect();

        info!("found {} files in {}", files.len(), subfolder);    
        Ok(files)
}

pub struct DownloadManager {
    client: Client,
    queue: Arc<Mutex<VecDeque<Download>>>,
    active: [Arc<Mutex<Option<Download>>>; MAX_PARALLEL_DOWNLOADS],
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub enum Status {
    Created,
    Running,
    Cancelled,
}

#[derive(Serialize, Clone, Debug)]
pub struct Download {
    status: Status,
    pub uuid: Uuid,
    url: String,
    path: PathBuf,
    current_size: u64,
    size: Option<u64>,
}

#[derive(Serialize)]
pub struct Downloads {    
    queue: Arc<Mutex<VecDeque<Download>>>,    
    active_downloads: Vec<Download>,
}

impl DownloadManager {
    
    pub fn new() -> DownloadManager {
        return DownloadManager { client: Client::new(), queue: Arc::new(Mutex::new(VecDeque::new())), active: Default::default()};
    }

    pub fn get_download(&self, uuid: Uuid) -> Option<Download> {
        // search active downloads
        for download in self.active.iter() {
            let dl = download.lock().unwrap();
            if let Some(d) = &*dl {
                if d.uuid == uuid {
                    return Some(d.clone())
                }
            }
        }

        // search queue
        let q = self.queue.lock().unwrap();
        for download in q.iter() {
            if download.uuid == uuid {
                return Some(download.clone());
            }
        }

        None
    }

    pub fn get_downloads(&self) -> Downloads {
        let active_downloads = self.active.iter()
            .filter_map(|dl| dl.lock().unwrap().clone())
            .collect();
        Downloads { queue: self.queue.clone(), active_downloads }
    }

    pub fn cancel_download(&self, uuid: Uuid) {        
        // search active downloads
        for download in self.active.iter() {
            let mut dl = download.lock().unwrap();
            if let Some(d) = dl.as_mut() {
                if d.uuid == uuid {
                    d.status = Status::Cancelled;
                    return;
                }
            }
        }

        // search queue
        self.queue.lock().unwrap().retain(|dl| dl.uuid != uuid);
    }

    pub fn trigger_download(&'static self, url: String, path: String, query: Option<String>) -> Download {
        let full_url = match query {
                Some(q) => format!("{}?{}", url, q),
                None => url,
        };

        let raw_download = Download{
            status: Status::Created,
            uuid: Uuid::new_v4(),
            url: full_url,
            path: sanitize_path(&path),
            current_size: 0,
            size: None
        };

        // to avoid Deadlocks, we need to lock the queue first
        let mut queue = self.queue.lock().unwrap();

        // check if there is an empty active Download slot
        for slot in self.active.iter() {
            let mut s = slot.lock().unwrap();

            if s.is_some() {continue;}

            *s = Some(raw_download.clone());
            let c2 = self.client.clone();
            let s2 = slot.clone();
            let q2 = self.queue.clone();
            spawn(Self::download_and_queue_next(c2, s2, q2));
            return raw_download;
        }

        // no free slot, add to queue
        queue.push_back(raw_download.clone());
        raw_download
    }

    async fn download_and_queue_next(client: Client, download: Arc<Mutex<Option<Download>>>, queue: Arc<Mutex<VecDeque<Download>>>) -> Result<(), Box<dyn std::error::Error>> {
        let result = Self::download(client.clone(), download.clone()).await;
        // remove the file if the download was cancelled
        if let Ok(Some(path)) = &result {
            info!("Download was Cancelled {:?}", download);
            fs::remove_file(path)?;
        }
        
        Self::queue_next(client, download, queue).await; // make sure this is always called, otherwise the download slot will never be freed
        result.map(|_| ()) // propagate error
    }

    async fn download(client: Client, download: Arc<Mutex<Option<Download>>>) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        let (response_future, path) = {
            let mut dl_guard = download.lock().unwrap();
            
            let mut dl = match dl_guard.as_mut() {
                Some(dl) => dl,
                None => return Err("Should start Download but Mutex is empty".into()),
            };

            dl.status = Status::Running;
            let response_future = client.get(&dl.url).send();
            let path = DOWNLOAD_FOLDER.join(&dl.path);
            (response_future, path)
        };


        // set size
        let response = response_future.await?;
        {
            let mut dl_guard = download.lock().unwrap();
            match dl_guard.as_mut() {
                Some(mut dl) => dl.size = response.content_length(),
                None => return Err("Should set Download Size but Mutex is empty".into()),
            };
        }
        
        // download
        info!("Starting Dowload: {:?}", download);
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = fs::File::create(&path)?;
        let mut stream = response.bytes_stream();
        while let Some(item) = stream.next().await {

            let chunk = item?;
            file.write_all(&chunk)?;

            let mut dl_guard = download.lock().unwrap();
            match dl_guard.as_mut() {
                Some(mut dl) => {
                    dl.current_size += chunk.len() as u64;
                    if dl.status == Status::Cancelled {return Ok(Some(path))}
                },
                None => return Err("Should update Download Size but Mutex is empty".into()),
            };
        }

        info!("Finished Dowload: {:?}", download);
        Ok(None)
    }

    async fn queue_next(client: Client, download: Arc<Mutex<Option<Download>>>, queue: Arc<Mutex<VecDeque<Download>>>) {
        // lock the queue first to avoid deadlocks
        let mut q = queue.lock().unwrap();
        let mut dl_guard = download.lock().unwrap();
        match q.pop_front() {
            Some(new_dl) => {
                *dl_guard = Some(new_dl);
                let dl2 = download.clone();
                let q2 = queue.clone();
                spawn(Self::download_and_queue_next(client, dl2, q2));
            },
            None => *dl_guard = None,
        };
    }
}